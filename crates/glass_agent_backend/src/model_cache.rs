//! Model list caching — fetch from provider APIs and cache to disk.
//!
//! Queries `/v1/models` endpoints, filters to chat-capable models,
//! caches results as JSON files in `~/.glass/cache/models/` with 24h TTL.

use serde::{Deserialize, Serialize};

// ── Types ─────────────────────────────────────────────────────────────────────

/// A model entry returned by a provider's `/v1/models` endpoint, normalized
/// and filtered to chat-capable models only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedModel {
    /// Provider-assigned model ID (e.g. `"gpt-4o"`).
    pub id: String,
    /// Human-readable display name (e.g. `"GPT-4o"`).
    pub display_name: String,
    /// Provider slug (e.g. `"openai-api"`).
    pub provider: String,
}

// ── Non-chat model filter ─────────────────────────────────────────────────────

/// Returns `true` when `id` belongs to a model that cannot be used for chat
/// completions and should therefore be excluded from the list.
fn is_non_chat_model(id: &str) -> bool {
    let lower = id.to_lowercase();
    lower.contains("embed")
        || lower.contains("tts")
        || lower.contains("whisper")
        || lower.contains("dall-e")
        || lower.contains("moderation")
}

// ── Friendly display name ─────────────────────────────────────────────────────

/// Map a raw model ID to a human-friendly display name.
///
/// Known mappings are checked first; unknown IDs are returned as-is.
pub(crate) fn friendly_model_name(id: &str) -> String {
    match id {
        "gpt-4o" => "GPT-4o".to_string(),
        "gpt-4o-mini" => "GPT-4o mini".to_string(),
        "o3" => "o3".to_string(),
        "o3-mini" => "o3 mini".to_string(),
        other => {
            let lower = other.to_lowercase();
            if lower.contains("opus") {
                "Claude Opus".to_string()
            } else if lower.contains("sonnet") {
                "Claude Sonnet".to_string()
            } else if lower.contains("haiku") {
                "Claude Haiku".to_string()
            } else {
                other.to_string()
            }
        }
    }
}

// ── Response parser ───────────────────────────────────────────────────────────

/// Parse a raw JSON body from a `/v1/models` response into a filtered,
/// normalized list of [`CachedModel`]s.
///
/// Exposed as a standalone function so it can be unit-tested without HTTP.
pub(crate) fn parse_model_list(provider: &str, body: &str) -> Vec<CachedModel> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        tracing::warn!("model_cache: failed to parse /v1/models response JSON");
        return Vec::new();
    };

    let Some(data) = v.get("data").and_then(|d| d.as_array()) else {
        tracing::warn!("model_cache: /v1/models response missing 'data' array");
        return Vec::new();
    };

    data.iter()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(|i| i.as_str())?;
            if is_non_chat_model(id) {
                return None;
            }
            Some(CachedModel {
                id: id.to_string(),
                display_name: friendly_model_name(id),
                provider: provider.to_string(),
            })
        })
        .collect()
}

// ── Cache helpers ─────────────────────────────────────────────────────────────

/// Return the path to the on-disk cache file for `provider`.
///
/// The directory `~/.glass/cache/models/` is created on first use.
fn cache_file_path(provider: &str) -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    let dir = home.join(".glass").join("cache").join("models");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{}.json", provider)))
}

/// Read the cache file and return its contents if it is younger than 24 hours.
fn read_fresh_cache(path: &std::path::Path) -> Option<Vec<CachedModel>> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .ok()?;

    if age.as_secs() >= 24 * 3600 {
        return None; // Cache is stale.
    }

    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Read the cache file regardless of age (used as fallback on fetch failure).
