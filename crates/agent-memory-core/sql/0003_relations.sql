-- 0003_relations.sql — flat typed relations between memories (Phase 4, MCP-07).
-- Run via rusqlite_migration `M::up`. Connection-level PRAGMAs (WAL, synchronous,
-- foreign_keys, busy_timeout) are applied per-connection in sqlite.rs, NOT here —
-- `foreign_keys = ON` there is what makes the cascading FK deletes below fire.
--
-- APPEND-ONLY: 0001/0002 shipped in v0.0.1 and are FROZEN (STORE-05). Schema
-- changes always land as a new numbered migration; the committed fixture test in
-- tests/migration_hygiene.rs turns any edit of a shipped migration red.
--
-- Edges are stored DIRECTED (from_id -> to_id) so `supersedes` / `caused_by` keep
-- their meaning; 1-hop expansion reads them in both directions (D-03). One row
-- per edge — no mirrored reverse rows.

CREATE TABLE memory_links (
    from_id    INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    to_id      INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL,            -- relates_to|supersedes|caused_by (closed enum, validated at the service seam)
    created_at INTEGER NOT NULL,            -- UTC unix epoch (i64), injected clock; most-recent edges win (D-07)
    PRIMARY KEY (from_id, to_id, kind),     -- one edge per (from, to, kind): duplicate link is a no-op (D-02)
    CHECK (from_id != to_id)                -- belt-and-braces; the service rejects self-links first (D-02)
) WITHOUT ROWID;

-- Reverse-direction expansion (edges pointing AT a memory). The primary key
-- already covers lookups by from_id.
CREATE INDEX idx_links_to ON memory_links(to_id);
