use std::collections::{HashMap, HashSet};

/// Set of existing texture files (relative paths WITH extension).
///
/// Two lookups, on purpose:
///   - `contains` — exact, byte for byte. This is what Android and iOS do.
///   - `find_ignoring_case` — what Windows does. A reference that only resolves
///     this way WORKS on the author's PC and shows the pink missing-texture
///     checkerboard on a phone. Testing on Windows never reveals it.
#[derive(Debug, Default)]
pub struct TextureRegistry {
    files: HashSet<String>,
    by_lower: HashMap<String, String>,
}

impl TextureRegistry {
    pub fn from_files(files: impl IntoIterator<Item = String>) -> Self {
        let files: HashSet<String> = files.into_iter().collect();
        let by_lower = files
            .iter()
            .map(|f| (f.to_ascii_lowercase(), f.clone()))
            .collect();
        Self { files, by_lower }
    }
    pub fn contains(&self, path_with_ext: &str) -> bool {
        self.files.contains(path_with_ext)
    }
    /// The on-disk spelling of a path that only matches when case is ignored.
    pub fn find_ignoring_case(&self, path_with_ext: &str) -> Option<&str> {
        self.by_lower
            .get(&path_with_ext.to_ascii_lowercase())
            .map(String::as_str)
    }
    pub fn len(&self) -> usize {
        self.files.len()
    }
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}
