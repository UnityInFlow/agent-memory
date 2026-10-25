//! Strict reader for format-v1 export files — the import half of
//! `agent-memory export` / `agent-memory import --from jsonl` (INTEROP-02).
//!
//! This module is deliberately the **opposite tolerance policy** from
//! [`super::gsd_state`] (Phase 5 D-11). A `STATE.md` is a human-edited text
//! file, so the gsd-state scanner degrades messy input to skip-counts. A JSONL
//! export is agent-memory's **own contract** ([`crate::export`]), so any defect
//! means corruption or a hand edit — and a half-restored corpus is worse than a
//! clear refusal. Every defect therefore rejects the **whole file**, and
//! nothing is written: [`parse_jsonl`] is the single entry point to the
//! importer's writes, and it only ever returns a fully validated
//! [`JsonlFile`].
//!
//! Pure function, no I/O — the CLI reads the file (D-01: import is a CLI
//! subcommand only; a file-path read on an unauthenticated daemon would be an
//! arbitrary-file-read primitive).
//!
//! # Error contract
//!
//! Every rejection is [`MemoryError::InvalidArgument`] whose text starts with
//! the **1-based line number** — `invalid argument: line 42: …` — so the CLI
//! prints a locatable message and any future transport maps it to REST 400 /
//! MCP `invalid_params` structurally (Phase 3 D-04/D-06). Line numbers come
//! from this module's own counter over `text.lines()`, **never** from
//! `serde_json::Error::line()`: serde sees one line at a time, so its own
//! number is always `1` (or `0` for data errors raised from a buffered
//! variant) — RESEARCH Pitfall 1.
//!
//! # Header handling
//!
//! Line 1 is probed **leniently** before the strict parse (RESEARCH Pattern 2):
//! a probe struct reads only `record`, `format` and `version`, ignoring unknown
//! fields, so a version-2 file that adds a field still reaches the
//! `upgrade agent-memory` message instead of failing first on
//! `unknown field …`. Only after the probe passes is line 1 parsed strictly as
//! [`Record::Header`].
//!
//! # Timestamp handling
//!
//! The parser validates every memory record's `last_accessed` (it must be an
//! integer) and then **drops** it: the importer refreshes `last_accessed` to
//! import-time `now` (D-10), so [`JsonlMemory`] carries an
//! [`ImportedMemory`] that has no such field. `created_at`, `access_count`
//! and `expires_at` are carried verbatim; whether a row is expired is decided
//! by the importer against the local clock, not here (expired rows are not
//! malformed).

use std::collections::HashSet;
use std::fmt::Display;

use serde::Deserialize;

use crate::domain::{validate_link_endpoints, ImportedMemory, LinkKind, MemoryError, MemoryType};
use crate::export::{Record, FORMAT_NAME, FORMAT_VERSION};

/// The validated header of a format-v1 file (line 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlHeader {
    /// Unix seconds at export time, from the exporting machine's clock.
    pub exported_at: i64,
    /// The source database's `meta.embedding_model` pin.
    pub embedding_model: String,
    /// The source database's `meta.embedding_dim` pin.
    pub embedding_dim: u32,
    /// The source database's schema version.
    pub schema_version: i64,
    /// The exporting binary's `CARGO_PKG_VERSION`.
    pub agent_memory_version: String,
    /// Number of `memory` records the file declares (and, once parsed,
    /// contains — a mismatch is rejected).
    pub exported_rows: u64,
    /// Number of `link` records the file declares (and contains).
    pub exported_links: u64,
}

/// One validated memory record: the export-local `key` the file's links
/// refer to, plus the row to restore. `key` is never written as an id — the
/// importer maps it to the target database's own id (PITFALLS 18).
#[derive(Debug, Clone, PartialEq)]
pub struct JsonlMemory {
    /// The export-local key (the source row id), used only to resolve links.
    pub key: i64,
    /// The row as the store will receive it (`last_accessed` already dropped).
    pub memory: ImportedMemory,
}

/// One validated link record, endpoints still expressed as export-local keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonlLink {
    /// `key` of the source memory.
    pub from: i64,
    /// `key` of the target memory.
    pub to: i64,
    /// The closed relation kind, already parsed through [`LinkKind::try_from`].
    pub kind: LinkKind,
    /// Unix seconds; preserved on import (edge recency orders expansion).
    pub created_at: i64,
}

