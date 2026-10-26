import unittest
import sys
import os

# Ensure the scripts directory is on sys.path
sys.path.insert(0, os.path.dirname(__file__))

try:
    import google.antigravity  # noqa: F401
except ModuleNotFoundError:
    import types
    for mod_name in ("google", "google.antigravity", "google.antigravity.hooks", "google.antigravity.types", "google.genai"):
        mod = types.ModuleType(mod_name)
        mod.__path__ = []
        sys.modules[mod_name] = mod
    sys.modules["google"].antigravity = sys.modules["google.antigravity"]
    sys.modules["google"].genai = sys.modules["google.genai"]
    sys.modules["google.antigravity"].Agent = object
    sys.modules["google.antigravity"].LocalAgentConfig = object
    sys.modules["google.antigravity.hooks"].policy = object()
    sys.modules["google.genai"].Client = object

    class _Chunk:
        def __init__(self, **kw):
            self.__dict__.update(kw)

    sys.modules["google.antigravity.types"].Text = type("Text", (_Chunk,), {})
    sys.modules["google.antigravity.types"].Thought = type("Thought", (_Chunk,), {})
    sys.modules["google.antigravity.types"].ToolCall = type("ToolCall", (_Chunk,), {})
    sys.modules["google.antigravity.types"].ToolResult = type("ToolResult", (_Chunk,), {})

# Import agy_acp module functions
import agy_acp

from unittest.mock import patch, MagicMock

