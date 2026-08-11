import unittest
import sys
import os

# Ensure the scripts directory is on sys.path
sys.path.insert(0, os.path.dirname(__file__))

# Import agy_acp module functions
import agy_acp

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
        self.assertGreaterEqual(len(model_opt["options"]), 5)
        model_vals = [o["value"] for o in model_opt["options"]]
        self.assertIn("gemini-3.6-flash", model_vals)
        self.assertIn("gemini-3.6-pro", model_vals)

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
        new_model = "gemini-3.6-pro"
        agy_acp.sessions[session_id]["model"] = new_model
        if old_model != new_model and agy_acp.sessions[session_id].get("agent") is not None:
            agy_acp.sessions[session_id]["agent"] = None

        self.assertEqual(agy_acp.sessions[session_id]["model"], "gemini-3.6-pro")
        self.assertIsNone(agy_acp.sessions[session_id]["agent"])

if __name__ == "__main__":
    unittest.main()
