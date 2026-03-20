//! OpenAI-compatible API backend.
//!
//! Implements [`AgentBackend`](crate::AgentBackend) for any endpoint that speaks
//! the OpenAI `/v1/chat/completions` API with SSE streaming. Covers OpenAI,
//! Google Gemini (OpenAI-compat mode), and local servers (vLLM, llama.cpp, LM Studio).
