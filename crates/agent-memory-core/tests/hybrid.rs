//! SEARCH-04/SEARCH-05: decay-aware hybrid search.
//!
//! With the embedder up, `MemoryService::search` runs two pure-relevance legs
//! (the distance-ordered KNN leg and the bm25-ordered `keyword_candidates`
//! leg), fuses them with Reciprocal Rank Fusion (k = 60, 1-based ranks,
//! duplicate ids sum their contributions), then multiplies each fused score by
//! the pinned-aware decay score exactly once (post-fusion, SEARCH-05). The
//! envelope reports `search_mode: "hybrid"`.
//!
//! RRF only looks at ranks, never raw distances or bm25 values, so every
//! fixture here controls ranks directly: query vector = axis 0, each row's
//! vector has a chosen cosine similarity to it, and bm25 order is set by term
//! frequency and document length. Real on-disk temp DBs and an injected
//! `TestClock`, as in tests/semantic.rs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_memory_core::clock::{Clock, TestClock};
use agent_memory_core::decay::{decay_score, DecayConfig, DEFAULT_HALF_LIFE_SECS};
use agent_memory_core::domain::{MemoryType, NewMemory};
use agent_memory_core::embed::{Embedder, FakeEmbedder, EMBEDDING_DIM};
use agent_memory_core::service::{rrf_fuse, MemoryService, SearchArgs, SearchMode, RRF_K};
use agent_memory_core::store::sqlite::SqliteStore;
use agent_memory_core::store::Store;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct Harness {
    _dir: tempfile::TempDir,
    db_path: PathBuf,
    clock: Arc<TestClock>,
    store: Arc<SqliteStore>,
    service: MemoryService,
}

fn harness(embedder: Arc<dyn Embedder>) -> Harness {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.db");
    let clock = Arc::new(TestClock::new(1_000));
    let store = Arc::new(SqliteStore::open(&db_path).expect("open store"));
    let service = MemoryService::new(
        store.clone(),
        clock.clone() as Arc<dyn Clock>,
        embedder,
        DecayConfig::default(),
    );
    Harness {
        _dir: dir,
        db_path,
        clock,
        store,
        service,
    }
}

/// A succeeding embedder programmed with `vectors` (anything else embeds to a
/// deterministic hash-seeded unit vector, ~orthogonal to every axis).
fn live(vectors: HashMap<String, Vec<f32>>) -> Arc<dyn Embedder> {
    Arc::new(FakeEmbedder::with_vectors(vectors))
}

fn new_memory(content: &str, mem_type: MemoryType) -> NewMemory {
    NewMemory {
        content: content.to_string(),
        mem_type,
        tags: vec![],
        source: None,
        scope: None,
        ttl_secs: None,
    }
}

async fn store(h: &Harness, content: &str, mem_type: MemoryType) -> i64 {
    h.service
        .store(new_memory(content, mem_type))
        .await
        .expect("store")
}

fn query(text: &str, limit: Option<i64>) -> SearchArgs {
    SearchArgs {
        query: text.to_string(),
        limit,
        ..Default::default()
    }
}

/// A 768-dim unit vector along `axis`.
fn unit_axis(axis: usize) -> Vec<f32> {
    let mut v = vec![0.0_f32; EMBEDDING_DIM];
    v[axis] = 1.0;
    v
}

/// A unit vector whose cosine similarity to `unit_axis(0)` is exactly `sim`,
/// with the remainder on its own private `axis` so rows never coincide.
fn sim_vec(sim: f32, axis: usize) -> Vec<f32> {
    assert!(axis > 0, "axis 0 is the query axis");
    let mut v = vec![0.0_f32; EMBEDDING_DIM];
    v[0] = sim;
    v[axis] = (1.0 - sim * sim).sqrt();
    v
}

fn ids(results: &[agent_memory_core::domain::MemoryView]) -> Vec<i64> {
    results.iter().map(|v| v.id).collect()
}

fn raw_conn(db_path: &Path) -> rusqlite::Connection {
    agent_memory_core::store::sqlite::register_vec_extension().expect("register vec0");
    rusqlite::Connection::open(db_path).expect("open db for assertion")
}

const EPS: f64 = 1e-12;

// ---------------------------------------------------------------------------
// Test 1: rrf_fuse against hand-computed values
// ---------------------------------------------------------------------------

#[test]
fn rrf_k_is_the_cormack_clarke_constant() {
    assert_eq!(RRF_K, 60.0);
}

