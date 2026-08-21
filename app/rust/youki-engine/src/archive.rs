//! archive — port of FileManagerFragment.kt's ZIP compress/extract/browse
//! logic. This is genuine CPU-bound compute (compression/decompression),
//! not an Android Framework API call, so it moves to Rust per the handoff
//! doc's "pure compute" category — unlike the file-open dialogs, progress
//! UI (ProgressDialog), and toast/snackbar calls in the original, which are
//! Android UI framework calls and stay in Kotlin (the JNI bridge, once
//! written, is expected to call these functions from a background thread
//! and report progress back to Kotlin via a callback — see the progress
//! parameter shape noted on each function below).
//!
//! SECURITY-CRITICAL: extract_zip preserves the original's zip-slip
//! path-traversal check verbatim. A malicious ZIP entry named e.g.
//! "../../../../data/data/other.app/files/evil" must not be allowed to
//! write outside the destination directory — this is not a style choice,
//! it's the difference between a working extractor and an arbitrary-file-
//! write vulnerability. Do not simplify this check away.

use std::fs;
use std::io::{self};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

// NOTE: not yet called from the JNI bridge (jni_bridge.rs) — the Kotlin side
// (FileManagerFragment.kt's zip-browsing UI) hasn't been wired to this port
// yet. Kept and allowed rather than deleted: it's a complete, correct port
// of loadZipEntries() per this file's module doc, ready for the JNI bridge
// call once that migration step happens.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ZipEntryInfo {
    pub name: String,
    pub full_path: String,
    pub is_directory: bool,
    pub size: u64,
    pub compressed_size: u64,
}

/// Port of `loadZipEntries()` — lists the direct children (files and
/// folders) of `prefix` inside the zip, without recursing further (folders
/// are collapsed into a single synthetic entry, same as the original).
/// Sorted the same way: directories first, then alphabetically
/// case-insensitive within each group.
#[allow(dead_code)]
pub fn list_zip_entries(zip_path: &Path, prefix: &str) -> io::Result<Vec<ZipEntryInfo>> {
    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file).map_err(to_io_err)?;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut result: Vec<ZipEntryInfo> = Vec::new();

    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(to_io_err)?;
        let name = entry.name().to_string();
        if !name.starts_with(prefix) {
            continue;
        }
        let relative = &name[prefix.len()..];
        if relative.is_empty() {
            continue;
        }

        match relative.find('/') {
            None => {
                // A direct file at this level.
                if seen.insert(relative.to_string()) {
                    result.push(ZipEntryInfo {
                        name: relative.to_string(),
                        full_path: name.clone(),
                        is_directory: false,
                        size: entry.size(),
                        compressed_size: entry.compressed_size(),
                    });
                }
            }
            Some(slash_index) => {
                // A folder.
                let dir_name = &relative[..slash_index];
                if seen.insert(dir_name.to_string()) {
                    result.push(ZipEntryInfo {
                        name: dir_name.to_string(),
                        full_path: format!("{prefix}{dir_name}/"),
                        is_directory: true,
                        size: 0,
                        compressed_size: 0,
                    });
                }
            }
        }
    }

    // Directories first, then alphabetical case-insensitive — matches
    // `sortedWith(compareBy({ !it.isDirectory }, { it.name.lowercase() }))`.
    result.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(result)
}

/// Progress callback: (current_index, total_count) -> called after each
/// entry is processed. The JNI bridge is expected to marshal this back to
/// Kotlin's ProgressDialog updates (originally done via
/// `requireActivity().runOnUiThread { progress.progress = ... }`).
pub type ProgressCallback<'a> = dyn FnMut(usize, usize) + 'a;

/// Port of `extractZip()`. SECURITY: preserves the original's zip-slip
/// protection verbatim — any entry whose resolved output path would land
/// outside `dest_dir` is skipped rather than written, exactly like the
/// original's `if (!outFile.canonicalPath.startsWith(destCanon...)) return`.
pub fn extract_zip(
    zip_path: &Path,
    dest_dir: &Path,
    delete_after: bool,
    mut on_progress: Option<&mut ProgressCallback>,
) -> io::Result<()> {
    fs::create_dir_all(dest_dir)?;
    // canonicalize() resolves symlinks + ".."/"." components, matching
    // File.canonicalPath's behavior in the original — this is the basis
    // the zip-slip check below relies on.
    let dest_canon = dest_dir.canonicalize()?;

    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file).map_err(to_io_err)?;
    let total = archive.len().max(1);

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(to_io_err)?;
        let entry_name = entry.name().to_string();
        let would_be_dir = entry.is_dir();

        // ── zip-slip protection (SECURITY-CRITICAL — see module doc) ──────
        // MUST run before any directory is created on disk. An earlier
        // draft of this function called create_dir_all(&parent) before this
        // check, which meant a malicious "../../evil" entry would already
        // have its escaping directory created before the check could
        // reject it — defeating the entire point of the check. Fixed here:
        // resolve the intended output path purely lexically (no filesystem
        // access, no directory creation) against dest_canon, reject
        // anything that escapes, and only touch the filesystem afterward.
        let joined = dest_canon.join(&entry_name);
        let resolved = lexically_normalize(&joined);
        if !(resolved.starts_with(&dest_canon) || resolved == dest_canon) {
            // Matches the original's `return@forEachIndexed` (skip this
            // entry silently) rather than aborting the whole extraction —
            // preserved as-is even though a stricter implementation might
            // prefer to abort entirely on any zip-slip attempt detected.
            if let Some(cb) = on_progress.as_deref_mut() {
                cb(i + 1, total);
            }
            continue;
        }

        if would_be_dir {
            fs::create_dir_all(&resolved)?;
            if let Some(cb) = on_progress.as_deref_mut() {
                cb(i + 1, total);
            }
            continue;
        }

        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent)?;
        }

        let final_out_path = resolved;

        let mut out_file = fs::File::create(&final_out_path)?;
        io::copy(&mut entry, &mut out_file)?;

        if let Some(cb) = on_progress.as_deref_mut() {
            cb(i + 1, total);
        }
    }

    if delete_after {
        // Matches the original's best-effort `zipFile.delete()` — ignore
        // failure (e.g. read-only filesystem), same as Kotlin's Boolean
        // return value being discarded there.
        let _ = fs::remove_file(zip_path);
    }

    Ok(())
}

