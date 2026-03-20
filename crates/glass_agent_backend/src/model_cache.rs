//! Model list caching — fetch from provider APIs and cache to disk.
//!
//! Queries `/v1/models` endpoints, filters to chat-capable models,
//! caches results as JSON files in `~/.glass/cache/models/` with 24h TTL.
