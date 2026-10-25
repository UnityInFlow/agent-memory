#!/usr/bin/env bash
# Phase 5 gap closure, plan 05-05 Task 2: live-Ollama realism run.
#
# Performs the "on the other machine" step of the Phase 5 user story with the
# REAL binary against the REAL local embedder (INTEROP-02 criterion 3, D-13):
#
#   1. seed database A from a three-bullet GSD STATE.md fixture
#   2. `agent-memory export --output move.jsonl`          (A → file)
#   3. `agent-memory import --from jsonl` into database B  (file → B, rows pending)
#   4. `agent-memory serve-rest --db B --addr 127.0.0.1:0`; wait for the FIRST
#      sweep tick to backfill the pending rows against nomic-embed-text
#   5. `POST /api/search` with a PARAPHRASE of one memory → search_mode hybrid,
#      imported memory present in results
#   6. stop the daemon, print the evidence block
#
# This is NOT a cargo test and must not become one: it needs a local Ollama with
# nomic-embed-text, which CI does not have. The mechanism is covered with a fake
# embedder by `imported_rows_land_pending_and_the_sweep_backfills_them`; this run
# is the realism evidence 05-VERIFICATION.md routed to a human.
#
# Every wait is a counter-bounded loop (this host has no `timeout` binary). The
# temp directory is kept on purpose: the repo's pre-bash hook refuses to remove
# database files from the shell, and the kept files ARE the evidence.
set -u

OLLAMA_URL="${AGENT_MEMORY_OLLAMA_URL:-http://localhost:11434}"
STEP_SECS=0.5

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# wait_for <pattern> <file> <max_iterations>  (STEP_SECS per iteration)
wait_for() {
  local pattern="$1" file="$2" max="$3" i=0
  until grep -q -- "$pattern" "$file" 2>/dev/null; do
    if [ "$i" -ge "$max" ]; then
      return 1
    fi
    sleep "$STEP_SECS"
    i=$((i + 1))
  done
  return 0
}

# --- 0. precondition: live Ollama with the pinned model -----------------------
TAGS="$(curl -s --max-time 5 "$OLLAMA_URL/api/tags")" \
  || fail "Ollama is not reachable at $OLLAMA_URL — this run needs the live daemon"
MODEL_LINE="$(printf '%s' "$TAGS" | tr ',' '\n' | grep '"name":"nomic-embed-text' | head -1)"
[ -n "$MODEL_LINE" ] \
  || fail "Ollama at $OLLAMA_URL does not list nomic-embed-text — run: ollama pull nomic-embed-text"
OLLAMA_VERSION="$(curl -s --max-time 5 "$OLLAMA_URL/api/version")"

# --- 1. build the real binary -------------------------------------------------
cargo build -q -p agent-memory --locked || fail "cargo build -p agent-memory --locked"
B="target/debug/agent-memory"
[ -x "$B" ] || fail "binary not found at $B"
T="$(mktemp -d)"
echo "workdir: $T (kept)"

DPID=""
DPID_RECORDED=""
cleanup() {
  if [ -n "$DPID" ] && kill -0 "$DPID" 2>/dev/null; then
    kill "$DPID" 2>/dev/null
    wait "$DPID" 2>/dev/null
  fi
}
trap cleanup EXIT

M1="SQLite is the embedded storage engine so the memory daemon runs with no cloud account and no server process"
M2="Release tarballs carry a signed SLSA provenance attestation built on GitHub-hosted runners"
M3="Clippy must pass with -D warnings and cargo fmt must be clean before every commit"
# A paraphrase of M1 that shares (almost) no keyword with it — the semantic leg
# has to do the work.
QUERY="which database did we choose so that nothing needs a hosted service"

# --- 2. fixture: a GSD STATE.md with three Decisions bullets ------------------
printf '### Decisions\n\n- %s\n- %s\n- %s\n' "$M1" "$M2" "$M3" > "$T/STATE.md"

# --- 3. database A via the gsd-state importer ---------------------------------
"$B" import --from gsd-state --scope realism --db "$T/a.db" "$T/STATE.md" \
  > "$T/a-import.out" 2> "$T/a-import.err" || fail "gsd-state import into a.db: $(cat "$T/a-import.err")"
grep -q '^imported: 3, skipped duplicates: 0' "$T/a-import.out" \
  || fail "a.db import line unexpected: $(cat "$T/a-import.out")"

# --- 4. export A → move.jsonl -------------------------------------------------
"$B" export --db "$T/a.db" --output "$T/move.jsonl" 2> "$T/export.err" \
  || fail "export: $(cat "$T/export.err")"
grep -q 'exported 3 memories and 0 links' "$T/export.err" \
  || fail "export report unexpected: $(cat "$T/export.err")"
LINES="$(wc -l < "$T/move.jsonl" | tr -d ' ')"
[ "$LINES" = 4 ] || fail "move.jsonl must have 4 lines (header + 3 memories), got $LINES"
head -1 "$T/move.jsonl" | grep -q '"exported_rows":3' || fail "header lacks exported_rows 3"
head -1 "$T/move.jsonl" | grep -q '"exported_links":0' || fail "header lacks exported_links 0"

