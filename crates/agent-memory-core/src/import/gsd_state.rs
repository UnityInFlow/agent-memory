//! Tolerant line-scanner parser for GSD `STATE.md` files (INTEROP-01).
//!
//! Maps the GSD sections to typed drafts (RESEARCH Pattern 5, verified against
//! this repo's real `.planning/STATE.md`):
//!
//! | STATE.md section        | Memory type  | Extra tag  |
//! |-------------------------|--------------|------------|
//! | `### Decisions`         | `Decision`   | —          |
//! | `### Blockers/Concerns` | `Constraint` | —          |
//! | `### Pending Todos`     | `Todo`       | —          |
//! | `## Deferred Items`     | `Todo`       | `deferred` |
//!
//! Tolerance rules (T-02-20/T-02-22): headings inside fenced code blocks never
//! change the current section; unknown sections are ignored; malformed bullets
//! and oversized items are skipped and counted; a garbage file yields zero
//! drafts and never panics. Pure function, no I/O — the CLI reads the file.

use crate::domain::{MemoryType, NewMemory};

/// Per-item content cap (bytes). Oversized bullets/rows are skipped and counted
/// instead of imported (T-02-22 — one runaway line must not bloat the store).
pub const MAX_IMPORT_CONTENT_BYTES: usize = 8192;

/// The provenance value stamped on every imported draft (T-02-23): auditable
/// and bulk-removable, and one leg of the `(source, mem_type, content)`
/// idempotency key.
const IMPORT_SOURCE: &str = "gsd-state";

/// The literal placeholder GSD writes under `### Pending Todos` when the list
/// is empty — never a real todo.
const EMPTY_PLACEHOLDER: &str = "None yet.";

/// Result of parsing one STATE.md: the typed drafts plus the count of lines
/// that were recognized as items but skipped (malformed or oversized).
#[derive(Debug)]
pub struct ParsedStateFile {
    /// Drafts ready for [`crate::service::MemoryService::import`].
    pub drafts: Vec<NewMemory>,
    /// Malformed or oversized items skipped by the tolerance rules.
    pub skipped: usize,
}

/// Which mapped STATE.md section the scanner is currently inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Decisions,
    Blockers,
    Todos,
    Deferred,
    Unknown,
}

/// Parse a GSD STATE.md into typed [`NewMemory`] drafts plus a skipped count.
///
/// Single-pass line scanner tracking (a) fence state — a line starting with
/// three backticks toggles it and headings inside a fence are ignored; (b) the
/// current section, matched on the trimmed heading text after `#` characters;
/// (c) bullets within mapped sections; (d) data rows of the Deferred Items
/// table. Every draft carries `source = "gsd-state"`, the `gsd` tag, the given
/// `scope`, and no TTL.
pub fn parse_gsd_state(text: &str, scope: Option<String>) -> ParsedStateFile {
    let mut drafts = Vec::new();
    let mut skipped = 0usize;
    let mut in_fence = false;
    let mut section = Section::Unknown;
    let mut deferred_header_seen = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // (a) Fence tracking: a fake `### Decisions` inside a code block must
        // never open the Decisions section.
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        // (b) Section switching on heading text, any heading level.
        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').trim();
            section = match heading {
                "Decisions" => Section::Decisions,
                "Blockers/Concerns" => Section::Blockers,
                "Pending Todos" => Section::Todos,
                "Deferred Items" => {
                    deferred_header_seen = false;
                    Section::Deferred
                }
                _ => Section::Unknown,
            };
            continue;
        }

        match section {
            // (c) Bullets in the three bullet-mapped sections.
            Section::Decisions | Section::Blockers | Section::Todos => {
                let mem_type = match section {
                    Section::Decisions => MemoryType::Decision,
                    Section::Blockers => MemoryType::Constraint,
                    _ => MemoryType::Todo,
                };
                if trimmed == "-" {
                    // Malformed: a lone dash with no content.
                    skipped += 1;
                    continue;
                }
                let Some(rest) = trimmed.strip_prefix("- ") else {
                    // Plain prose (e.g. the "None yet." placeholder line or a
                    // section preamble) — not an item, not an error.
                    continue;
                };
                let content = rest.trim();
                if content.is_empty() {
                    skipped += 1;
                    continue;
                }
                if content == EMPTY_PLACEHOLDER {
                    continue;
                }
                if content.len() > MAX_IMPORT_CONTENT_BYTES {
                    skipped += 1;
                    continue;
                }
                drafts.push(draft(content.to_string(), mem_type, false, &scope));
            }
            // (d) Deferred Items table: skip the header row and `---`
            // separator rows; join columns 2 (Item) and 3 (Status).
            Section::Deferred => {
                if !trimmed.starts_with('|') {
                    continue;
                }
                let is_separator = trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '));
                if is_separator {
                    continue;
                }
                if !deferred_header_seen {
                    deferred_header_seen = true;
                    continue;
                }
                let cells: Vec<&str> = trimmed
                    .trim_matches('|')
                    .split('|')
                    .map(str::trim)
                    .collect();
                if cells.len() < 3 {
                    skipped += 1;
                    continue;
                }
                let content = format!("{} — {}", cells[1], cells[2]);
                if content.is_empty() || content.len() > MAX_IMPORT_CONTENT_BYTES {
                    skipped += 1;
                    continue;
                }
                drafts.push(draft(content, MemoryType::Todo, true, &scope));
            }
            Section::Unknown => {}
        }
    }

    ParsedStateFile { drafts, skipped }
}