#[test]
fn rrf_dual_leg_rank_two_beats_single_leg_rank_one() {
    // 20 is rank 2 in BOTH legs: 1/62 + 1/62 = 2/62. 10 and 30 are rank 1 in
    // one leg each: 1/61. 2/62 ≈ 0.03226 > 1/61 ≈ 0.01639.
    let fused = rrf_fuse(&[vec![10, 20], vec![30, 20]]);
    assert_eq!(fused.len(), 3, "three distinct ids: {fused:?}");

    assert_eq!(fused[0].0, 20, "the dual-leg id wins: {fused:?}");
    assert!((fused[0].1 - 2.0 / 62.0).abs() < EPS, "{fused:?}");

    // 10 and 30 tie at exactly 1/61: the ascending-id tie-break decides.
    assert_eq!((fused[1].0, fused[2].0), (10, 30), "{fused:?}");
    assert!((fused[1].1 - 1.0 / 61.0).abs() < EPS, "{fused:?}");
    assert!((fused[2].1 - 1.0 / 61.0).abs() < EPS, "{fused:?}");
    assert!(fused[0].1 > fused[1].1, "2/62 strictly beats 1/61");
}

#[test]
fn rrf_duplicate_ids_sum_their_contributions() {
    // 7: rank 1 in leg A (1/61) + rank 3 in leg B (1/63).
    // 8: rank 2 in leg A (1/62) only. 9: rank 1 in leg B (1/61) only.
    let fused = rrf_fuse(&[vec![7, 8], vec![9, 5, 7]]);
    let score = |id: i64| {
        fused
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, s)| *s)
            .unwrap_or_else(|| panic!("id {id} missing from {fused:?}"))
    };
    assert!((score(7) - (1.0 / 61.0 + 1.0 / 63.0)).abs() < EPS);
    assert!((score(8) - 1.0 / 62.0).abs() < EPS);
    assert!((score(9) - 1.0 / 61.0).abs() < EPS);
    assert!((score(5) - 1.0 / 62.0).abs() < EPS);
    // 7 first; 9 (1/61); then the 1/62 tie between 5 and 8 broken by id.
    assert_eq!(
        fused.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        vec![7, 9, 5, 8]
    );
}

#[test]
fn rrf_ties_break_deterministically_by_ascending_id() {
    for _ in 0..20 {
        // HashMap iteration order varies run to run; the output must not.
        let fused = rrf_fuse(&[vec![42, 3], vec![17, 99]]);
        assert_eq!(
            fused.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![17, 42, 3, 99],
            "rank-1 ids tie (17 < 42), then rank-2 ids tie (3 < 99)"
        );
    }
}

#[test]
fn rrf_of_empty_legs_is_empty() {
    assert!(rrf_fuse(&[vec![], vec![]]).is_empty());
    assert!(rrf_fuse(&[]).is_empty());
}

// ---------------------------------------------------------------------------
// Test 2: end-to-end fusion surfaces both a semantic-only and a keyword-only hit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn hybrid_surfaces_the_top_semantic_and_the_top_keyword_hit() {
    // X: nearest vector, but does NOT contain the keyword.
    // Y: the only keyword hit, but its vector is orthogonal to the query.
    const X: &str = "gentle western wind patterns along the coast";
    const Y: &str = "zephyr rollout notes for the zephyr cluster";
    const Q: &str = "zephyr";
    let mut vectors = HashMap::new();
    vectors.insert(Q.to_string(), unit_axis(0));
    vectors.insert(X.to_string(), sim_vec(0.99, 1));
    vectors.insert(Y.to_string(), unit_axis(2));
    let h = harness(live(vectors));

    let x = store(&h, X, MemoryType::Pattern).await;
    let y = store(&h, Y, MemoryType::Pattern).await;
    for filler in [
        "weekly standup happens every monday",
        "the frontend bundle uses tailwind",
        "invoices are exported nightly",
    ] {
        store(&h, filler, MemoryType::Pattern).await;
    }

    let outcome = h.service.search(query(Q, None)).await.expect("search");
    assert_eq!(outcome.search_mode, SearchMode::Hybrid);

    // Y: keyword rank 1 (1/61) + some KNN rank r >= 2 (1/(60+r)) > 1/61.
    // X: KNN rank 1 only (1/61). Every filler: one KNN contribution < 1/61.
    let got = ids(&outcome.results);
    assert_eq!(
        &got[..2],
        &[y, x],
        "both the keyword-only and the semantic-only hit surface, fused"
    );
    assert_eq!(got.len(), 5, "every KNN candidate is still a fused result");
}

#[tokio::test]
async fn hybrid_wire_label_is_lowercase_hybrid() {
    let h = harness(live(HashMap::new()));
    store(&h, "anything at all", MemoryType::Pattern).await;
    let outcome = h
        .service
        .search(query("anything", None))
        .await
        .expect("search");
    let json = serde_json::to_value(&outcome).expect("serialize");
    assert_eq!(json["search_mode"], "hybrid");
}

