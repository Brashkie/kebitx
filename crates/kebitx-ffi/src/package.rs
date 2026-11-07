//! One file ready to open in Minecraft: a pack as `.mcpack`, a world as `.mcworld`.
//! Writes that file and nothing else, and never leaves half of one behind.

use crate::Lines;
use kebitx_build::zip::write_zip;
use kebitx_core::analyze::is_world;
use kebitx_core::filesystem::all_files;
use kebitx_core::i18n::{tf, tr};
use kebitx_core::json::Json;
use kebitx_core::manifest::{packs_in, world_packs};
use std::path::{Path, PathBuf};

/// What this folder packs into, if it packs at all: only what the game opens.
pub fn extension_for(root: &Path) -> Option<&'static str> {
    if is_world(root) && root.join("level.dat").is_file() {
        Some("mcworld")
    } else if root.join("manifest.json").is_file() {
        Some("mcpack")
    } else {
        None
    }
}

fn failed(root: &Path, why: String) -> String {
    Lines {
        kind: tr("Could not create the package").to_string(),
        broken: 0,
        summary: vec![why],
        problems: Vec::new(),
        notes: Vec::new(),
        extra: Vec::new(),
    }
    .json(root)
}

pub fn package_json(root: &Path, out: &Path) -> Option<String> {
    let Some(extension) = extension_for(root) else {
        return Some(failed(
            root,
            tr("Only a pack or a world can be packaged.").to_string(),
        ));
    };
    // compared whole: the output may not exist yet, its folder must
    let out_folder = out.parent().and_then(|p| p.canonicalize().ok());
    let inside = root
        .canonicalize()
        .ok()
        .zip(out_folder)
        .is_some_and(|(r, o)| o.starts_with(r));
    if inside {
        return Some(failed(
            root,
            tr("The package cannot go inside the folder it packs.").to_string(),
        ));
    }
    let mut entries: Vec<(String, PathBuf)> = all_files(root)
        .into_iter()
        .filter_map(|file| {
            let name = file.strip_prefix(root).ok()?;
            Some((name.to_string_lossy().replace('\\', "/"), file))
        })
        .collect();
    // the same folder, the same bytes
    entries.sort();
    let mut partial = out.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    if let Err(why) = write_zip(&partial, &entries)
        .and_then(|()| std::fs::rename(&partial, out).map_err(|e| e.to_string()))
    {
        let _ = std::fs::remove_file(&partial);
        return Some(failed(root, why));
    }
    let mut summary = vec![
        tf("File: {0}", &[&out.display()]),
        tf("Files: {0}", &[&entries.len()]),
    ];
    if extension == "mcworld" && world_packs(root).len() > packs_in(root).len() {
        summary.push(
            tr("The packs it uses from the game's folders are not inside: whoever opens it needs them too.")
                .to_string(),
        );
    }
    let lines = Lines {
        kind: tr("Package created").to_string(),
        broken: 0,
        summary,
        problems: Vec::new(),
        notes: Vec::new(),
        extra: vec![(
            "package_file".to_string(),
            Json::Str(out.to_string_lossy().to_string()),
        )],
    };
    Some(lines.json(root))
}
