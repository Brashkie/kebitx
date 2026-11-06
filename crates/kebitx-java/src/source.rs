//! What is analysed: a folder, or a `.jar` / `.zip` read in place.
//!
//! An archive is never extracted: its entry names are only looked up, never
//! turned into paths, so a hostile name (`../x`) cannot reach the disk.

use kebitx_core::codec::zip::ZipArchive;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One file is at most this big (the game's own `.jar` entries are far smaller).
const READ_LIMIT: u64 = 64 * 1024 * 1024;

/// Bundled jars nest (a jar in a jar in a jar): this deep is plenty.
const MAX_NESTING: usize = 3;

pub enum Source {
    Dir(PathBuf),
    Zip(Box<ZipArchive>),
}

impl Source {
    /// A folder, or a `.jar` / `.zip` file.
    pub fn open(path: &Path) -> Result<Self, String> {
        if path.is_dir() {
            return Ok(Self::Dir(path.to_path_buf()));
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        match ext.as_deref() {
            Some("jar" | "zip") => ZipArchive::open(path).map(|z| Self::Zip(Box::new(z))),
            _ => Err(format!("not a folder, .jar or .zip: {}", path.display())),
        }
    }

    /// Every file, as a `/`-separated path relative to the root.
    pub fn files(&self) -> BTreeSet<String> {
        match self {
            Self::Dir(root) => kebitx_core::filesystem::all_files(root)
                .into_iter()
                .filter_map(|f| {
                    let rel = f.strip_prefix(root).ok()?;
                    Some(rel.to_string_lossy().replace('\\', "/"))
                })
                .collect(),
            Self::Zip(z) => z.names().filter(|n| !n.ends_with('/')).cloned().collect(),
        }
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        match self {
            Self::Dir(root) => {
                if name.split('/').any(|p| p == "..") {
                    return None;
                }
                kebitx_core::filesystem::read_limited(&root.join(name), READ_LIMIT).ok()
            }
            Self::Zip(z) => z.read(name, READ_LIMIT).ok(),
        }
    }

    /// Calls `f` with every jar bundled in this one (`META-INF/jars/`, what
    /// the loader loads as mods), and every jar bundled in those, read in
    /// memory. `files` is this source's own list.
    pub fn each_bundled(
        &self,
        files: &BTreeSet<String>,
        f: &mut dyn FnMut(&Source, &BTreeSet<String>),
    ) {
        self.walk_bundled(files, 0, f);
    }

    fn walk_bundled(
        &self,
        files: &BTreeSet<String>,
        depth: usize,
        f: &mut dyn FnMut(&Source, &BTreeSet<String>),
    ) {
        if depth >= MAX_NESTING {
            return;
        }
        for jar in files
            .iter()
            .filter(|n| n.starts_with("META-INF/jars/") && n.ends_with(".jar"))
        {
            let Some(inner) = self
                .read(jar)
                .and_then(|b| ZipArchive::from_bytes(b).ok())
                .map(|z| Self::Zip(Box::new(z)))
            else {
                continue;
            };
            let inner_files = inner.files();
            f(&inner, &inner_files);
            inner.walk_bundled(&inner_files, depth + 1, f);
        }
    }
}
