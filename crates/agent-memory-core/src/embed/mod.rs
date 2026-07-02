//! The embedding seam: [`Embedder`] abstracts vector generation behind a
//! dyn-compatible trait so every Ollama-outage condition becomes a typed
//! [`EmbedError`] the service can catch.
//!
//! Invariant guarded here (SEARCH-03): **store and search never fail because
//! Ollama is down.** [`crate::service::MemoryService`] holds an
//! `Arc<dyn Embedder>` (mirroring `Arc<dyn Clock>`); on embed failure it inserts
//! with `embedding_status = 0` (store) or routes to the FTS5 keyword path
//! (search). Production injects [`ollama::OllamaClient`]; tests inject
//! [`FakeEmbedder`].
//!
//! Dyn-compatibility note: native async-fn-in-trait is NOT dyn-safe, so the
//! trait methods return [`BoxFuture`]s; implementations use
//! `Box::pin(async move { ... })`.

pub mod ollama;

/// Embedding dimension for `nomic-embed-text` (matches `vec_memories FLOAT[768]`
/// and the `meta` table's `embedding_dim` row — RESEARCH Pitfall 6).
pub const EMBEDDING_DIM: usize = 768;

/// A boxed, `Send` future — the return shape of the dyn-compatible [`Embedder`]
/// methods (`Arc<dyn Embedder>` requires object safety, which native async fn
/// in traits does not provide).
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Typed embedding errors, following the [`crate::domain::MemoryError`]
/// thiserror conventions. Any of these at the service seam triggers graceful
/// degradation — never a caller-visible failure (SEARCH-03).
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// The HTTP call to Ollama failed (connect/timeout/status).
    #[error("embedding HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// A returned vector did not have [`EMBEDDING_DIM`] dimensions. A
    /// wrong-dim vector must degrade, never insert (RESEARCH Pitfall 6).
    #[error("embedding dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },

    /// The embedding model is not pulled/available on the Ollama daemon.
    #[error("embedding model missing: {0}")]
    ModelMissing(String),

    /// The response body did not have the expected shape.
    #[error("invalid embedding response: {0}")]
    InvalidResponse(String),
}

/// Health of the embedding backend, probed via `GET /api/tags`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedderHealth {
    /// Daemon reachable and the model is pulled.
    Ready,
    /// Daemon reachable but `nomic-embed-text` is not pulled.
    ModelMissing,
    /// Daemon unreachable (connection refused, DNS, timeout, …).
    Unreachable(String),
}

/// The degrade seam (SEARCH-03 lives at this boundary).
///
/// `Send + Sync` and **dyn-compatible**: methods return [`BoxFuture`]s so the
/// service can hold `Arc<dyn Embedder>` exactly like `Arc<dyn Clock>`.
pub trait Embedder: Send + Sync {
    /// Embed a batch of inputs, returning one [`EMBEDDING_DIM`]-dim vector per
    /// input in order. Implementations must guard the response count and
    /// per-vector dimension.
    fn embed<'a>(
        &'a self,
        inputs: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, EmbedError>>;

    /// Probe backend availability (daemon reachable? model pulled?). Used for
    /// the one-line startup visibility log — never blocks serving.
    fn health<'a>(&'a self) -> BoxFuture<'a, Result<EmbedderHealth, EmbedError>>;
}

/// Deterministic test double, mirroring the `TestClock` gating mechanism: the
/// `test-clock` feature gates test doubles generally (the core crate's self
/// dev-dependency enables it, so integration tests need no `--features` flag
/// and production consumers never see this type).
///
/// Behavior:
/// - a programmed input returns its programmed vector;
/// - an unknown input returns a deterministic hash-seeded unit vector, so any
///   text embeds reproducibly without programming;
/// - with `fail` set, `embed` returns [`EmbedError::InvalidResponse`] and
///   `health` reports [`EmbedderHealth::Unreachable`].
#[cfg(any(test, feature = "test-clock"))]
#[derive(Debug, Default)]
pub struct FakeEmbedder {
    vectors: std::collections::HashMap<String, Vec<f32>>,
    fail: bool,
}