// ---------------------------------------------------------------------------
// Test 3: both legs are oversampled (limit * 4) before fusion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn semantic_leg_is_oversampled_beyond_limit() {
    // limit = 2. K holds the only keyword hit and sits at KNN rank 5 — beyond
    // the limit, inside limit * 4. Fused: K = 1/61 + 1/65 beats A1 = 1/61.
    // If the KNN leg fetched only `limit` rows, K would score 1/61, tie A1,
    // and lose the ascending-id tie-break (K is stored after A1).
    const Q: &str = "quasar";
    const K: &str = "quasar calibration notes";
    let near = [
        ("alpha survey of the northern sky", 0.99_f32),
        ("beta survey of the southern sky", 0.98),
        ("gamma survey of the eastern sky", 0.97),
        ("delta survey of the western sky", 0.96),
    ];
    let mut vectors = HashMap::new();
    vectors.insert(Q.to_string(), unit_axis(0));
    for (i, (text, sim)) in near.iter().enumerate() {
        vectors.insert(text.to_string(), sim_vec(*sim, i + 1));
    }
    vectors.insert(K.to_string(), sim_vec(0.95, 10));
    let h = harness(live(vectors));

    let mut near_ids = Vec::new();
    for (text, _) in near {
        near_ids.push(store(&h, text, MemoryType::Pattern).await);
    }
    let k = store(&h, K, MemoryType::Pattern).await;

    let outcome = h.service.search(query(Q, Some(2))).await.expect("search");
    assert_eq!(outcome.search_mode, SearchMode::Hybrid);
    assert_eq!(
        ids(&outcome.results),
        vec![k, near_ids[0]],
        "the keyword hit at KNN rank 5 must enter fusion and win"
    );
}

#[tokio::test]
async fn keyword_leg_is_oversampled_beyond_limit() {
    // limit = 2 (both legs fetch 8). M is KNN rank 1 and keyword rank 3 —
    // beyond the limit. W1/W2 are keyword ranks 1/2 with far-away vectors,
    // pushed out of the KNN top 8 by seven closer fillers.
    // Fused: M = 1/61 + 1/63 beats W1 = 1/61. If the keyword leg fetched only
    // `limit` rows, M would score 1/61, tie W1 and lose the id tie-break.
    const Q: &str = "quasar";
    const W1: &str = "quasar quasar quasar";
    const W2: &str = "quasar quasar quasar beacon";
    const M: &str = "quasar telescope calibration notes for the southern observatory array";
    let mut vectors = HashMap::new();
    vectors.insert(Q.to_string(), unit_axis(0));
    vectors.insert(W1.to_string(), unit_axis(1));
    vectors.insert(W2.to_string(), unit_axis(2));
    vectors.insert(M.to_string(), sim_vec(0.99, 3));
    let fillers: Vec<String> = (0..7)
        .map(|i| format!("filler record number {i}"))
        .collect();
    for (i, f) in fillers.iter().enumerate() {
        vectors.insert(f.clone(), sim_vec(0.9 - 0.05 * i as f32, 20 + i));
    }
    let h = harness(live(vectors));

    let w1 = store(&h, W1, MemoryType::Pattern).await;
    store(&h, W2, MemoryType::Pattern).await;
    let m = store(&h, M, MemoryType::Pattern).await;
    for f in &fillers {
        store(&h, f, MemoryType::Pattern).await;
    }

    let outcome = h.service.search(query(Q, Some(2))).await.expect("search");
    assert_eq!(outcome.search_mode, SearchMode::Hybrid);
    assert_eq!(
        ids(&outcome.results),
        vec![m, w1],
        "the keyword-rank-3 hit must enter fusion and win"
    );
}

// ---------------------------------------------------------------------------
// Test 4 (SEARCH-05 golden): decay is applied exactly once, post-fusion
// ---------------------------------------------------------------------------

/// Two memories with EQUAL fused RRF score: A is KNN rank 1 + keyword rank 2,
/// B is KNN rank 2 + keyword rank 1 (both 1/61 + 1/62). The one stored FIRST
/// is aged two half-lives before the second is stored. Returns
/// `(stale_id, fresh_id, results)`.
async fn equal_rank_pair(stale_is_a: bool) -> (i64, i64, Vec<i64>) {
    const Q: &str = "lantern";
    const A: &str = "lantern maintenance schedule for the north wing corridor";
    const B: &str = "lantern lantern checklist";
    let mut vectors = HashMap::new();
    vectors.insert(Q.to_string(), unit_axis(0));
    vectors.insert(A.to_string(), sim_vec(0.99, 1));
    vectors.insert(B.to_string(), sim_vec(0.98, 2));
    let h = harness(live(vectors));

    let (first, second) = if stale_is_a { (A, B) } else { (B, A) };
    let stale = store(&h, first, MemoryType::Pattern).await;
    h.clock.advance(DEFAULT_HALF_LIFE_SECS as i64 * 2);
    let fresh = store(&h, second, MemoryType::Pattern).await;

    let outcome = h.service.search(query(Q, None)).await.expect("search");
    assert_eq!(outcome.search_mode, SearchMode::Hybrid);
    (stale, fresh, ids(&outcome.results))
}

