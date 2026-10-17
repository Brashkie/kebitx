//! What "better" means: the numbers a repair must not make worse.

use kebitx_core::analyze::{analyze_packs, is_resource_pack, resource_packs_in};
use kebitx_core::compat::check_world;
use kebitx_core::manifest::{pack_at, packs_in, parse_version, Pack};
use kebitx_core::passes::unused_asset;
use kebitx_core::vanilla::VanillaCatalog;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Score {
    /// Broken references (missing textures, models, animations, ...).
    pub broken: usize,
    /// References that resolve only ignoring case: broken on Android/iOS.
    pub case_mismatches: usize,
    pub compat_errors: usize,
    pub unused_assets: usize,
}

impl Score {
    /// Nothing worse, and at least one thing better.
    pub fn strictly_better_than(&self, before: &Score) -> bool {
        let pairs = [
            (self.broken, before.broken),
            (self.case_mismatches, before.case_mismatches),
            (self.compat_errors, before.compat_errors),
            (self.unused_assets, before.unused_assets),
        ];
        pairs.iter().all(|(a, b)| a <= b) && pairs.iter().any(|(a, b)| a < b)
    }
}

/// How `root` is analysed: one pack, or a world of packs. The same decision
/// the CLI makes.
pub(crate) struct Layout {
    pub single: bool,
    /// Resource-pack roots, in the order `analyze_packs` reports them.
    pub resource_roots: Vec<PathBuf>,
    /// Every pack (behavior packs too), for the orphan pass.
    pub packs: Vec<Pack>,
}

pub(crate) fn layout(root: &Path) -> Layout {
    let rps = resource_packs_in(root);
    let single = is_resource_pack(root) || rps.is_empty();
    if single {
        Layout {
            single,
            resource_roots: vec![root.to_path_buf()],
            packs: vec![pack_at(root)],
        }
    } else {
        Layout {
            single,
            resource_roots: rps,
            packs: packs_in(root),
        }
    }
}

pub fn score(root: &Path, catalog: &VanillaCatalog) -> Score {
    let l = layout(root);
    let reports = analyze_packs(&l.resource_roots, catalog);
    let game = if catalog.is_empty() {
        None
    } else {
        parse_version(&catalog.version)
    };
    Score {
        broken: reports.iter().map(|(_, r)| r.broken()).sum(),
        case_mismatches: reports.iter().map(|(_, r)| r.case_mismatches).sum(),
        compat_errors: if l.single {
            0
        } else {
            check_world(root, game).errors()
        },
        unused_assets: unused_asset::run(&l.packs, catalog).unused.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strictly_better_means_nothing_worse_and_something_better() {
        let before = Score {
            broken: 2,
            case_mismatches: 6,
            compat_errors: 0,
            unused_assets: 10,
        };
        let fixed = Score {
            case_mismatches: 0,
            ..before
        };
        assert!(fixed.strictly_better_than(&before));
        assert!(
            !before.strictly_better_than(&before),
            "unchanged is not better"
        );
        let traded = Score {
            case_mismatches: 0,
            broken: 3,
            ..before
        };
        assert!(
            !traded.strictly_better_than(&before),
            "fixing one thing by breaking another is not better"
        );
    }
}
