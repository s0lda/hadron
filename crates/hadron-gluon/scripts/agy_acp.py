import sys
import json
import uuid
import logging
import asyncio
from typing import Optional, Dict, Any
import os

from google.antigravity import Agent, LocalAgentConfig

logging.basicConfig(level=logging.INFO, stream=sys.stderr)
logger = logging.getLogger("agy_acp")

sessions: Dict[str, Any] = {}

def send_response(msg_id, result):
    sys.stdout.write(json.dumps({
        "jsonrpc": "2.0",
        "id": msg_id,
        "result": result
    }) + "\n")
    sys.stdout.flush()

def send_notification(method, params):
    sys.stdout.write(json.dumps({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    }) + "\n")
    sys.stdout.flush()

async def handle_prompt(msg_id, session_id, prompt):
    session_data = sessions.get(session_id)
    if not session_data:
        send_response(msg_id, {"stopReason": "error"})
        return

    agent = session_data["agent"]
    
    # prompt is a list of blocks
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
                "inputTokens": getattr(usage, "prompt_token_count", 0) if usage else 0,
                "outputTokens": getattr(usage, "candidates_token_count", 0) if usage else 0
            } if usage else None
        })
    except Exception as e:
        logger.error(f"Error: {e}")
        send_response(msg_id, {"stopReason": "error"})

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
                "agentInfo": {"name": "Antigravity ACP (Python)", "version": "0.1.0"}
            })
            
        elif method == "session/new":
            session_id = str(uuid.uuid4())
            cwd = params.get("cwd", "")
            api_key = os.environ.get("GEMINI_API_KEY", "")
            
            agent = Agent(
                config=LocalAgentConfig(
                    api_key=api_key if api_key else None,
                    workspaces=[cwd] if cwd else None
                )
            )
            
            async def init_session(msg_id, session_id, agent):
                try:
                    await agent.__aenter__()
                    sessions[session_id] = {
                        "agent": agent,
                        "model": "gemini-2.5-pro"
                    }
                    send_response(msg_id, {
                        "sessionId": session_id,
                        "configOptions": [
                            {
                                "id": "model",
                                "name": "Model",
                                "type": "select",
                                "category": "model",
                                "currentValue": "gemini-2.5-pro",
                                "options": [
                                    {"value": "gemini-2.5-pro", "name": "Gemini 2.5 Pro"}
                                ]
                            }
                        ]
                    })
                except Exception as e:
                    logger.error(f"Error starting agent: {e}")
                    send_response(msg_id, {"error": {"code": -32000, "message": str(e)}})
            
            asyncio.create_task(init_session(msg_id, session_id, agent))
            
        elif method == "session/set_config_option":
            session_id = params.get("sessionId")
            config_id = params.get("configId")
            if session_id in sessions and config_id == "model":
                sessions[session_id]["model"] = str(params.get("value"))
            send_response(msg_id, {})
            
        elif method == "session/prompt":
            session_id = params.get("sessionId")
            prompt = params.get("prompt", [])
            asyncio.create_task(handle_prompt(msg_id, session_id, prompt))
            
        elif msg_id is not None:
            send_response(msg_id, {})

if __name__ == "__main__":
    asyncio.run(main())
