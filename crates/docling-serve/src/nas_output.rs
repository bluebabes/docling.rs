//! Optional Markdown persistence for NAS deployments.
//!
//! Enabled by DOCLING_RS_OUTPUT_DIR (set to /output in Dockerfile.nas).
//! Files are written only after a successful conversion; each name is unique.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// Persist Markdown without affecting the API result if the mount is missing
/// or read-only. A warning in Docker logs makes the failure visible.
pub(super) fn save_markdown(source_name: &str, markdown: &[u8]) {
    let Some(dir) = std::env::var_os("DOCLING_RS_OUTPUT_DIR") else {
        return;
    };
    if dir.is_empty() {
        return;
    }
    match write_markdown(Path::new(&dir), source_name, markdown) {
        Ok(path) => eprintln!("docling-serve: saved Markdown to {}", path.display()),
        Err(e) => eprintln!(
            "docling-serve: could not save Markdown for {:?} in {}: {e}",
            source_name,
            Path::new(&dir).display()
        ),
    }
}

/// Keep only the uploaded name's final component. The result is always a
/// basename; client-controlled slashes, backslashes and control characters
/// cannot steer writes outside the operator-configured output directory.
fn safe_stem(source_name: &str) -> String {
    let basename = source_name.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = Path::new(basename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let cleaned: String = stem
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '.'))
        .take(64)
        .collect();
    let cleaned = cleaned.trim_matches(|c| c == '.' || c == ' ');
    if cleaned.is_empty() {
        "document".to_owned()
    } else {
        cleaned.to_owned()
    }
}

fn write_markdown(dir: &Path, source_name: &str, markdown: &[u8]) -> io::Result<PathBuf> {
    if !dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("output directory {} is missing", dir.display()),
        ));
    }
    let stem = safe_stem(source_name);
    for n in 1..=10_000 {
        let filename = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}-{n}.md")
        };
        let path = dir.join(filename);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        // The NAS host user must be able to read files written by container
        // root, even when the host directory has a different owner.
        options.mode(0o644);
        match options.open(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(markdown).and_then(|()| file.sync_all()) {
                    let _ = fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many files with the same name",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_upload_paths_and_preserves_chinese_names() {
        assert_eq!(safe_stem("/tmp/小学词汇电子思维导图.pdf"), "小学词汇电子思维导图");
        assert_eq!(safe_stem("C:\\fake\\demo.pdf"), "demo");
        assert_eq!(safe_stem("../../.."), "document");
        assert_eq!(safe_stem("../../../../foo.pdf"), "foo");
    }

    #[test]
    fn preserves_existing_results_when_upload_name_repeats() {
        let dir = std::env::temp_dir().join(format!(
            "docling-nas-output-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let first = write_markdown(&dir, "报告.pdf", b"first").unwrap();
        let second = write_markdown(&dir, "报告.pdf", b"second").unwrap();
        assert_eq!(first.file_name().unwrap(), "报告.md");
        assert_eq!(second.file_name().unwrap(), "报告-2.md");
        assert_eq!(fs::read(first).unwrap(), b"first");
        assert_eq!(fs::read(second).unwrap(), b"second");
        fs::remove_dir_all(dir).unwrap();
    }
}
