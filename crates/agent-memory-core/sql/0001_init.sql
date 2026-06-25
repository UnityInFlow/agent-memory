-- 0001_init.sql — initial schema for agent-memory (Phase 1).
-- Run via rusqlite_migration `M::up`. Connection-level PRAGMAs (WAL, synchronous,
-- foreign_keys, busy_timeout) are applied per-connection in sqlite.rs, NOT here.
-- No embedding column in Phase 1; it is added by a later Phase-2 migration.

CREATE TABLE memories (
    id            INTEGER PRIMARY KEY,          -- rowid; == memories_fts.rowid
    mem_type      TEXT NOT NULL,                -- DECISION|PATTERN|ERROR|TODO|ARCHITECTURE|CONSTRAINT
    content       TEXT NOT NULL,
    tags          TEXT NOT NULL DEFAULT '[]',   -- JSON array (serde_json)
    source        TEXT,                         -- NULL allowed (D-05)
    scope         TEXT,                         -- NULL = global (D-03/D-05)
    base_weight   REAL NOT NULL DEFAULT 1.0,    -- per-type importance (pinning feeds this)
    decay_score   REAL NOT NULL DEFAULT 1.0,    -- materialized by sweep; recomputed on read
    access_count  INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,             -- UTC unix epoch (i64)
    last_accessed INTEGER NOT NULL,             -- UTC unix epoch (i64)
    expires_at    INTEGER                       -- NULL = no TTL (STORE-04)
);

CREATE INDEX idx_memories_type    ON memories(mem_type);
CREATE INDEX idx_memories_scope   ON memories(scope);
CREATE INDEX idx_memories_expires ON memories(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX idx_memories_decay   ON memories(decay_score);

-- FTS5 external-content mirror over memories.content, kept in sync by triggers.
-- Created now (Phase 1) so Plan 02's keyword search has no schema work to do.
CREATE VIRTUAL TABLE memories_fts USING fts5(
    content,
    content='memories',
    content_rowid='id'
);

CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content) VALUES('delete', old.id, old.content);
END;

CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content) VALUES('delete', old.id, old.content);
    INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
END;
