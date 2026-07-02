-- 0002_embeddings.sql — semantic-search sidecar schema (Phase 2, SEARCH-02/03).
-- Run via rusqlite_migration `M::up`. Requires the sqlite-vec extension to be
-- registered process-globally BEFORE any Connection::open (register_vec_extension
-- in sqlite.rs), or the vec0 CREATE below fails.
--
-- DELIBERATE DIVERGENCE from 0001: NO triggers are created for vec_memories.
-- vec0 virtual tables ignore triggers (RESEARCH Pitfall 3), so vector deletes
-- are explicit in Rust — forget() and sweep_expired() each clear their rows.

-- 0 = none/pending (Ollama was down at store time; backfilled by the sweep),
-- 1 = present in vec_memories.
ALTER TABLE memories ADD COLUMN embedding_status INTEGER NOT NULL DEFAULT 0;

CREATE VIRTUAL TABLE vec_memories USING vec0(
    memory_id INTEGER PRIMARY KEY,               -- == memories.id
    embedding FLOAT[768] distance_metric=cosine
);

-- Model/dimension pin: detectable drift instead of silent corpus corruption
-- (RESEARCH Pitfall 6). A different model/dim must degrade, never mix vectors.
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO meta(key, value) VALUES ('embedding_model', 'nomic-embed-text'),
                                    ('embedding_dim', '768');
