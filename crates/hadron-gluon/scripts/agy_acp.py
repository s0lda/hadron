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
from google.antigravity.models import DEFAULT_MODEL

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


async def handle_prompt(msg_id, session_id, prompt):
    session_data = sessions.get(session_id)
    if not session_data:
        send_error(msg_id, f"unknown session {session_id!r}")
        return

    agent = session_data["agent"]

    prompt_text = "\n".join(
        b.get("text", "") for b in prompt if b.get("type") == "text"
    )

    try:
        response = await agent.chat(prompt_text)
        async for chunk in response:
            if chunk:
                send_notification("session/notification", {
                    "sessionId": session_id,
                    "update": {
                        "type": "agent_message_chunk",
                        "content": {"type": "text", "text": chunk}
                    }
                })

        usage = response.usage_metadata
        if usage:
            send_notification("session/notification", {
                "sessionId": session_id,
                "update": {
                    "type": "usage_update",
                    "used": getattr(usage, "total_token_count", 0),
                    "size": getattr(usage, "context_window_size", 2000000)
                }
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
        send_error(msg_id, f"Antigravity SDK turn failed: {e}")


async def init_session(msg_id, session_id, agent):
    try:
        await agent.__aenter__()
        sessions[session_id] = {"agent": agent, "model": DEFAULT_MODEL}
        send_response(msg_id, {
            "sessionId": session_id,
            "configOptions": [
                {
                    "id": "model",
                    "name": "Model",
                    "type": "select",
                    # Without `category`, the client does not recognise this as
                    # the model selector at all.
                    "category": "model",
                    # Advertise the model we ACTUALLY run — the SDK's own default,
                    # imported rather than copied. Offering a model we never pass
                    # to the SDK is a picker that lies.
                    "currentValue": DEFAULT_MODEL,
                    "options": [{"value": DEFAULT_MODEL, "name": DEFAULT_MODEL}]
                }
            ]
        })
    except Exception as e:
        logger.error(f"session/new failed: {e}")
        send_error(msg_id, f"could not start the Antigravity agent: {e}")


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
            api_key = os.environ.get("GEMINI_API_KEY", "")
            if not api_key:
                # Fail here, with the reason. The SDK would otherwise raise deep
                # inside its connection code and the seat would just say "error".
                logger.error(NO_KEY)
                send_error(msg_id, NO_KEY)
                continue

            session_id = str(uuid.uuid4())
            cwd = params.get("cwd", "")
            agent = Agent(
                config=LocalAgentConfig(
                    api_key=api_key,
                    workspaces=[cwd] if cwd else None
                )
            )
            asyncio.create_task(init_session(msg_id, session_id, agent))

        elif method == "session/set_config_option":
            session_id = params.get("sessionId")
            config_id = params.get("configId")
            value = str(params.get("value"))
            if session_id not in sessions:
                send_error(msg_id, f"unknown session {session_id!r}")
            elif config_id == "model" and value != DEFAULT_MODEL:
                # We only ever run DEFAULT_MODEL. Refusing loudly is honest; the
                # client logs the refusal and stays on the default.
                send_error(msg_id, f"this seat runs {DEFAULT_MODEL} and cannot switch to {value!r}")
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