/// Build one draft with the fixed import provenance: source `gsd-state`, tag
/// `gsd` (plus `deferred` for table rows), the given scope, and no TTL.
fn draft(
    content: String,
    mem_type: MemoryType,
    deferred: bool,
    scope: &Option<String>,
) -> NewMemory {
    let mut tags = vec!["gsd".to_string()];
    if deferred {
        tags.push("deferred".to_string());
    }
    NewMemory {
        content,
        mem_type,
        tags,
        source: Some(IMPORT_SOURCE.to_string()),
        scope: scope.clone(),
        ttl_secs: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> ParsedStateFile {
        parse_gsd_state(text, Some("test-scope".to_string()))
    }

    #[test]
    fn decisions_bullets_map_to_decision_drafts_with_provenance() {
        let parsed = parse("### Decisions\n\n- first decision\n- second decision\n");
        assert_eq!(parsed.drafts.len(), 2);
        assert_eq!(parsed.skipped, 0);
        for d in &parsed.drafts {
            assert_eq!(d.mem_type, MemoryType::Decision);
            assert_eq!(d.source.as_deref(), Some("gsd-state"));
            assert!(d.tags.contains(&"gsd".to_string()));
            assert_eq!(d.scope.as_deref(), Some("test-scope"));
            assert_eq!(d.ttl_secs, None);
        }
    }

    #[test]
    fn blockers_map_to_constraint_drafts() {
        let parsed = parse("### Blockers/Concerns\n\n- a hard constraint\n");
        assert_eq!(parsed.drafts.len(), 1);
        assert_eq!(parsed.drafts[0].mem_type, MemoryType::Constraint);
    }

    #[test]
    fn pending_todos_placeholder_yields_zero_but_real_bullet_yields_one() {
        // "None yet." both as GSD's plain placeholder line and as a bullet.
        let parsed = parse("### Pending Todos\n\nNone yet.\n\n- None yet.\n- a real todo\n");
        assert_eq!(parsed.drafts.len(), 1);
        assert_eq!(parsed.drafts[0].mem_type, MemoryType::Todo);
        assert_eq!(parsed.drafts[0].content, "a real todo");
    }

    #[test]
    fn deferred_table_data_rows_map_to_tagged_todos() {
        let parsed = parse(
            "## Deferred Items\n\n| Category | Item | Status |\n|---|---|---|\n| Dist | win binaries | Deferred to v2 |\n| Search | rrf fusion | Deferred to v2 |\n",
        );
        assert_eq!(parsed.drafts.len(), 2, "header + separator rows excluded");
        for d in &parsed.drafts {
            assert_eq!(d.mem_type, MemoryType::Todo);
            assert!(d.tags.contains(&"gsd".to_string()));
            assert!(d.tags.contains(&"deferred".to_string()));
        }
        assert_eq!(parsed.drafts[0].content, "win binaries — Deferred to v2");
    }

    #[test]
    fn fenced_fake_heading_never_opens_a_section() {
        let parsed = parse(
            "## Notes\n\n```\n### Decisions\n- fenced bullet must not import\n```\n\n### Decisions\n\n- real decision\n",
        );
        assert_eq!(parsed.drafts.len(), 1, "only the real decision imports");
        assert_eq!(parsed.drafts[0].content, "real decision");
    }

    #[test]
    fn unknown_sections_are_ignored() {
        let parsed = parse("### Random Section\n\n- ignored bullet\n");
        assert!(parsed.drafts.is_empty());
        assert_eq!(parsed.skipped, 0, "unknown-section bullets are not errors");
    }

    #[test]
    fn oversized_bullet_is_skipped_and_counted() {
        let big = "x".repeat(MAX_IMPORT_CONTENT_BYTES + 1);
        let parsed = parse(&format!("### Decisions\n\n- {big}\n- small\n"));
        assert_eq!(parsed.drafts.len(), 1);
        assert_eq!(parsed.drafts[0].content, "small");
        assert_eq!(parsed.skipped, 1);
    }

    #[test]
    fn malformed_lone_dash_is_skipped_and_counted() {
        let parsed = parse("### Blockers/Concerns\n\n- real blocker\n-\n");
        assert_eq!(parsed.drafts.len(), 1);
        assert_eq!(parsed.skipped, 1);
    }

    #[test]
    fn headingless_garbage_yields_no_drafts_and_never_panics() {
        let parsed = parse("just prose\n- a stray bullet\n||\n```\nunclosed fence\n");
        assert!(parsed.drafts.is_empty());
        assert_eq!(parsed.skipped, 0);
    }
}