/// A fully validated format-v1 file — the only input
/// [`crate::service::MemoryService::import_jsonl`] accepts. Every link's
/// endpoints name a key in `memories`; every type and kind is a closed enum;
/// no content is empty; the declared counts match.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonlFile {
    /// Line 1.
    pub header: JsonlHeader,
    /// Every memory record, in file order.
    pub memories: Vec<JsonlMemory>,
    /// Every link record, in file order.
    pub links: Vec<JsonlLink>,
}

/// The lenient line-1 probe (RESEARCH Pattern 2): no `deny_unknown_fields`,
/// so a newer version's extra header fields are ignored here and the version
/// check below gets to speak first. `format` and `version` are optional so
/// that a file whose first line is not a header at all is reported as such,
/// rather than as a missing field.
#[derive(Deserialize)]
struct HeaderProbe {
    record: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    version: Option<serde_json::Value>,
}

/// Build the D-11 rejection: `InvalidArgument("line {n}: {msg}")`, whose
/// Display is `invalid argument: line {n}: {msg}`.
fn invalid_line(line_no: usize, msg: impl Display) -> MemoryError {
    MemoryError::InvalidArgument(format!("line {line_no}: {msg}"))
}

/// Re-wrap a seam error (`MemoryType::try_from`, `LinkKind::try_from`,
/// `validate_link_endpoints`) under the line prefix. An `InvalidArgument`
/// already reads `invalid argument: …`, so its payload is lifted out rather
/// than nested (`invalid argument: line 3: invalid argument: kind …`); every
/// other variant keeps its full Display text.
fn rewrap(line_no: usize, e: MemoryError) -> MemoryError {
    match e {
        MemoryError::InvalidArgument(msg) => invalid_line(line_no, msg),
        other => invalid_line(line_no, other),
    }
}

/// serde_json appends ` at line 1 column N` to syntax and data errors — its
/// own per-line position, which is meaningless (and misleading) next to the
/// importer's line counter (Pitfall 1). Strip it; keep the message itself.
fn serde_message(e: &serde_json::Error) -> String {
    let text = e.to_string();
    let suffix = format!(" at line {} column {}", e.line(), e.column());
    match text.strip_suffix(&suffix) {
        Some(stripped) => stripped.to_string(),
        None => text,
    }
}

/// The lenient probe of line 1, in contract order: parseable → `record` is
/// `header` → `format` is [`FORMAT_NAME`] → `version` is an unsigned integer
/// → `>= 1` → `<= FORMAT_VERSION`. Returns `Ok(())` when the strict parse may
/// proceed.
fn check_header(line: &str) -> Result<(), MemoryError> {
    let probe: HeaderProbe = serde_json::from_str(line).map_err(|e| {
        invalid_line(
            1,
            format!("missing or invalid header ({})", serde_message(&e)),
        )
    })?;
    if probe.record != "header" {
        return Err(invalid_line(1, "first record must be the header"));
    }
    match probe.format.as_deref() {
        Some(format) if format == FORMAT_NAME => {}
        Some(format) => {
            return Err(invalid_line(
                1,
                format!("format must be '{FORMAT_NAME}' (got '{format}')"),
            ));
        }
        None => {
            return Err(invalid_line(
                1,
                format!("format must be '{FORMAT_NAME}' (missing)"),
            ));
        }
    }
    let version = match probe.version {
        Some(value) => value.as_u64().ok_or_else(|| {
            invalid_line(
                1,
                format!("version must be a positive integer (got {value})"),
            )
        })?,
        None => {
            return Err(invalid_line(
                1,
                "version must be a positive integer (missing)",
            ));
        }
    };
    if version == 0 {
        return Err(invalid_line(1, "version must be >= 1 (got 0)"));
    }
    if version > u64::from(FORMAT_VERSION) {
        return Err(invalid_line(
            1,
            format!(
                "file is format version {version} but this binary reads up to version \
                 {FORMAT_VERSION} — upgrade agent-memory"
            ),
        ));
    }
    Ok(())
}

/// Parse one strict [`Record`] from a line that has already passed the
/// object-shape check, wrapping serde's message under the line prefix.
fn parse_record(line_no: usize, line: &str) -> Result<Record, MemoryError> {
    serde_json::from_str::<Record>(line).map_err(|e| invalid_line(line_no, serde_message(&e)))
}

