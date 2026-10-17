//! Shared-bytes pass: texture and sound files with identical content.
//!
//! INFORMATION ONLY, and never merged automatically. Villager lesson: six
//! texture shortnames pointing at identical bytes are six override points — a
//! resource pack may replace one and not the others. Equal bytes say "this
//! costs space twice", not "these are the same thing". The human decides.
//!
//! Identity is proved, not estimated: files are grouped by size, then by hash,
//! and every member is compared byte for byte with the group's first file.

use crate::filesystem::{all_files, is_asset, read_limited, MAX_ASSET_BYTES};
use crate::manifest::{Pack, PackKind};
use crate::par::par_map;
use std::collections::HashMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// Size of ONE copy.
    pub bytes: u64,
    /// `pack/relative/path` of every copy, sorted.
    pub files: Vec<String>,
}

impl DuplicateGroup {
    /// Space the extra copies take.
    pub fn wasted(&self) -> u64 {
        self.bytes * (self.files.len() as u64 - 1)
    }
}

pub fn run(packs: &[Pack]) -> Vec<DuplicateGroup> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for p in packs
        .iter()
        .filter(|p| p.manifest.kind == PackKind::Resources)
    {
        for dir in ["textures", "sounds"] {
            for f in all_files(&p.root.join(dir)) {
                let Ok(rel) = f.strip_prefix(&p.root) else {
                    continue;
                };
                let rel = rel.to_string_lossy().replace('\\', "/");
                if is_asset(&rel.to_ascii_lowercase()) {
                    files.push((format!("{}/{rel}", p.folder), f));
                }
            }
        }
    }

    // 1. sizes, on every core; empty files are placeholders, not content
    let sizes = par_map(&files, |(_, f)| {
        fs::metadata(f).map(|m| m.len()).unwrap_or(0)
    });
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, &size) in sizes.iter().enumerate() {
        if size > 0 {
            by_size.entry(size).or_default().push(i);
        }
    }

    // 2. hashes, only for files that share their size with another
    let candidates: Vec<usize> = by_size
        .values()
        .filter(|v| v.len() > 1)
        .flatten()
        .copied()
        .collect();
    let hashes = par_map(&candidates, |&i| {
        let bytes = read_limited(&files[i].1, MAX_ASSET_BYTES).ok()?;
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        Some(h.finish())
    });
    let mut by_hash: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
    for (&i, h) in candidates.iter().zip(hashes) {
        if let Some(h) = h {
            by_hash.entry((sizes[i], h)).or_default().push(i);
        }
    }

    // 3. proof: every member compared byte for byte with the group's first. A
    // hash collision becomes a group of its own instead of a false duplicate.
    let suspects: Vec<(u64, Vec<usize>)> = by_hash
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|((size, _), v)| (size, v))
        .collect();
    let proved: Vec<Vec<DuplicateGroup>> = par_map(&suspects, |(size, members)| {
        let mut buckets: Vec<(Vec<u8>, Vec<String>)> = Vec::new();
        for &i in members {
            let Ok(bytes) = read_limited(&files[i].1, MAX_ASSET_BYTES) else {
                continue;
            };
            match buckets.iter_mut().find(|(first, _)| *first == bytes) {
                Some((_, names)) => names.push(files[i].0.clone()),
                None => buckets.push((bytes, vec![files[i].0.clone()])),
            }
        }
        buckets
            .into_iter()
            .filter(|(_, names)| names.len() > 1)
            .map(|(_, mut names)| {
                names.sort();
                DuplicateGroup {
                    bytes: *size,
                    files: names,
                }
            })
            .collect()
    });
    let mut groups: Vec<DuplicateGroup> = proved.into_iter().flatten().collect();
    groups.sort_by(|a, b| {
        b.wasted()
            .cmp(&a.wasted())
            .then_with(|| a.files.cmp(&b.files))
    });
    groups
}