/// Port of `doCompress()` / `doCompressMultiple()`'s shared `addToZip`
/// recursive helper + driving loop, unified into one function since both
/// callers only differed in whether they passed one root or several — this
/// version accepts a slice of roots, and a single-file call site can just
/// pass a one-element slice.
pub fn compress_to_zip(
    sources: &[PathBuf],
    zip_path: &Path,
    mut on_progress: Option<&mut ProgressCallback>,
) -> io::Result<()> {
    let file = fs::File::create(zip_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // Count total files up front for progress reporting (the original
    // didn't report progress during compression — its ProgressDialog was
    // `isIndeterminate = true` for compress, unlike extract's determinate
    // bar — so on_progress here is best-effort and can be None).
    let mut processed = 0usize;
    let total_estimate = count_files(sources);

    for source in sources {
        let root_name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        add_to_zip(source, &root_name, &mut zip, &options, &mut processed, total_estimate, &mut on_progress)?;
    }

    zip.finish().map_err(to_io_err)?;
    Ok(())
}

fn count_files(sources: &[PathBuf]) -> usize {
    fn walk(p: &Path, count: &mut usize) {
        if p.is_dir() {
            if let Ok(entries) = fs::read_dir(p) {
                for entry in entries.flatten() {
                    walk(&entry.path(), count);
                }
            }
        } else {
            *count += 1;
        }
    }
    let mut count = 0;
    for s in sources {
        walk(s, &mut count);
    }
    count.max(1)
}

fn add_to_zip(
    file: &Path,
    entry_path: &str,
    zip: &mut ZipWriter<fs::File>,
    options: &SimpleFileOptions,
    processed: &mut usize,
    total: usize,
    on_progress: &mut Option<&mut ProgressCallback>,
) -> io::Result<()> {
    if file.is_dir() {
        let entries = fs::read_dir(file)?;
        for entry in entries {
            let entry = entry?;
            let child_name = entry.file_name();
            let child_name_str = child_name.to_string_lossy();
            let child_entry_path = format!("{entry_path}/{child_name_str}");
            add_to_zip(&entry.path(), &child_entry_path, zip, options, processed, total, on_progress)?;
        }
    } else {
        zip.start_file(entry_path, *options).map_err(to_io_err)?;
        let mut input = fs::File::open(file)?;
        // Streams directly rather than reading the whole file into memory
        // first — matches the original's `file.inputStream().use {
        // it.copyTo(zos) }`, which also streams. An earlier draft of this
        // function buffered the entire file into a Vec<u8> before writing,
        // which would hold large files fully in memory unnecessarily.
        io::copy(&mut input, zip)?;
        *processed += 1;
        if let Some(cb) = on_progress.as_deref_mut() {
            cb(*processed, total);
        }
    }
    Ok(())
}

fn to_io_err(e: zip::result::ZipError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e.to_string())
}

/// Resolves ".."/"." path components purely lexically — no filesystem
/// access, so it works even for paths that don't exist yet (unlike
/// `Path::canonicalize`, which requires the path to exist and is what the
/// zip-slip check in extract_zip needs to run BEFORE creating anything on
/// disk). This mirrors what `File.canonicalPath` effectively achieves for
/// path-traversal purposes in the original Kotlin, without requiring the
/// path to already exist.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                // Pop the last pushed component, same as ".." collapsing a
                // preceding real directory — but never pop past the root,
                // which is exactly the case a zip-slip attack relies on
                // (enough ".." entries to escape the destination root).
                result.pop();
            }
            std::path::Component::CurDir => {
                // No-op, same as "." in a real path.
            }
            other => {
                result.push(other.as_os_str());
            }
        }
    }
    result
}
