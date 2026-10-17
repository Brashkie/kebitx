//! The game's own files, by content, as the game stacks them.
//!
//! The catalog answers "does the game have it"; some questions need the
//! bytes (the pixels of a texture, the text of `en_US.lang`). Layers are
//! applied in version order — `vanilla`, then `vanilla_1.14`, ...,
//! `vanilla_1.26.50` — so a file a newer layer patches is the newer one,
//! as in the game. (Sorting the folder names as text would put `1.9` after
//! `1.21`.) A suffix that is not a version (`vanilla_base`) is a base: it goes
//! below everything. An EMPTY file is no content: bundles index some files
//! with zero bytes while the real text is loose (`texts/en_US.lang`).

use super::brarchive::{self, Brarchive};
use crate::par::par_map;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The layer folders of `data/resource_packs`: bases first, then `vanilla`,
/// then by version.
pub fn ordered_layers(install: &Path) -> Vec<PathBuf> {
    ordered_layers_in(install, PackKind::Resource)
}

/// Which of the game's pack stacks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    Resource,
    Behavior,
}

/// The layer folders of one stack, in the order the game applies them.
pub fn ordered_layers_in(install: &Path, kind: PackKind) -> Vec<PathBuf> {
    let folder = match kind {
        PackKind::Resource => "resource_packs",
        PackKind::Behavior => "behavior_packs",
    };
    let mut layers = match (
        super::stack_dir(install, folder),
        super::samples_layer(install, folder),
    ) {
        (Some(dir), _) => super::layers(&dir),
        (None, Some(single)) => vec![single],
        _ => Vec::new(),
    };
    let key = |p: &PathBuf| -> (u8, Vec<u32>) {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        match name.strip_prefix("vanilla_") {
            None => (1, Vec::new()),
            Some(v) => match v
                .split('.')
                .map(|x| x.parse::<u32>())
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(version) => (2, version),
                Err(_) => (0, Vec::new()),
            },
        }
    };
    layers.sort_by_key(key);
    layers
}

enum Source {
    Loose { layer: PathBuf, file: PathBuf },
    Bundle { layer: PathBuf, bundle: PathBuf },
}

/// `f` applied to every game file whose pack-relative, lowercased path
/// `wanted` accepts; for a path several layers have, the top layer's result.
/// Files are read and `f` runs on every core.
pub fn map_files<T: Send>(
    install: &Path,
    wanted: impl Fn(&str) -> bool + Sync,
    f: impl Fn(&str, &[u8]) -> Option<T> + Sync,
) -> HashMap<String, T> {
    map_files_in(install, PackKind::Resource, wanted, f)
}

/// `map_files` over either stack.
pub fn map_files_in<T: Send>(
    install: &Path,
    kind: PackKind,
    wanted: impl Fn(&str) -> bool + Sync,
    f: impl Fn(&str, &[u8]) -> Option<T> + Sync,
) -> HashMap<String, T> {
    let mut sources = Vec::new();
    for layer in ordered_layers_in(install, kind) {
        for file in super::catalog::loose_files(&layer) {
            sources.push(Source::Loose {
                layer: layer.clone(),
                file,
            });
        }
        for bundle in brarchive::bundles(&layer) {
            sources.push(Source::Bundle {
                layer: layer.clone(),
                bundle,
            });
        }
    }
    let parts = par_map(&sources, |s| {
        let mut out: Vec<(String, T)> = Vec::new();
        match s {
            Source::Loose { layer, file } => {
                let rel = file
                    .strip_prefix(layer)
                    .map(|r| r.to_string_lossy().replace('\\', "/").to_ascii_lowercase())
                    .unwrap_or_default();
                if wanted(&rel) {
                    if let Ok(bytes) =
                        crate::filesystem::read_limited(file, crate::filesystem::MAX_ASSET_BYTES)
                    {
                        if let Some(v) = (!bytes.is_empty()).then(|| f(&rel, &bytes)).flatten() {
                            out.push((rel, v));
                        }
                    }
                }
            }
            Source::Bundle { layer, bundle } => {
                let folder = brarchive::folder_of(layer, bundle);
                let Ok(archive) = Brarchive::open(bundle) else {
                    return out;
                };
                let full = |n: &str| format!("{folder}/{n}").to_ascii_lowercase();
                for (name, bytes) in archive.read_matching(|n| wanted(&full(n))) {
                    let rel = full(&name);
                    if let Some(v) = (!bytes.is_empty()).then(|| f(&rel, &bytes)).flatten() {
                        out.push((rel, v));
                    }
                }
            }
        }
        out
    });
    let mut map = HashMap::new();
    for part in parts {
        for (k, v) in part {
            map.insert(k, v); // later sources are higher layers
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_sort_by_version_not_text() {
        let tmp = std::env::temp_dir().join(format!("kebitx-layers-{}", std::process::id()));
        for l in [
            "vanilla_1.9",
            "vanilla",
            "vanilla_base",
            "vanilla_1.21.0",
            "vanilla_1.14",
        ] {
            std::fs::create_dir_all(tmp.join("data/resource_packs").join(l)).unwrap();
        }
        let names: Vec<String> = ordered_layers(&tmp)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(
            names,
            [
                "vanilla_base",
                "vanilla",
                "vanilla_1.9",
                "vanilla_1.14",
                "vanilla_1.21.0"
            ]
        );
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn a_server_and_mojangs_samples_are_sources_too() {
        let tmp = std::env::temp_dir().join(format!("kebitx-layouts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // a dedicated server: the stacks at its root
        let server = tmp.join("server");
        for l in ["vanilla", "vanilla_1.21.0"] {
            std::fs::create_dir_all(server.join("resource_packs").join(l)).unwrap();
        }
        // Mojang's samples: one layer per stack
        let samples = tmp.join("samples");
        std::fs::create_dir_all(samples.join("resource_pack")).unwrap();
        std::fs::create_dir_all(samples.join("behavior_pack")).unwrap();
        assert!(super::super::is_source(&server) && super::super::is_source(&samples));
        assert_eq!(ordered_layers(&server).len(), 2);
        assert_eq!(
            ordered_layers(&samples),
            vec![samples.join("resource_pack")]
        );
        assert_eq!(
            ordered_layers_in(&samples, PackKind::Behavior),
            vec![samples.join("behavior_pack")]
        );
        assert!(!super::super::is_source(&tmp));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod version_tests {
    #[test]
    fn a_gdk_install_says_its_version_in_its_config() {
        let tmp = std::env::temp_dir().join(format!("kebitx-gdk-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("Content")).unwrap();
        std::fs::write(
            tmp.join("Content/MicrosoftGame.Config"),
            "<Game configVersion=\"1\">\n  <Identity Name=\"Microsoft.MinecraftUWP\"\n    Publisher=\"CN=Microsoft\"\n    Version=\"1.26.5203.0\" />\n</Game>",
        )
        .unwrap();
        assert_eq!(super::super::game_version(&tmp.join("Content")), "1.26.52");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