class TestAgyAcpModels(unittest.TestCase):
    def test_session_config_response_lists_multiple_models(self):
        self.assertTrue(hasattr(agy_acp, "SUPPORTED_MODELS"), "agy_acp missing SUPPORTED_MODELS")
        resp = agy_acp.session_config_response("test-session-123")
        self.assertEqual(resp["sessionId"], "test-session-123")
        opts = resp["configOptions"]
        self.assertEqual(len(opts), 1)
        model_opt = opts[0]
        self.assertEqual(model_opt["id"], "model")
        self.assertEqual(model_opt["category"], "model")
        self.assertGreaterEqual(len(model_opt["options"]), 3)
        model_vals = [o["value"] for o in model_opt["options"]]
        self.assertIn("gemini-3.8-flash", model_vals)
        self.assertIn("gemini-3.6-flash", model_vals)
        self.assertIn("gemini-3.1-pro", model_vals)

    def test_session_set_config_option_updates_model(self):
        # Register a test session
        session_id = "test-session-456"
        agy_acp.sessions[session_id] = {
            "agent": "mock_agent_instance",
            "model": "gemini-3.6-flash",
            "cwd": "",
        }
        # Simulate updating config option for model
        old_model = agy_acp.sessions[session_id].get("model")
        new_model = "gemini-3.8-flash"
        agy_acp.sessions[session_id]["model"] = new_model
        if old_model != new_model and agy_acp.sessions[session_id].get("agent") is not None:
            agy_acp.sessions[session_id]["agent"] = None

        self.assertEqual(agy_acp.sessions[session_id]["model"], "gemini-3.8-flash")
        self.assertIsNone(agy_acp.sessions[session_id]["agent"])
        # Verify session_config_options returns configOptions matching ACP schema
        opts = agy_acp.session_config_options(session_id)
        self.assertEqual(len(opts), 1)
        self.assertEqual(opts[0]["id"], "model")
        self.assertEqual(opts[0]["currentValue"], "gemini-3.8-flash")

    def test_fetch_sdk_models_fallback_when_no_api_key(self):
        with patch.dict(os.environ, {}, clear=True):
            models = agy_acp.fetch_sdk_models()
            self.assertEqual(models, agy_acp.DEFAULT_SUPPORTED_MODELS)

    @patch("google.genai.Client")
    def test_fetch_sdk_models_dynamic_listing(self, mock_client_cls):
        mock_model1 = MagicMock()
        mock_model1.name = "models/gemini-2.5-flash"
        mock_model1.display_name = "Gemini 2.5 Flash"
        
        mock_model2 = MagicMock()
        mock_model2.name = "models/gemini-3.6-pro"
        mock_model2.display_name = "Gemini 3.6 Pro"

        mock_client = MagicMock()
        mock_client.models.list.return_value = [mock_model1, mock_model2]
        mock_client_cls.return_value = mock_client

        with patch.dict(os.environ, {"GEMINI_API_KEY": "test-key"}):
            models = agy_acp.fetch_sdk_models()
            self.assertEqual(len(models), 2)
            self.assertEqual(models[0], {"value": "gemini-2.5-flash", "name": "Gemini 2.5 Flash"})
            self.assertEqual(models[1], {"value": "gemini-3.6-pro", "name": "Gemini 3.6 Pro"})

    @patch("google.genai.Client")
    def test_fetch_sdk_models_error_fallback(self, mock_client_cls):
        mock_client_cls.side_effect = Exception("API connection error")
        with patch.dict(os.environ, {"GEMINI_API_KEY": "test-key"}):
            models = agy_acp.fetch_sdk_models()
            self.assertEqual(models, agy_acp.DEFAULT_SUPPORTED_MODELS)

    def test_handle_prompt_cancelled(self):
        import asyncio
        session_id = "test-session-cancel"
        mock_agent = MagicMock()
        
        async def hang_chat(prompt):
            await asyncio.sleep(10)
        mock_agent.chat.side_effect = hang_chat

        agy_acp.sessions[session_id] = {
            "agent": mock_agent,
            "model": "gemini-3.6-flash",
            "cwd": "",
            "current_task": None,
        }

        responses = []
        with patch("agy_acp.send_response", side_effect=lambda mid, res: responses.append((mid, res))):
            async def run():
                prompt_task = asyncio.create_task(
                    agy_acp.handle_prompt(42, session_id, [{"type": "text", "text": "hello"}])
                )
                agy_acp.sessions[session_id]["current_task"] = prompt_task
                await asyncio.sleep(0.01)
                # Cancel the task
                prompt_task.cancel()
                try:
                    await prompt_task
                except asyncio.CancelledError:
                    pass

            asyncio.run(run())

        self.assertEqual(len(responses), 1)
        msg_id, res = responses[0]
        self.assertEqual(msg_id, 42)
        self.assertEqual(res, {"stopReason": "cancelled"})
        self.assertIsNone(agy_acp.sessions[session_id]["current_task"])

    def test_handle_prompt_divides_multistep_context_usage_by_invocations(self):
        import asyncio

        session_id = "test-session-multistep-usage"
        mock_agent = MagicMock()

        class FakeResponse:
            @property
            async def chunks(self):
                yield agy_acp.ToolCall(name="view_file", args={})
                yield agy_acp.ToolCall(name="run_command", args={})
                yield agy_acp.Text(text="done")

            async def text(self):
                return "done"

            @property
            def usage_metadata(self):
                u = MagicMock()
                # 3 invocations (2 tool calls + 1 final) totaling 600k prompt tokens (below max_tokens // 2 = 1M)
                u.prompt_token_count = 600_000
                u.cached_content_token_count = 540_000
                u.candidates_token_count = 1_500
                u.thoughts_token_count = 500
                u.total_token_count = 602_000
                u.context_window_size = 2_000_000
                return u

        async def fake_chat(prompt):
            return FakeResponse()

        mock_agent.chat.side_effect = fake_chat
        agy_acp.sessions[session_id] = {
            "agent": mock_agent,
            "model": "gemini-3.6-flash",
            "cwd": "",
            "current_task": None,
        }

        updates = []
        responses = []
        with patch("agy_acp.send_session_update", side_effect=lambda sid, upd: updates.append((sid, upd))), \
             patch("agy_acp.send_response", side_effect=lambda mid, res: responses.append((mid, res))):
            asyncio.run(agy_acp.handle_prompt(99, session_id, [{"type": "text", "text": "go"}]))

        usage_updates = [upd for _, upd in updates if upd.get("sessionUpdate") == "usage_update"]
        self.assertEqual(len(usage_updates), 1)
        self.assertEqual(usage_updates[0]["used"], 200_000)
        self.assertEqual(usage_updates[0]["size"], 2_000_000)

if __name__ == "__main__":
    unittest.main()

