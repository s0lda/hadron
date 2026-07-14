# Antigravity (AGY) SDK Research

This document maps the Google Antigravity SDK surface as requested by the orchestrator.

## 1. SDK / HTTP API & Authentication
- **SDK**: The `google-antigravity` SDK is a **Python library** (`google-antigravity` PyPI package). There is no documented Rust SDK or native HTTP REST API for the Antigravity abstraction itself (it acts as an orchestration layer over the Gemini API). 
- **Authentication**: It authenticates natively via the `GEMINI_API_KEY` environment variable or a `.env` file. It can also be passed explicitly in code via `LocalAgentConfig(api_key="...")`. It uses the standard Gemini API key, so if `agy` already holds one, that same key will work.

## 2. Turn Shape (Resident vs. Stateless, Streaming)
- **Resident/Stateful**: Yes, the SDK maintains a resident, multi-turn session. It uses a `Conversation` object that maintains history, manages context compaction, and tracks turns across the interaction.
- **Streaming**: Yes, the SDK supports streaming natively. The `chat()` method returns an async iterator, allowing tokens to be streamed as they arrive (`async for token in response:`). It also supports streaming the model's intermediate reasoning via `async for thought in response.thoughts:`.

## 3. Token Usage Reporting
- **Usage Reporting**: Yes, the SDK tracks token usage. `agent.conversation.total_usage` returns a `UsageMetadata` object.
- **Components**: It breaks down tokens exactly into the components we need (though named slightly differently):
  - `prompt_token_count` (Input)
  - `candidates_token_count` (Output)
  - `cached_content_token_count` (Cache-read)
  - `thoughts_token_count` (Thinking / Reasoning)
  - `total_token_count` (Sum)
- **Cumulative vs. Per-turn**: The SDK exposes `total_usage` which is **cumulative** for the session. To get the per-turn token spend, the host would need to diff the total usage before and after the turn.

## 4. Built-in Tools
- **Tool Execution**: The SDK is **not** just a talker. It ships with a suite of built-in tools that can touch the repository.
- **Available Tools**: It natively supports:
  - `view_file`, `create_file`, `edit_file` (file edits)
  - `list_directory`, `search_directory`, `find_file` (filesystem exploration)
  - `run_command` (shell execution, though denied by default via `confirm_run_command()` policy, it can be enabled)
  - `start_subagent`, `ask_question`, `generate_image`

Because it handles tools natively, it is capable of full repository interaction, matching the capabilities of the CLI version.
