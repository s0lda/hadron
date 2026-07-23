"""ACP stdio adapter for the Antigravity (Gemini) Python SDK.

Speaks line-delimited JSON-RPC on stdin/stdout — the transport `hadron-gluon`'s
ACP adapter drives — and maps it onto `google.antigravity.Agent`.

AUTH: the SDK accepts an API key or a Vertex project and NOTHING else. It has no
OAuth path, so the credentials the `agy` CLI logs in with (~/.gemini/oauth_creds.json)
are useless here: this seat needs GEMINI_API_KEY in the daemon's environment.
"""

import sys
import json
import uuid
import logging
import asyncio
from typing import Any, Dict
import os

from google.antigravity import Agent, LocalAgentConfig
from google.antigravity.types import Text, Thought

# The model this seat runs. The SDK's own DEFAULT_MODEL is gemini-3.5-flash, which
# has been returning 503 "high demand" errors; gemini-3.6-flash is its current
# sibling (same SDK naming scheme — the thinking level is a separate SDK param, so
# there is no `-high/-low` suffix here, unlike the agy CLI's model ids). Pinned in
# ONE place and used for BOTH what we pass to the Agent and what session/new
# advertises, so the picker can never advertise a model we don't actually run.
SEAT_MODEL = "gemini-3.6-flash"

# stdout is the protocol. Anything that prints to it — a stray `print`, a chatty
# dependency — corrupts the JSON-RPC stream and takes the seat down with no clue
# why. Keep the real handle to ourselves and point everything else at stderr.
_protocol_out = sys.stdout
sys.stdout = sys.stderr

logging.basicConfig(level=logging.INFO, stream=sys.stderr)
logger = logging.getLogger("agy_acp")

sessions: Dict[str, Any] = {}

NO_KEY = (
    "GEMINI_API_KEY is not set in this process's environment. The Antigravity SDK "
    "authenticates with an API key or a Vertex project only — it cannot use the OAuth "
    "login the agy CLI uses, so this seat cannot start without one."
)


def _write(payload):
    _protocol_out.write(json.dumps(payload) + "\n")
    _protocol_out.flush()


def send_response(msg_id, result):
    _write({"jsonrpc": "2.0", "id": msg_id, "result": result})


def send_error(msg_id, message, code=-32000):
    """A failure the client must see AS a failure.

    An error stuffed inside `result` is not a JSON-RPC error: the client reads it
    as a success, and the seat then fails later for reasons nobody can trace.
    """
    _write({"jsonrpc": "2.0", "id": msg_id, "error": {"code": code, "message": message}})


def send_notification(method, params):
    _write({"jsonrpc": "2.0", "method": method, "params": params})


def send_session_update(session_id, update):
    """Emit one ACP `session/update` notification — the ONLY way a turn's text,
    thoughts, or usage reach the client.

    The wire contract is the ACP schema's `SessionNotification`/`SessionUpdate`,
    and it is unforgiving in two places that are easy to get wrong:

      * the method is `session/update` (NOT `session/notification`): the Rust
        client routes notifications by method name, so a wrong method never
        reaches the handler at all;
      * the update's discriminator field is `sessionUpdate` (NOT `type`):
        `SessionUpdate` is `#[serde(tag = "sessionUpdate")]`, so a wrong tag
        fails to deserialize and the update is dropped.

    Get either wrong and every chunk is silently discarded — the turn bills
    tokens and posts an empty message. So the wire shape lives HERE, once.
    """
    send_notification("session/update", {"sessionId": session_id, "update": update})


async def handle_prompt(msg_id, session_id, prompt):
    session_data = sessions.get(session_id)
    if not session_data:
        send_error(msg_id, f"unknown session {session_id!r}")
        return

    # Boot the SDK Agent lazily, on the FIRST prompt — not at session/new. Model
    # detection (Settings' probe) opens a session but never prompts, so it must
    # not require the key or a live Google connection; a real turn does. Keep the
    # old init_session error text so a boot failure still reads the same way.
    try:
        agent = await ensure_agent(session_data)
    except Exception as e:
        logger.error(f"could not start the Antigravity agent: {e}")
        send_error(msg_id, f"could not start the Antigravity agent: {e}")
        return

    prompt_text = "\n".join(
        b.get("text", "") for b in prompt if b.get("type") == "text"
    )

    try:
        response = await agent.chat(prompt_text)
        text_accumulated = []
        thought_accumulated = []
        async for chunk in response.chunks:
            if isinstance(chunk, Text):
                text_accumulated.append(chunk.text)
                send_session_update(session_id, {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": chunk.text}
                })
            elif isinstance(chunk, Thought):
                thought_accumulated.append(chunk.text)
                send_session_update(session_id, {
                    "sessionUpdate": "agent_thought_chunk",
                    "content": {"type": "text", "text": chunk.text}
                })

        # Fallback if no Text chunks were streamed (e.g. model generated thoughts or text only via resolve())
        if not text_accumulated:
            fallback_text = await response.text()
            if not fallback_text and thought_accumulated:
                fallback_text = "".join(thought_accumulated)
            if fallback_text:
                send_session_update(session_id, {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": fallback_text}
                })

        usage = response.usage_metadata
        if usage:
            send_session_update(session_id, {
                "sessionUpdate": "usage_update",
                "used": getattr(usage, "total_token_count", 0),
                "size": getattr(usage, "context_window_size", 2000000)
            })

        send_response(msg_id, {
            "stopReason": "end_turn",
            "usage": {
                "inputTokens": getattr(usage, "prompt_token_count", 0),
                "outputTokens": getattr(usage, "candidates_token_count", 0)
            } if usage else None
        })
    except Exception as e:
        # A bare `stopReason: error` tells the swarm the turn failed and nothing
        # about why. Hand back the reason — the client turns it into a real error.
        logger.error(f"turn failed: {e}")
        sessions.pop(session_id, None)
        send_error(msg_id, f"Antigravity SDK turn failed: {e}")


