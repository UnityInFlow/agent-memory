//! Production [`Embedder`]: the Ollama `/api/embed` batch client.
//!
//! Uses the MODERN batch endpoint `POST /api/embed` with `{ model, input }`
//! (input is the batch array) — NEVER the legacy `/api/embeddings` singular
//! endpoint (deprecated shape). Health is `GET /api/tags`. The base URL comes
//! ONLY from the `--ollama-url` flag / `AGENT_MEMORY_OLLAMA_URL` env (default
//! `http://localhost:11434`), never from request payloads (threat T-02-02).
//! Every call carries a 10-second timeout (T-02-03).

use std::time::Duration;

use super::{BoxFuture, EmbedError, Embedder, EmbedderHealth, EMBEDDING_DIM};

/// The one supported embedding model. The `meta` table pins the same value so
/// dimension/model drift is detectable (RESEARCH Pitfall 6).
const MODEL: &str = "nomic-embed-text";

/// Per-request timeout: an unresponsive Ollama must degrade quickly, not hang
/// a search (T-02-03).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// HTTP client for a local Ollama daemon.
pub struct OllamaClient {
    http: reqwest::Client,
    base_url: String,
}

/// Request body for `POST /api/embed` (batch: `input` is an array).
#[derive(serde::Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

/// Response body for `POST /api/embed`.
#[derive(serde::Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

/// Response body for `GET /api/tags` (model presence probe).
#[derive(serde::Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(serde::Deserialize)]
struct TagModel {
    name: String,
}

impl OllamaClient {
    /// Build a client for the daemon at `base_url` (e.g. `http://localhost:11434`).
    pub fn new(base_url: impl Into<String>) -> Self {
        OllamaClient {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }
}

impl Embedder for OllamaClient {
    fn embed<'a>(
        &'a self,
        inputs: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, EmbedError>> {
        Box::pin(async move {
            let resp: EmbedResponse = self
                .http
                .post(format!("{}/api/embed", self.base_url))
                .json(&EmbedRequest {
                    model: MODEL,
                    input: inputs,
                })
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            // Guard the shape before anything reaches SQL: a short batch or a
            // wrong-dim vector must degrade, never insert (Pitfall 6).
            if resp.embeddings.len() != inputs.len() {
                return Err(EmbedError::InvalidResponse(format!(
                    "expected {} embeddings, got {}",
                    inputs.len(),
                    resp.embeddings.len()
                )));
            }
            for embedding in &resp.embeddings {
                if embedding.len() != EMBEDDING_DIM {
                    return Err(EmbedError::DimensionMismatch {
                        expected: EMBEDDING_DIM,
                        got: embedding.len(),
                    });
                }
            }
            Ok(resp.embeddings)
        })
    }

    fn health<'a>(&'a self) -> BoxFuture<'a, Result<EmbedderHealth, EmbedError>> {
        Box::pin(async move {
            let response = match self
                .http
                .get(format!("{}/api/tags", self.base_url))
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await
            {
                Ok(r) => r,
                // Connection-level failure (refused/DNS/timeout) → Unreachable.
                Err(e) => return Ok(EmbedderHealth::Unreachable(e.to_string())),
            };
            let response = match response.error_for_status() {
                Ok(r) => r,
                Err(e) => return Ok(EmbedderHealth::Unreachable(e.to_string())),
            };
            let tags: TagsResponse = response
                .json()
                .await
                .map_err(|e| EmbedError::InvalidResponse(e.to_string()))?;

            // Model names carry a tag suffix (`nomic-embed-text:latest`), so
            // match on the prefix.
            if tags.models.iter().any(|m| m.name.starts_with(MODEL)) {
                Ok(EmbedderHealth::Ready)
            } else {
                Ok(EmbedderHealth::ModelMissing)
            }
        })
    }
}