#[tokio::test]
async fn at_equal_fused_rank_the_fresh_memory_outranks_the_stale_one() {
    // Both orientations: the stale one is stored first, so it has the LOWER
    // id and would win the RRF id tie-break — only decay can put fresh first.
    for stale_is_a in [true, false] {
        let (stale, fresh, got) = equal_rank_pair(stale_is_a).await;
        assert_eq!(
            got,
            vec![fresh, stale],
            "fresh must strictly outrank stale at equal fused rank (stale_is_a = {stale_is_a})"
        );
    }
}

#[tokio::test]
async fn a_stale_pinned_memory_keeps_its_pinned_advantage_and_decay_applies_once() {
    // P (DECISION, pinned) is KNN rank 1 + keyword rank 1: RRF 2/61.
    // U (PATTERN, fresh) is KNN rank 2 + keyword rank 2: RRF 2/62.
    // P is aged 270_000 s. Pinned decay d = 2^(-270000 / (6 * 30d)) ≈ 0.98804.
    //   once, multiplicative : P = 2/61 * d   ≈ 0.032395 > U = 2/62 ≈ 0.032258
    //   twice (d^2 ≈ 0.97622): P ≈ 0.032007 < U   -> U would win
    //   pinning ignored (unpinned d ≈ 0.93031): P ≈ 0.030502 < U -> U would win
    //   additive (rrf + d)   : P ≈ 1.0208 < U ≈ 1.0323 -> U would win
    // So P first proves: multiplicative, once, pinned-aware.
    const Q: &str = "beacon";
    const P: &str = "beacon beacon protocol";
    const U: &str = "beacon protocol notes for the relay station handbook";
    const AGE: i64 = 270_000;
    let mut vectors = HashMap::new();
    vectors.insert(Q.to_string(), unit_axis(0));
    vectors.insert(P.to_string(), sim_vec(0.99, 1));
    vectors.insert(U.to_string(), sim_vec(0.98, 2));
    let h = harness(live(vectors));

    let p = store(&h, P, MemoryType::Decision).await;
    h.clock.advance(AGE);
    let u = store(&h, U, MemoryType::Pattern).await;

    let hl = DEFAULT_HALF_LIFE_SECS;
    let d_pinned = decay_score(AGE, 0, hl, true);
    assert!(d_pinned > 61.0 / 62.0 && d_pinned * d_pinned < 61.0 / 62.0);
    assert!(decay_score(AGE, 0, hl, false) < 61.0 / 62.0);

    let outcome = h.service.search(query(Q, None)).await.expect("search");
    assert_eq!(outcome.search_mode, SearchMode::Hybrid);
    assert_eq!(ids(&outcome.results), vec![p, u]);

    // The wire decay_score is the single, pinned-aware value at `now`.
    let now = h.clock.now();
    let p_view = &outcome.results[0];
    assert!((p_view.decay_score - decay_score(now, p_view.last_accessed, hl, true)).abs() < EPS);
    assert!((p_view.decay_score - d_pinned).abs() < EPS);
    assert!((outcome.results[1].decay_score - 1.0).abs() < EPS);
}

// ---------------------------------------------------------------------------
// Test 5: emptiness is not a fallback
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_corpus_is_an_empty_hybrid_answer() {
    let h = harness(live(HashMap::new()));
    let outcome = h
        .service
        .search(query("nothing", None))
        .await
        .expect("search");
    assert_eq!(outcome.search_mode, SearchMode::Hybrid);
    assert!(outcome.results.is_empty());
}

#[tokio::test]
async fn filters_that_exclude_everything_stay_hybrid() {
    let h = harness(live(HashMap::new()));
    store(&h, "a scoped memory about caching", MemoryType::Pattern).await;
    let outcome = h
        .service
        .search(SearchArgs {
            query: "caching".to_string(),
            scope: Some("no-such-scope".to_string()),
            ..Default::default()
        })
        .await
        .expect("search");
    assert_eq!(
        outcome.search_mode,
        SearchMode::Hybrid,
        "zero matches in both legs is still a hybrid answer, never a fallback"
    );
    assert!(outcome.results.is_empty());
}

// Keeps the shared helpers referenced until Task 2 extends this file.
#[allow(dead_code)]
fn _harness_fields_used(h: &Harness) -> (&Path, &Arc<SqliteStore>) {
    let _ = raw_conn;
    (&h.db_path, &h.store)
}

#[allow(dead_code)]
fn _store_trait_in_scope(s: &dyn Store) -> &dyn Store {
    s
}