# --- 5. import move.jsonl into database B -------------------------------------
"$B" import --from jsonl --db "$T/b.db" "$T/move.jsonl" \
  > "$T/b-import.out" 2> "$T/b-import.err" || fail "jsonl import into b.db: $(cat "$T/b-import.err")"
grep -q '^imported: 3, skipped duplicates: 0, skipped expired: 0, links imported: 0, links skipped duplicates: 0, links dropped: 0$' "$T/b-import.out" \
  || fail "b.db six-count line unexpected: $(cat "$T/b-import.out")"
grep -q '^3 rows pending embedding' "$T/b-import.out" \
  || fail "pending line unexpected: $(cat "$T/b-import.out")"

# --- 6. serve-rest on B; first sweep tick backfills the three pending rows ----
RUST_LOG="${RUST_LOG:-info}" "$B" serve-rest --db "$T/b.db" --addr 127.0.0.1:0 \
  > /dev/null 2> "$T/daemon.log" &
DPID=$!
DPID_RECORDED="$DPID"
wait_for 'REST listening on' "$T/daemon.log" 60 \
  || fail "daemon did not log 'REST listening on' within 30 s: $(cat "$T/daemon.log")"
PORT="$(grep -o 'REST listening on 127.0.0.1:[0-9]*' "$T/daemon.log" | head -1 | sed 's/.*://')"
[ -n "$PORT" ] || fail "could not parse the ephemeral port from: $(grep 'REST listening on' "$T/daemon.log")"
wait_for 'embedded pending memories on sweep' "$T/daemon.log" 240 \
  || fail "first sweep did not backfill within 120 s: $(cat "$T/daemon.log")"
grep -q 'backfilled=3' "$T/daemon.log" \
  || fail "expected backfilled=3, got: $(grep 'embedded pending memories on sweep' "$T/daemon.log")"

# --- 7. /health then the paraphrase search ------------------------------------
curl -s --max-time 10 "http://127.0.0.1:$PORT/health" > "$T/health.json" || fail "GET /health"
grep -q '"embedder":"ready"' "$T/health.json" || fail "embedder not ready: $(cat "$T/health.json")"

jq -n --arg q "$QUERY" '{query: $q, limit: 3}' > "$T/search-request.json"
curl -s --max-time 60 -X POST "http://127.0.0.1:$PORT/api/search" \
  -H 'content-type: application/json' --data-binary @"$T/search-request.json" \
  > "$T/search.json" || fail "POST /api/search"
grep -q '"search_mode":"hybrid"' "$T/search.json" \
  || fail "search_mode is not hybrid: $(cat "$T/search.json")"
RANK="$(jq -r --arg m "$M1" '[.results[].content] | index($m)' "$T/search.json")"
[ -n "$RANK" ] && [ "$RANK" != "null" ] \
  || fail "the imported memory is not in results: $(cat "$T/search.json")"
ID="$(jq -r --arg m "$M1" '.results[] | select(.content == $m) | .id' "$T/search.json" | head -1)"

# --- 8. stop the daemon --------------------------------------------------------
cleanup
DPID=""

# --- 9. evidence block -----------------------------------------------------------
{
  echo "## Live Ollama realism evidence ($(date -u +%Y-%m-%dT%H:%M:%SZ))"
  echo
  echo "- host: $(uname -srm); Ollama at $OLLAMA_URL, version $OLLAMA_VERSION; model: $MODEL_LINE"
  echo "- binary: $B ($("$B" --version 2>/dev/null | head -1)); workdir: $T (kept, not removed)"
  echo
  echo '```text'
  echo "\$ $B import --from gsd-state --scope realism --db $T/a.db $T/STATE.md"
  cat "$T/a-import.out"
  echo "\$ $B export --db $T/a.db --output $T/move.jsonl"
  cat "$T/export.err"
  echo "move.jsonl line 1: $(head -1 "$T/move.jsonl")"
  echo "\$ $B import --from jsonl --db $T/b.db $T/move.jsonl"
  cat "$T/b-import.out"
  echo "\$ RUST_LOG=info $B serve-rest --db $T/b.db --addr 127.0.0.1:0   (pid $DPID_RECORDED, stderr → daemon.log)"
  grep -E 'semantic search ready|REST listening on|lifecycle sweep complete|embedded pending memories on sweep|Ollama' "$T/daemon.log"
  echo "\$ curl http://127.0.0.1:$PORT/health"
  cat "$T/health.json"
  echo
  echo "\$ curl -X POST http://127.0.0.1:$PORT/api/search -d '$(cat "$T/search-request.json" | tr -d '\n')'"
  cat "$T/search.json"
  echo
  echo "paraphrase target rank (0 = first): $RANK; imported memory id in b.db: $ID"
  echo '```'
} | tee "$T/EVIDENCE.md"

echo "PASS: live Ollama realism run — search_mode hybrid, imported memory id $ID at rank $RANK (evidence: $T/EVIDENCE.md)"
