//! What a world has saved, as a report. Reads, and never writes.

use crate::{cancelled, Lines};
use kebitx_core::i18n::{tf, tr};
use kebitx_core::json::Json;
use kebitx_java::modset::Progress;
use kebitx_world::bedrock::{inspect, World};
use std::collections::BTreeMap;
use std::path::Path;

/// Does this folder keep a world's data, for the inspector to read?
pub fn has_database(root: &Path) -> bool {
    root.join("db").join("CURRENT").is_file()
}

fn size(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// One note per kind of thing, as `what\tname\thow many`, most first.
fn rows(what: &str, counts: &BTreeMap<String, u64>, notes: &mut Vec<String>) {
    let mut sorted: Vec<(&String, &u64)> = counts
        .iter()
        // air is what is not there: on every list it would be first
        .filter(|(name, _)| !matches!(name.as_str(), "minecraft:air" | "air"))
        .collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, count) in sorted {
        notes.push(format!("{what}\t{name}\t{count}"));
    }
}

/// What fits at a glance, and the rest for whoever looks further.
fn summary_of(w: &World) -> (Vec<String>, Vec<String>) {
    let mut out = Vec::new();
    let mut more = Vec::new();
    if let Some(version) = &w.version {
        out.push(tf("Version: {0}", &[version]));
    }
    for (number, dimension) in &w.dimensions {
        let name = match number {
            0 => tr("Overworld").to_string(),
            1 => tr("Nether").to_string(),
            2 => tr("The End").to_string(),
            n => tf("Dimension {0}", &[n]),
        };
        out.push(tf("{0}: {1} chunks", &[&name, &dimension.chunks]));
    }
    let air: u64 = ["minecraft:air", "air"]
        .iter()
        .filter_map(|k| w.blocks.get(*k))
        .sum();
    out.push(tf("Entities: {0}", &[&w.entities.values().sum::<u64>()]));
    out.push(tf("Players: {0}", &[&w.players]));
    more.push(tf("Blocks, without air: {0}", &[&(w.total_blocks() - air)]));
    more.push(tf(
        "Block entities: {0}",
        &[&w.block_entities.values().sum::<u64>()],
    ));
    more.push(tf(
        "Size: {0} on disk; unpacked, terrain {1}, entities {2}, block entities {3}, players {4}, other {5}",
        &[
            &size(w.disk_bytes),
            &size(w.sizes.terrain),
            &size(w.sizes.entities),
            &size(w.sizes.block_entities),
            &size(w.sizes.players),
            &size(w.sizes.other),
        ],
    ));
    if let Some(seed) = w.seed {
        more.push(tf("Seed: {0}", &[&seed]));
    }
    if w.old_subchunks > 0 {
        out.push(tf(
            "Old format, not counted: {0} chunk parts",
            &[&w.old_subchunks],
        ));
    }
    let damage = w.damage.tables + w.damage.blocks + w.damage.missing_tables;
    let unreadable = w.unreadable + damage as u64;
    if unreadable > 0 {
        out.push(tf("Not readable: {0}", &[&unreadable]));
    }
    (out, more)
}

pub fn world_report(root: &Path, go: Progress) -> Option<String> {
    let stage = tr("Reading the world");
    let found = inspect(root, &mut |done, total| go(stage, done, total));
    let world = match found {
        Ok(Some(world)) => world,
        Ok(None) => return Some(cancelled(root)),
        Err(why) => {
            let lines = Lines {
                kind: tr("Could not read the world").to_string(),
                broken: 0,
                summary: vec![why],
                problems: Vec::new(),
                notes: Vec::new(),
                extra: vec![("nothing".to_string(), Json::Bool(true))],
            };
            return Some(lines.json(root));
        }
    };
    let mut notes = Vec::new();
    rows("Block", &world.blocks, &mut notes);
    rows("Entity", &world.entities, &mut notes);
    rows("Block entity", &world.block_entities, &mut notes);
    let (summary, details) = summary_of(&world);
    let lines = Lines {
        kind: tf("World: {0}", &[&world.name]),
        broken: 0,
        summary,
        problems: Vec::new(),
        notes,
        extra: vec![
            ("world_view".to_string(), Json::Bool(true)),
            ("details".to_string(), Json::Str(details.join("\n"))),
        ],
    };
    Some(lines.json(root))
}
