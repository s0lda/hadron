# SDK Dynamic Model Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable `agy_acp.py` to dynamically query available Gemini models from the Google GenAI SDK when an API key is available, falling back gracefully to static defaults when offline or unauthenticated.

**Architecture:** Add `fetch_sdk_models()` in `agy_acp.py` that utilizes `google.genai.Client(api_key=...).models.list()` to dynamically build the `configOptions` model list for `session/new` and model validation in `ensure_agent()`.

**Tech Stack:** Python 3.11, `google.genai`, `unittest`, `hadron-gluon` (ACP transport)

## Global Constraints

- Never break JSON-RPC stdout output formatting (`sys.stdout` must remain redirected).
- Maintain backward compatibility with `session_config_response(session_id)`.
- Fall back gracefully to `DEFAULT_SUPPORTED_MODELS` if `GEMINI_API_KEY` is not present or if network/API errors occur during model listing.

---

### Task 1: Add `fetch_sdk_models()` to `agy_acp.py`

**Files:**
- Modify: `crates/hadron-gluon/scripts/agy_acp.py`

**Interfaces:**
- Consumes: `os.environ.get("GEMINI_API_KEY")`, `google.genai.Client`
- Produces: `fetch_sdk_models() -> List[Dict[str, str]]`, `DEFAULT_SUPPORTED_MODELS`

- [x] **Step 1: Implement `fetch_sdk_models()` and update `session_config_response`** (commit 3c987994)

- [x] **Step 2: Update `session_config_response` and model validation in `ensure_agent`** (commit 3c987994)

---

### Task 2: Update `test_agy_acp_models.py` with Mocked Dynamic SDK Model Listing Tests

**Files:**
- Modify: `crates/hadron-gluon/scripts/test_agy_acp_models.py`

**Interfaces:**
- Consumes: `agy_acp.fetch_sdk_models()`, `unittest.mock`
- Produces: Dynamic model listing test coverage

- [x] **Step 1: Write dynamic SDK model listing unit test with unittest.mock** (commit 3c987994)

- [x] **Step 2: Run Python unit test suite** (commit 3c987994)

---

### Task 3: Workspace Test Suite & Plan Verification

**Files:**
- Modify: `.hadron/docs/plans/2026-08-11-sdk-dynamic-model-discovery.md`

- [x] **Step 1: Execute full workspace verification** (commit 3c987994)

- [x] **Step 2: Commit changes** (commit 3c987994)
