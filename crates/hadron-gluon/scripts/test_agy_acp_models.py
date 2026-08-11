import unittest
import sys
import os

# Ensure the scripts directory is on sys.path
sys.path.insert(0, os.path.dirname(__file__))

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
        new_model = "gemini-3.1-pro"
        agy_acp.sessions[session_id]["model"] = new_model
        if old_model != new_model and agy_acp.sessions[session_id].get("agent") is not None:
            agy_acp.sessions[session_id]["agent"] = None

        self.assertEqual(agy_acp.sessions[session_id]["model"], "gemini-3.1-pro")
        self.assertIsNone(agy_acp.sessions[session_id]["agent"])

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

if __name__ == "__main__":
    unittest.main()

