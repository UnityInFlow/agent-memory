//! Private, atomic export-file writing for `agent-memory export --output`.
//!
//! This is the ONE place that touches export-file permissions and atomic
//! replacement (Phase 5 D-02) — keep it here so the Windows ACL story
//! (deferred to Phase 6, DIST-03) is a single-file change, exactly like
//! `config.rs` for the data directory.
//!
//! Three rules, each with a reason:
//!
//! - **Mode `0600` from the first byte.** The export is the whole memory
//!   corpus — T-01-03-sensitive, and never to be looser than the `0700`
//!   database directory. The mode is set at `create`, so there is no
//!   `0644`-then-`chmod` window (T-05-02); umask can only remove bits.
//! - **The temp file lives beside the target.** `std::fs::rename` does not
//!   cross mount points, and a shared temp directory is a different trust
//!   domain (other users, cleaners, predictable names). The temp is
//!   `{target}.export-tmp-{pid}` in the target's own directory.
//! - **`create_new`, never truncate.** A stale temp from a crashed run is an
//!   explicit `AlreadyExists` error the user sees, never a silent overwrite of
//!   whatever that file is — the pid suffix makes a collision rare, not
//!   impossible.
//!
//! The write is committed only after the stream is flushed (the service does
//! that inside its blocking task), `sync_all`-ed and renamed over the target
//! in one step, so the target is always either the previous complete file or
//! the new complete file, never a partial one (T-05-03). Every failure path
//! removes the temp best-effort and never masks the original error.

use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};

/// An export in flight: a private temp file beside `target`, to be
/// [`committed`](PendingExport::commit) into place or
/// [`aborted`](PendingExport::abort). Dropping it without either leaves the
/// temp on disk — callers must do one or the other.
#[derive(Debug)]
pub struct PendingExport {
    temp: PathBuf,
    target: PathBuf,
    file: File,
}

/// The in-flight temp path for `target`: `{file name}.export-tmp-{pid}` in
/// the SAME directory, so the final rename never crosses a filesystem. A
/// target without a usable file name (e.g. `/`) falls back to the name
/// `export`.
pub fn export_temp_path(target: &Path) -> PathBuf {
    let mut name: OsString = target
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("export"));
    name.push(format!(".export-tmp-{}", std::process::id()));
    target.with_file_name(name)
}

/// Start an export to `target`: create its private temp file (mode `0600` on
/// Unix, `create_new`) in the target's directory. The target itself is not
/// touched until [`PendingExport::commit`].
pub fn begin(target: &Path) -> std::io::Result<PendingExport> {
    let temp = export_temp_path(target);
    let file = create_private(&temp)?;
    Ok(PendingExport {
        temp,
        target: target.to_path_buf(),
        file,
    })
}

impl PendingExport {
    /// The temp file this export is being written to (test-only accessor;
    /// production callers never need the path — `commit`/`abort` own it).
    #[cfg(test)]
    pub fn temp_path(&self) -> &Path {
        &self.temp
    }

    /// A second handle on the temp file for the writer, so this struct keeps
    /// the original for `sync_all` at commit. Both handles share one file
    /// position and one set of bytes.
    pub fn writer(&self) -> std::io::Result<File> {
        self.file.try_clone()
    }

    /// Durably publish the export: `sync_all` the temp, then rename it over
    /// `target` (replacing an existing file). The caller must have flushed its
    /// writer first. On any failure the temp is removed (best-effort) and the
    /// error is returned unchanged; the previous target, if any, is untouched.
    pub fn commit(self) -> std::io::Result<()> {
        if let Err(e) = self.file.sync_all() {
            discard_temp(&self.temp);
            return Err(e);
        }
        if let Err(e) = std::fs::rename(&self.temp, &self.target) {
            discard_temp(&self.temp);
            return Err(e);
        }
        Ok(())
    }

    /// Give up: remove the temp (best-effort) and leave `target` exactly as it
    /// was. Never fails — this runs on error paths and must not mask them.
    pub fn abort(self) {
        discard_temp(&self.temp);
    }
}

/// Best-effort removal of an export temp file. A leftover is harmless but
/// noisy, so a non-`NotFound` failure is logged at warn level and otherwise
/// ignored — mirroring the store's backup-temp discipline.
fn discard_temp(temp: &Path) {
    if let Err(e) = std::fs::remove_file(temp) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(temp = %temp.display(), error = %e, "could not remove export temp file");
        }
    }
}

/// Create `path` for writing, owner-only (`0600`), refusing to open an
/// existing file. The mode is applied at creation, so the file is never
/// readable by others for even an instant.
#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Non-Unix fallback: `create_new` without a mode. Windows ACL inheritance is
/// the Phase 6 seam (DIST-03), the same split as `config.rs::create_data_dir`.
#[cfg(not(unix))]
fn create_private(path: &Path) -> std::io::Result<File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_entries(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .expect("read dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".export-tmp-"))
            .collect()
    }

    #[test]
    fn begin_creates_a_private_temp_beside_the_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("out.jsonl");

        let pending = begin(&target).expect("begin");

        let expected_temp = dir
            .path()
            .join(format!("out.jsonl.export-tmp-{}", std::process::id()));
        assert_eq!(pending.temp_path(), expected_temp.as_path());
        assert!(expected_temp.exists(), "the temp file exists");
        assert!(!target.exists(), "the target does not exist until commit");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&expected_temp)
                .expect("temp metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "temp is owner-only (T-05-02)");
        }
        pending.abort();
    }

    #[test]
    fn commit_replaces_an_existing_target_and_leaves_no_temp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("out.jsonl");
        std::fs::write(&target, "old").expect("seed target");

        let pending = begin(&target).expect("begin");
        {
            let mut w = pending.writer().expect("writer");
            w.write_all(b"new\n").expect("write");
            w.flush().expect("flush");
        }
        pending.commit().expect("commit");

        assert_eq!(std::fs::read_to_string(&target).expect("read"), "new\n");
        assert!(temp_entries(dir.path()).is_empty(), "no temp left behind");
    }

    #[test]
    fn abort_removes_the_temp_and_keeps_the_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("out.jsonl");
        std::fs::write(&target, "old").expect("seed target");

        let pending = begin(&target).expect("begin");
        pending
            .writer()
            .expect("writer")
            .write_all(b"partial")
            .expect("write");
        pending.abort();

        assert_eq!(std::fs::read_to_string(&target).expect("read"), "old");
        assert!(temp_entries(dir.path()).is_empty(), "no temp left behind");
    }

    #[test]
    fn begin_refuses_to_clobber_an_existing_temp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("out.jsonl");
        let stale = export_temp_path(&target);
        std::fs::write(&stale, "stale run").expect("seed stale temp");

        let err = begin(&target).expect_err("create_new must refuse an existing temp");
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read_to_string(&stale).expect("read stale"),
            "stale run",
            "the stale temp is never truncated"
        );
    }

    #[test]
    fn export_temp_path_stays_in_the_target_directory() {
        let target = std::path::Path::new("/a/b/out.jsonl");
        let temp = export_temp_path(target);
        assert_eq!(temp.parent(), Some(std::path::Path::new("/a/b")));
        let name = temp.file_name().expect("file name").to_string_lossy();
        assert!(
            name.starts_with("out.jsonl.export-tmp-"),
            "temp name is derived from the target: {name}"
        );
    }
}