def session_config_response(session_id):
    """The static capabilities a `session/new` advertises: the one model this
    seat runs. Fixed and known, so reporting it needs NO live connection and NO
    key — the Settings model probe reads exactly this. Booting the SDK here was
    what made model detection fail (and bill a connection) whenever the key or
    Google was momentarily unreachable."""
    return {
        "sessionId": session_id,
        "configOptions": [
            {
                "id": "model",
                "name": "Model",
                "type": "select",
                # Without `category`, the client does not recognise this as
                # the model selector at all.
                "category": "model",
                # Advertise the model we ACTUALLY run (SEAT_MODEL, passed to the
                # Agent in ensure_agent). Offering a model we never pass to the
                # SDK is a picker that lies.
                "currentValue": SEAT_MODEL,
                "options": [{"value": SEAT_MODEL, "name": SEAT_MODEL}]
            }
        ]
    }


async def ensure_agent(session_data):
    """Boot the SDK Agent on first use and cache it on the session. Deferred from
    session/new so model detection needs no key or live connection; a real turn
    does. Raises with a human-readable reason the caller turns into an error."""
    if session_data.get("agent") is not None:
        return session_data["agent"]
    api_key = os.environ.get("GEMINI_API_KEY", "")
    if not api_key:
        raise RuntimeError(NO_KEY)
    cwd = session_data.get("cwd") or ""
    agent = Agent(
        config=LocalAgentConfig(
            api_key=api_key,
            model=SEAT_MODEL,
            workspaces=[cwd] if cwd else None
        )
    )
    await agent.__aenter__()
    session_data["agent"] = agent
    return agent


async def main():
    loop = asyncio.get_running_loop()

    while True:
        line = await loop.run_in_executor(None, sys.stdin.readline)
        if not line:
            break

        try:
            req = json.loads(line)
        except Exception:
            continue

        method = req.get("method")
        msg_id = req.get("id")
        params = req.get("params", {})

        if method == "initialize":
            send_response(msg_id, {
                "protocolVersion": params.get("protocolVersion", 1),
                "agentInfo": {"name": "Antigravity (SDK)", "version": "0.1.0"}
            })

        elif method == "session/new":
            # Register the session and advertise the static model list WITHOUT a
            # key or an SDK boot. The Agent is booted lazily on the first prompt
            # (ensure_agent), where the key is genuinely required. This keeps the
            # model probe fast, free, and independent of Google reachability.
            session_id = str(uuid.uuid4())
            sessions[session_id] = {
                "agent": None,
                "model": SEAT_MODEL,
                "cwd": params.get("cwd", ""),
            }
            send_response(msg_id, session_config_response(session_id))

        elif method == "session/set_config_option":
            session_id = params.get("sessionId")
            config_id = params.get("configId")
            value = str(params.get("value"))
            if session_id not in sessions:
                send_error(msg_id, f"unknown session {session_id!r}")
            elif config_id == "model" and value != SEAT_MODEL:
                # We only ever run SEAT_MODEL. Refusing loudly is honest; the
                # client logs the refusal and stays on the pinned model.
                send_error(msg_id, f"this seat runs {SEAT_MODEL} and cannot switch to {value!r}")
            else:
                send_response(msg_id, {})

        elif method == "session/prompt":
            session_id = params.get("sessionId")
            prompt = params.get("prompt", [])
            asyncio.create_task(handle_prompt(msg_id, session_id, prompt))

        elif msg_id is not None:
            send_response(msg_id, {})


if __name__ == "__main__":
    asyncio.run(main())