#[cfg(any(test, feature = "test-clock"))]
impl FakeEmbedder {
    /// A succeeding embedder with explicit input→vector programming.
    pub fn with_vectors(vectors: std::collections::HashMap<String, Vec<f32>>) -> Self {
        FakeEmbedder {
            vectors,
            fail: false,
        }
    }

    /// An embedder whose every call fails — the SEARCH-03 kill switch.
    pub fn failing() -> Self {
        FakeEmbedder {
            vectors: std::collections::HashMap::new(),
            fail: true,
        }
    }

    /// Deterministic hash-seeded unit vector for an unprogrammed input.
    fn seeded_unit_vector(input: &str) -> Vec<f32> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        input.hash(&mut hasher);
        let mut state = hasher.finish();
        if state == 0 {
            state = 0x9E37_79B9_7F4A_7C15; // never let xorshift get stuck at 0
        }
        let mut out = Vec::with_capacity(EMBEDDING_DIM);
        for _ in 0..EMBEDDING_DIM {
            // xorshift64: a deterministic pseudo-random stream from the seed.
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            out.push(((state as f64 / u64::MAX as f64) * 2.0 - 1.0) as f32);
        }
        let norm = out
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>()
            .sqrt();
        if norm > 0.0 {
            for v in &mut out {
                *v = (f64::from(*v) / norm) as f32;
            }
        }
        out
    }
}

#[cfg(any(test, feature = "test-clock"))]
impl Embedder for FakeEmbedder {
    fn embed<'a>(
        &'a self,
        inputs: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, EmbedError>> {
        Box::pin(async move {
            if self.fail {
                return Err(EmbedError::InvalidResponse(
                    "FakeEmbedder configured to fail".to_string(),
                ));
            }
            Ok(inputs
                .iter()
                .map(|input| {
                    self.vectors
                        .get(input)
                        .cloned()
                        .unwrap_or_else(|| Self::seeded_unit_vector(input))
                })
                .collect())
        })
    }

    fn health<'a>(&'a self) -> BoxFuture<'a, Result<EmbedderHealth, EmbedError>> {
        Box::pin(async move {
            if self.fail {
                Ok(EmbedderHealth::Unreachable(
                    "FakeEmbedder configured to fail".to_string(),
                ))
            } else {
                Ok(EmbedderHealth::Ready)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn programmed_input_returns_programmed_vector() {
        let mut vectors = std::collections::HashMap::new();
        let mut v = vec![0.0_f32; EMBEDDING_DIM];
        v[0] = 1.0;
        vectors.insert("known".to_string(), v.clone());
        let fake = FakeEmbedder::with_vectors(vectors);
        let out = fake
            .embed(&["known".to_string()])
            .await
            .expect("embed succeeds");
        assert_eq!(out, vec![v]);
    }

    #[tokio::test]
    async fn unknown_input_gets_deterministic_unit_vector_of_right_dim() {
        let fake = FakeEmbedder::with_vectors(std::collections::HashMap::new());
        let a = fake
            .embed(&["some unseen text".to_string()])
            .await
            .expect("embed succeeds");
        let b = fake
            .embed(&["some unseen text".to_string()])
            .await
            .expect("embed succeeds");
        assert_eq!(a, b, "same input must embed identically");
        assert_eq!(a[0].len(), EMBEDDING_DIM);
        let norm: f64 = a[0]
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "seeded vector is unit-norm, got {norm}"
        );
    }

    #[tokio::test]
    async fn distinct_inputs_get_distinct_vectors() {
        let fake = FakeEmbedder::with_vectors(std::collections::HashMap::new());
        let out = fake
            .embed(&["alpha".to_string(), "beta".to_string()])
            .await
            .expect("embed succeeds");
        assert_eq!(out.len(), 2, "one vector per input");
        assert_ne!(out[0], out[1], "different inputs must embed differently");
    }

    #[tokio::test]
    async fn failing_embedder_errors_on_embed_and_reports_unreachable() {
        let fake = FakeEmbedder::failing();
        let err = fake
            .embed(&["anything".to_string()])
            .await
            .expect_err("failing embedder must error");
        assert!(matches!(err, EmbedError::InvalidResponse(_)));
        let health = fake.health().await.expect("health call itself succeeds");
        assert!(matches!(health, EmbedderHealth::Unreachable(_)));
    }
}
