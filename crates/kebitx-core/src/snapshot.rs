//! Read-once file contents for one analysis run.
//!
//! Several stages read the same files: an entity file is opened by the
//! reference check, the graphics check, both orphan passes and the
//! compatibility check. Opening a file is the expensive part on Windows (the
//! antivirus scans every open), so while a snapshot is ENABLED, the first read
//! of a path keeps its bytes and the next ones get them from memory.
//!
//! Off by default: the library and the tests always read the disk. The CLI
//! turns it on for one `analyze`, when nothing changes the files; the builder
//! never does, because its filters rewrite files between steps.
//!
//! `read` and `read_to_string` mirror `std::fs`: same results, same errors
//! (`read_to_string` refuses invalid UTF-8 just like the std one).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type Files = HashMap<PathBuf, Arc<[u8]>>;

static FILES: Mutex<Option<Files>> = Mutex::new(None);

/// Start keeping file contents (an empty snapshot).
pub fn enable() {
    if let Ok(mut f) = FILES.lock() {
        *f = Some(HashMap::new());
    }
}

/// Stop, and free everything kept.
pub fn disable() {
    if let Ok(mut f) = FILES.lock() {
        *f = None;
    }
}

/// `std::fs::read`, through the snapshot when it is enabled.
pub fn read(path: impl AsRef<Path>) -> io::Result<Arc<[u8]>> {
    let path = path.as_ref();
    if let Ok(guard) = FILES.lock() {
        match guard.as_ref() {
            None => {
                drop(guard);
                return crate::filesystem::read_limited(path, crate::filesystem::MAX_TEXT_BYTES)
                    .map(Arc::from);
            }
            Some(files) => {
                if let Some(bytes) = files.get(path) {
                    return Ok(bytes.clone());
                }
            }
        }
    }
    // read outside the lock: other threads keep going meanwhile
    let bytes: Arc<[u8]> = Arc::from(crate::filesystem::read_limited(
        path,
        crate::filesystem::MAX_TEXT_BYTES,
    )?);
    if let Ok(mut guard) = FILES.lock() {
        if let Some(files) = guard.as_mut() {
            files
                .entry(path.to_path_buf())
                .or_insert_with(|| bytes.clone());
        }
    }
    Ok(bytes)
}

/// `std::fs::read_to_string`, through the snapshot when it is enabled.
pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let bytes = read(path)?;
    String::from_utf8(bytes.to_vec()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_answers_as_std_and_reads_once_when_enabled() {
        let dir = std::env::temp_dir().join(format!("kebitx-snapshot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.json");
        std::fs::write(&f, "{\"ñ\": 1}").unwrap();
        let bad = dir.join("bad.txt");
        std::fs::write(&bad, [0xff, 0xfe]).unwrap();

        // disabled: a plain read, and invalid UTF-8 is an error as in std
        assert_eq!(read_to_string(&f).unwrap(), "{\"ñ\": 1}");
        assert!(read_to_string(&bad).is_err());
        assert!(read(dir.join("missing")).is_err());

        enable();
        assert_eq!(read_to_string(&f).unwrap(), "{\"ñ\": 1}");
        std::fs::write(&f, "changed").unwrap();
        assert_eq!(
            read_to_string(&f).unwrap(),
            "{\"ñ\": 1}",
            "served from the snapshot"
        );
        disable();
        assert_eq!(
            read_to_string(&f).unwrap(),
            "changed",
            "off again: the disk"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