fn read_stale_cache(path: &std::path::Path) -> Vec<CachedModel> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist `models` to `path` as JSON, logging but not propagating errors.
fn write_cache(path: &std::path::Path, models: &[CachedModel]) {
    match serde_json::to_string(models) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                tracing::warn!("model_cache: failed to write cache file {}: {}", path.display(), e);
            }
        }
        Err(e) => {
            tracing::warn!("model_cache: failed to serialize model list: {}", e);
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Fetch the list of chat-capable models for `provider`.
///
/// 1. Returns cached data if `~/.glass/cache/models/{provider}.json` is less
///    than 24 hours old.
/// 2. Otherwise, queries `{endpoint}/v1/models` with Bearer auth, filters to
///    chat-capable models, writes the result to the cache file, and returns it.
/// 3. On any network or parse failure, falls back to whatever is in the stale
///    cache (or an empty vec if no cache exists).
pub fn fetch_models(provider: &str, endpoint: &str, api_key: &str) -> Vec<CachedModel> {
    let cache_path = match cache_file_path(provider) {
        Some(p) => p,
        None => {
            tracing::warn!("model_cache: could not determine cache directory");
            return Vec::new();
        }
    };

    // Return fresh cache if available.
    if let Some(cached) = read_fresh_cache(&cache_path) {
        tracing::debug!(
            "model_cache: returning cached models for provider '{}' ({} models)",
            provider,
            cached.len()
        );
        return cached;
    }

    // Fetch from the API.
    let url = format!("{}/v1/models", endpoint.trim_end_matches('/'));
    tracing::debug!("model_cache: fetching {}", url);

    let response = ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", api_key))
        .call();

    match response {
        Ok(mut resp) => {
            match resp.body_mut().read_to_string() {
                Ok(body) => {
                    let models = parse_model_list(provider, &body);
                    write_cache(&cache_path, &models);
                    tracing::info!(
                        "model_cache: fetched {} models for provider '{}'",
                        models.len(),
                        provider
                    );
                    models
                }
                Err(e) => {
                    tracing::warn!("model_cache: failed to read response body: {}", e);
                    read_stale_cache(&cache_path)
                }
            }
        }
        Err(e) => {
            tracing::warn!("model_cache: HTTP request to {} failed: {}", url, e);
            read_stale_cache(&cache_path)
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_name_known_models() {
        assert_eq!(friendly_model_name("gpt-4o"), "GPT-4o");
        assert_eq!(friendly_model_name("gpt-4o-mini"), "GPT-4o mini");
        assert_eq!(friendly_model_name("o3"), "o3");
        assert_eq!(friendly_model_name("o3-mini"), "o3 mini");
        assert_eq!(
            friendly_model_name("claude-3-opus-20240229"),
            "Claude Opus"
        );
        assert_eq!(
            friendly_model_name("claude-3-5-sonnet-20241022"),
            "Claude Sonnet"
        );
        assert_eq!(
            friendly_model_name("claude-3-haiku-20240307"),
            "Claude Haiku"
        );
    }

    #[test]
    fn friendly_name_unknown_returns_id() {
        assert_eq!(friendly_model_name("custom-v1"), "custom-v1");
        assert_eq!(
            friendly_model_name("my-local-model-7b"),
            "my-local-model-7b"
        );
    }

    #[test]
    fn parse_model_list_filters_embeddings() {
        let body = r#"{
            "data": [
                {"id": "gpt-4o"},
                {"id": "text-embedding-3-large"},
                {"id": "tts-1"},
                {"id": "whisper-1"},
                {"id": "dall-e-3"},
                {"id": "omni-moderation-latest"},
                {"id": "gpt-4o-mini"},
                {"id": "o3"}
            ]
        }"#;

        let models = parse_model_list("openai-api", body);

        // Only chat-capable models should be returned.
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"gpt-4o"), "gpt-4o should be included");
        assert!(ids.contains(&"gpt-4o-mini"), "gpt-4o-mini should be included");
        assert!(ids.contains(&"o3"), "o3 should be included");

        // Non-chat models must be excluded.
        assert!(
            !ids.contains(&"text-embedding-3-large"),
            "embedding model should be excluded"
        );
        assert!(!ids.contains(&"tts-1"), "TTS model should be excluded");
        assert!(!ids.contains(&"whisper-1"), "Whisper should be excluded");
        assert!(!ids.contains(&"dall-e-3"), "DALL-E should be excluded");
        assert!(
            !ids.contains(&"omni-moderation-latest"),
            "moderation model should be excluded"
        );

        assert_eq!(models.len(), 3);

        // Verify provider and display names are set correctly.
        let gpt4o = models.iter().find(|m| m.id == "gpt-4o").unwrap();
        assert_eq!(gpt4o.display_name, "GPT-4o");
        assert_eq!(gpt4o.provider, "openai-api");
    }

    #[test]
    fn parse_model_list_empty_data() {
        let body = r#"{"data": []}"#;
        let models = parse_model_list("openai-api", body);
        assert!(models.is_empty());
    }

    #[test]
    fn parse_model_list_invalid_json() {
        let models = parse_model_list("openai-api", "not-json");
        assert!(models.is_empty());
    }

    #[test]
    fn parse_model_list_missing_data_field() {
        let body = r#"{"object": "list"}"#;
        let models = parse_model_list("openai-api", body);
        assert!(models.is_empty());
    }
}