/// Parse and validate a whole format-v1 export file (D-08 pass 1, D-09, D-11).
///
/// Strict and all-or-nothing: the first defect is returned as
/// [`MemoryError::InvalidArgument`] naming the 1-based line, and no
/// [`JsonlFile`] exists for the importer to write from. Checks, in order:
///
/// 1. **Line 1** — the lenient header probe ([`check_header`]), then the strict
///    [`Record::Header`] parse.
/// 2. **Every later line** — must start with `{` (`expected a JSON object`);
///    must parse as a [`Record`] (unknown record tags, unknown fields and wrong
///    field types are serde errors); a second `header` is `duplicate header`;
///    a `memory` must have a valid `type` ([`MemoryType::try_from`]), non-empty
///    content and a `key` not seen before; a `link` must have a valid `kind`
///    ([`LinkKind::try_from`]) and distinct endpoints
///    ([`validate_link_endpoints`]).
/// 3. **After the loop** — every link endpoint must be a memory key that
///    appears *anywhere* in the file (order-independent, so a hand-ordered file
///    still parses), and the record counts must equal the header's
///    `exported_rows` / `exported_links` (a truncated file is refused).
///
/// A zero-byte or header-less file fails at line 1. Trailing blank lines are
/// NOT tolerated — the exporter never writes them, and a blank line is where a
/// truncated or concatenated file shows first.
pub fn parse_jsonl(text: &str) -> Result<JsonlFile, MemoryError> {
    let mut lines = text.lines().enumerate();

    let Some((_, first)) = lines.next() else {
        return Err(invalid_line(1, "missing or invalid header (empty file)"));
    };
    check_header(first)?;
    let header = match parse_record(1, first)? {
        Record::Header {
            exported_at,
            embedding_model,
            embedding_dim,
            schema_version,
            agent_memory_version,
            exported_rows,
            exported_links,
            // `format` / `version` were validated by the probe.
            format: _,
            version: _,
        } => JsonlHeader {
            exported_at,
            embedding_model,
            embedding_dim,
            schema_version,
            agent_memory_version,
            exported_rows,
            exported_links,
        },
        Record::Memory { .. } | Record::Link { .. } => {
            return Err(invalid_line(1, "first record must be the header"));
        }
    };

    let mut memories: Vec<JsonlMemory> = Vec::new();
    let mut links: Vec<(usize, JsonlLink)> = Vec::new();
    let mut keys: HashSet<i64> = HashSet::new();
    let mut last_line = 1usize;

    for (idx, line) in lines {
        let line_no = idx + 1;
        last_line = line_no;
        if !line.trim_start().starts_with('{') {
            return Err(invalid_line(line_no, "expected a JSON object"));
        }
        match parse_record(line_no, line)? {
            Record::Header { .. } => {
                return Err(invalid_line(line_no, "duplicate header"));
            }
            Record::Memory {
                key,
                mem_type,
                content,
                tags,
                source,
                scope,
                created_at,
                // Validated as an integer by serde, then dropped: the importer
                // refreshes it to import-time now (D-10).
                last_accessed: _,
                access_count,
                expires_at,
            } => {
                // The shared Phase 3 seam: the wire string becomes the closed
                // enum here or nowhere (D-09).
                let mem_type =
                    MemoryType::try_from(mem_type.as_str()).map_err(|e| rewrap(line_no, e))?;
                if content.trim().is_empty() {
                    return Err(invalid_line(line_no, "content must be a non-empty string"));
                }
                if !keys.insert(key) {
                    return Err(invalid_line(line_no, format!("duplicate memory key {key}")));
                }
                memories.push(JsonlMemory {
                    key,
                    memory: ImportedMemory {
                        content,
                        mem_type,
                        tags,
                        source,
                        scope,
                        created_at,
                        access_count,
                        expires_at,
                    },
                });
            }
            Record::Link {
                from,
                to,
                kind,
                created_at,
            } => {
                let kind = LinkKind::try_from(kind.as_str()).map_err(|e| rewrap(line_no, e))?;
                validate_link_endpoints(from, to).map_err(|e| rewrap(line_no, e))?;
                links.push((
                    line_no,
                    JsonlLink {
                        from,
                        to,
                        kind,
                        created_at,
                    },
                ));
            }
        }
    }

    // Links may name a key defined later in the file (order-independent), so
    // the key set is only complete once the loop has finished.
    for (line_no, link) in &links {
        for endpoint in [link.from, link.to] {
            if !keys.contains(&endpoint) {
                return Err(invalid_line(
                    *line_no,
                    format!("link key {endpoint} does not appear in the file"),
                ));
            }
        }
    }

    let memory_count = memories.len() as u64;
    if memory_count != header.exported_rows {
        return Err(invalid_line(
            last_line,
            format!(
                "file declares {} memory records but contains {memory_count}",
                header.exported_rows
            ),
        ));
    }
    let link_count = links.len() as u64;
    if link_count != header.exported_links {
        return Err(invalid_line(
            last_line,
            format!(
                "file declares {} link records but contains {link_count}",
                header.exported_links
            ),
        ));
    }

    Ok(JsonlFile {
        header,
        memories,
        links: links.into_iter().map(|(_, link)| link).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid v1 header declaring `rows` memories and `links` links.
    fn header(rows: u64, links: u64) -> String {
        format!(
            r#"{{"record":"header","format":"agent-memory-export","version":1,"exported_at":1000,"embedding_model":"nomic-embed-text","embedding_dim":768,"schema_version":3,"agent_memory_version":"0.0.1","exported_rows":{rows},"exported_links":{links}}}"#
        )
    }

    const MEM1: &str = r#"{"record":"memory","key":1,"type":"DECISION","content":"a","tags":[],"source":null,"scope":null,"created_at":1,"last_accessed":1,"access_count":0,"expires_at":null}"#;
    const MEM2: &str = r#"{"record":"memory","key":2,"type":"PATTERN","content":"b","tags":["t"],"source":"s","scope":"sc","created_at":2,"last_accessed":2,"access_count":3,"expires_at":99}"#;
    const LINK12: &str = r#"{"record":"link","from":1,"to":2,"kind":"relates_to","created_at":5}"#;

    fn file(lines: &[&str]) -> String {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    fn expect_invalid(name: &str, text: &str, needle: &str) {
        let err = parse_jsonl(text).expect_err(name);
        assert!(
            matches!(err, MemoryError::InvalidArgument(_)),
            "{name}: expected InvalidArgument, got {err:?}"
        );
        let shown = err.to_string();
        assert!(
            shown.contains(needle),
            "{name}: expected {needle:?} in {shown:?}"
        );
        assert!(
            shown.starts_with("invalid argument: line "),
            "{name}: D-11 shape, got {shown:?}"
        );
    }

    #[test]
    fn a_two_memory_one_link_file_parses_into_typed_records() {
        let text = file(&[&header(2, 1), MEM1, MEM2, LINK12]);
        let parsed = parse_jsonl(&text).expect("valid file parses");

        assert_eq!(parsed.header.exported_rows, 2);
        assert_eq!(parsed.header.exported_links, 1);
        assert_eq!(parsed.header.exported_at, 1_000);
        assert_eq!(parsed.header.embedding_model, "nomic-embed-text");
        assert_eq!(parsed.header.embedding_dim, 768);
        assert_eq!(parsed.header.schema_version, 3);
        assert_eq!(parsed.header.agent_memory_version, "0.0.1");

        assert_eq!(parsed.memories.len(), 2);
        assert_eq!(parsed.memories[0].key, 1);
        assert_eq!(parsed.memories[0].memory.mem_type, MemoryType::Decision);
        assert_eq!(parsed.memories[0].memory.content, "a");
        assert_eq!(parsed.memories[1].key, 2);
        assert_eq!(
            parsed.memories[1].memory,
            ImportedMemory {
                content: "b".to_string(),
                mem_type: MemoryType::Pattern,
                tags: vec!["t".to_string()],
                source: Some("s".to_string()),
                scope: Some("sc".to_string()),
                created_at: 2,
                access_count: 3,
                expires_at: Some(99),
            }
        );

        assert_eq!(parsed.links.len(), 1);
        assert_eq!(
            parsed.links[0],
            JsonlLink {
                from: 1,
                to: 2,
                kind: LinkKind::RelatesTo,
                created_at: 5,
            }
        );
    }

    #[test]
    fn a_newer_version_with_extra_fields_reaches_the_upgrade_message() {
        let text = "{\"record\":\"header\",\"format\":\"agent-memory-export\",\"version\":2,\"later\":1}\n";
        expect_invalid("version 2", text, "upgrade agent-memory");
        expect_invalid(
            "version 2 names both versions",
            text,
            "file is format version 2 but this binary reads up to version 1",
        );
    }

    #[test]
    fn a_wrong_format_is_rejected_at_line_one() {
        expect_invalid(
            "bad format",
            "{\"record\":\"header\",\"format\":\"x\",\"version\":1}\n",
            "line 1: format must be 'agent-memory-export' (got 'x')",
        );
    }

    #[test]
    fn version_zero_is_rejected_at_line_one() {
        expect_invalid(
            "version 0",
            "{\"record\":\"header\",\"format\":\"agent-memory-export\",\"version\":0}\n",
            "line 1: version must be >= 1 (got 0)",
        );
    }

    #[test]
    fn a_link_to_a_key_absent_from_the_file_is_rejected() {
        let dangling = r#"{"record":"link","from":1,"to":9,"kind":"relates_to","created_at":1}"#;
        expect_invalid(
            "dangling key",
            &file(&[&header(1, 1), MEM1, dangling]),
            "line 3: link key 9 does not appear in the file",
        );
    }

    #[test]
    fn an_empty_file_is_rejected_at_line_one() {
        expect_invalid("zero bytes", "", "line 1:");
        expect_invalid("one newline", "\n", "line 1:");
    }

    #[test]
    fn a_header_only_file_parses_to_no_records() {
        let parsed = parse_jsonl(&file(&[&header(0, 0)])).expect("header only parses");
        assert!(parsed.memories.is_empty());
        assert!(parsed.links.is_empty());
    }

    #[test]
    fn links_may_precede_the_memories_they_name() {
        let parsed = parse_jsonl(&file(&[&header(2, 1), LINK12, MEM1, MEM2]))
            .expect("order-independent key resolution");
        assert_eq!(parsed.memories.len(), 2);
        assert_eq!(parsed.links.len(), 1);
    }

    #[test]
    fn serde_position_suffix_is_stripped_from_messages() {
        let err = parse_jsonl(&file(&[&header(1, 0), r#"{"record":"memo","key":1}"#]))
            .expect_err("unknown record");
        let shown = err.to_string();
        assert!(shown.contains("unknown variant"), "{shown}");
        assert!(
            !shown.contains(" at line 1 column "),
            "serde's own position must never surface: {shown}"
        );
    }

    #[test]
    fn malformed_files_are_rejected_naming_the_line() {
        let h10 = header(1, 0);
        let h11 = header(1, 1);
        let h20 = header(2, 0);
        let h50 = header(5, 0);
        let h00 = header(0, 0);
        let unknown_field = MEM1.replace("\"expires_at\":null", "\"expires_at\":null,\"bogus\":1");
        let string_key = MEM1.replace("\"key\":1", "\"key\":\"1\"");
        let bad_type = MEM1.replace("\"type\":\"DECISION\"", "\"type\":\"MEMO\"");
        let blank_content = MEM1.replace("\"content\":\"a\"", "\"content\":\"   \"");
        let bad_kind = r#"{"record":"link","from":1,"to":2,"kind":"loves","created_at":1}"#;
        let self_link = r#"{"record":"link","from":1,"to":1,"kind":"relates_to","created_at":1}"#;
        let dangling = r#"{"record":"link","from":1,"to":9,"kind":"relates_to","created_at":1}"#;
        let string_tags = MEM1.replace("\"tags\":[]", "\"tags\":\"x\"");
        let float_expiry = MEM1.replace("\"expires_at\":null", "\"expires_at\":1.5");

        let hdr = |body: &str| format!("{{\"record\":\"header\",{body}}}\n");
        let fmt_x = hdr(r#""format":"x","version":1"#);
        let no_fmt = hdr(r#""version":1"#);
        let v0 = hdr(r#""format":"agent-memory-export","version":0"#);
        let v_str = hdr(r#""format":"agent-memory-export","version":"1""#);
        let v_frac = hdr(r#""format":"agent-memory-export","version":1.5"#);
        let v2 = hdr(r#""format":"agent-memory-export","version":2,"later":1"#);
        let v1_short = hdr(r#""format":"agent-memory-export","version":1"#);
        let memo = r#"{"record":"memo","key":1}"#;

        // One case per line on purpose: the table IS the D-11 contract, and a
        // reader should be able to scan name / input / expected prefix across.
        #[rustfmt::skip]
        let cases: Vec<(&str, String, &str)> = vec![
            ("no header", file(&[MEM1]), "line 1: first record must be the header"),
            ("not json at all", "garbage\n".to_string(), "line 1: missing or invalid header"),
            ("bad format", fmt_x, "line 1: format must be 'agent-memory-export' (got 'x')"),
            ("missing format", no_fmt, "line 1: format must be 'agent-memory-export' (missing)"),
            ("version 0", v0, "line 1: version must be >= 1 (got 0)"),
            ("string version", v_str, "line 1: version must be a positive integer (got \"1\")"),
            ("fractional version", v_frac, "line 1: version must be a positive integer (got 1.5)"),
            ("version 2 + unknown field", v2, "upgrade agent-memory"),
            ("header missing a required field", v1_short, "line 1: missing field"),
            ("not an object", file(&[&h10, "[1,2]"]), "line 2: expected a JSON object"),
            ("blank line", file(&[&h10, ""]), "line 2: expected a JSON object"),
            ("unknown record", file(&[&h10, memo]), "line 2: unknown variant `memo`"),
            ("unknown field", file(&[&h10, &unknown_field]), "line 2: unknown field `bogus`"),
            ("string key", file(&[&h10, &string_key]), "line 2: invalid type: string \"1\""),
            ("bad type", file(&[&h10, &bad_type]), "line 2: invalid memory type 'MEMO'"),
            ("blank content", file(&[&h10, &blank_content]), "line 2: content must be a non-empty string"),
            ("tags not an array", file(&[&h10, &string_tags]), "line 2: invalid type"),
            ("fractional expires_at", file(&[&h10, &float_expiry]), "line 2: invalid type"),
            ("bad kind", file(&[&h11, MEM1, bad_kind]), "line 3: kind must be one of"),
            ("self link", file(&[&h11, MEM1, self_link]), "line 3: from_id and to_id must differ"),
            ("dangling key", file(&[&h11, MEM1, dangling]), "line 3: link key 9 does not appear in the file"),
            ("duplicate key", file(&[&h20, MEM1, MEM1]), "line 3: duplicate memory key 1"),
            ("count mismatch", file(&[&h50, MEM1]), "line 2: file declares 5 memory records but contains 1"),
            ("link count mismatch", file(&[&h11, MEM1]), "line 2: file declares 1 link records but contains 0"),
            ("duplicate header", file(&[&h00, &h00]), "line 2: duplicate header"),
        ];
        assert!(cases.len() >= 18, "the D-11 table must cover every class");
        for (name, text, needle) in &cases {
            expect_invalid(name, text, needle);
        }
    }

    #[test]
    fn hostile_input_never_panics() {
        // 300 nested arrays as `tags`: serde_json's recursion limit (128)
        // returns an error long before the stack is at risk.
        let nested = format!("{}{}", "[".repeat(300), "]".repeat(300));
        let deep_tags = MEM1.replace("\"tags\":[]", &format!("\"tags\":{nested}"));
        assert!(parse_jsonl(&file(&[&header(1, 0), &deep_tags])).is_err());

        // A 4 MiB content string is data, not an attack: it parses.
        let big = "x".repeat(4 << 20);
        let big_content = MEM1.replace("\"content\":\"a\"", &format!("\"content\":\"{big}\""));
        let parsed =
            parse_jsonl(&file(&[&header(1, 0), &big_content])).expect("4 MiB content parses");
        assert_eq!(parsed.memories[0].memory.content.len(), 4 << 20);

        // 1 MiB of `{`: unbalanced and too deep — an error, not an abort.
        let braces = "{".repeat(1 << 20);
        assert!(parse_jsonl(&file(&[&header(1, 0), &braces])).is_err());

        // A NUL escape inside content is stored as data.
        let nul = MEM1.replace("\"content\":\"a\"", "\"content\":\"a\\u0000b\"");
        let parsed = parse_jsonl(&file(&[&header(1, 0), &nul])).expect("NUL is data");
        assert_eq!(parsed.memories[0].memory.content, "a\u{0}b");
    }
}
