//! Behavior packs: what their files name, resolved against what exists.
//!
//! The same rule as everywhere: a reference is broken only when it is PROVEN.
//!
//! - **Inside one file** (an entity's component groups and events, a recipe's
//!   keys), the file alone decides.
//! - **Files** (loot and trade tables, functions, a script's imports) must be in
//!   the packs or, for tables, in the game. The game has no functions and no
//!   behavior animations of its own, so those are decided without it. Tables need
//!   the game's catalog: without it they stay unverified.
//! - **Identifiers** (items, blocks, entities, features) in the `minecraft:`
//!   namespace are never judged: the game ships no list of its items, and a list
//!   of what its files happen to name proves nothing. Another namespace is judged
//!   only when a pack here defines things in it: a name nobody here owns may well
//!   come from a pack that is not here, so it stays unverified.
//!
//! A pack that depends on a pack that cannot be found leaves files and identifiers
//! unverified: what is missing may be in that pack.
//!
//! **Broken or dead.** What does not exist is not always a break. The game's own
//! entities name component groups, events and loot tables that do not exist, and
//! run: that part does nothing. So those are `Issue::Dead`, reported and not
//! counted as broken, and so is an identifier, whose absence holds only for the
//! packs that are here. A test runs this over the game's files to keep it honest.

mod commands;
mod scripts;

use crate::filesystem::{all_files, read_limited, MAX_TEXT_BYTES};
use crate::json::{self, Json};
use crate::manifest::{read_manifest, world_packs, Dependency, Pack, PackKind};
use crate::par::par_map;
use crate::report::{AnalysisReport, Issue};
use crate::vanilla::VanillaCatalog;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The folders a behavior pack alone has. `items` and `blocks` are not here: a
/// resource pack can have them too.
const BEHAVIOR_ONLY: [&str; 7] = [
    "entities",
    "functions",
    "loot_tables",
    "recipes",
    "spawn_rules",
    "trading",
    "feature_rules",
];

/// Is this folder a behavior pack: by its manifest or, without one, by its folders?
pub fn is_behavior_pack(root: &Path) -> bool {
    match read_manifest(root) {
        Some(m) => m.kind == PackKind::Data,
        None => {
            !root.join("manifest.json").is_file()
                && BEHAVIOR_ONLY.iter().any(|f| root.join(f).is_dir())
        }
    }
}

/// One text file of a pack: its path in the pack with `/`, and what it says.
pub struct PackFile {
    pub rel: String,
    pub text: Result<String, String>,
}

/// What one pack brings: the files that are read, and every path it has.
pub struct LoadedPack {
    pub name: String,
    pub files: Vec<PackFile>,
    pub paths: Vec<String>,
    /// `manifest.json`'s script entry and the modules it may import
    pub script_entries: Vec<String>,
    pub script_modules: HashSet<String>,
}

fn wanted(rel: &str) -> bool {
    let (folder, _) = rel.split_once('/').unwrap_or(("", ""));
    let ext = rel.rsplit('.').next().unwrap_or("");
    match folder {
        "entities"
        | "items"
        | "blocks"
        | "recipes"
        | "loot_tables"
        | "trading"
        | "spawn_rules"
        | "feature_rules"
        | "features"
        | "animations"
        | "animation_controllers" => ext.eq_ignore_ascii_case("json"),
        "functions" => ext.eq_ignore_ascii_case("mcfunction") || rel == "functions/tick.json",
        "scripts" => ext.eq_ignore_ascii_case("js"),
        _ => false,
    }
}

/// `subpacks/high/functions/a.mcfunction` is `functions/a.mcfunction` when the game
/// reads that subpack. All of them are kept: which one a player chose is unknown.
fn logical(rel: &str) -> &str {
    rel.strip_prefix("subpacks/")
        .and_then(|r| r.split_once('/'))
        .map_or(rel, |(_, inner)| inner)
}

pub fn load(root: &Path) -> LoadedPack {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut paths = Vec::new();
    let mut to_read = Vec::new();
    for f in all_files(root) {
        let Ok(rel) = f.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if wanted(logical(&rel)) {
            to_read.push((rel.clone(), f));
        }
        paths.push(rel);
    }
    let files = par_map(&to_read, |(rel, path)| PackFile {
        rel: rel.clone(),
        text: read_limited(path, MAX_TEXT_BYTES)
            .map_err(|e| e.to_string())
            .and_then(|b| String::from_utf8(b).map_err(|_| "not UTF-8 text".to_string())),
    });
    let mut script_entries = Vec::new();
    if let Ok(src) = crate::filesystem::read_text(&root.join("manifest.json")) {
        if let Ok(m) = json::parse(&src) {
            for module in m
                .get("modules")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
            {
                let is_script = matches!(
                    module.get("type").and_then(Json::as_str),
                    Some("script" | "javascript")
                );
                if let (true, Some(entry)) = (is_script, module.get("entry").and_then(Json::as_str))
                {
                    script_entries.push(entry.replace('\\', "/"));
                }
            }
        }
    }
    let script_modules = read_manifest(root)
        .map(|m| {
            m.dependencies
                .into_iter()
                .filter_map(|d| match d {
                    Dependency::Module { name, .. } => Some(name),
                    Dependency::Pack { .. } => None,
                })
                .collect()
        })
        .unwrap_or_default();
    LoadedPack {
        name,
        files,
        paths,
        script_entries,
        script_modules,
    }
}

/// `Json` a file parses to, with `Err` as what the report says about it.
fn parsed(file: &PackFile) -> Result<Json, String> {
    let text = file.text.as_ref().map_err(String::clone)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    json::parse(text).map_err(|e| format!("{e:?}"))
}

/// Everything a reference can land on, over all the packs being analysed.
#[derive(Default)]
pub struct Scope {
    /// Exact paths, and lowercased path -> the path on disk.
    files: HashSet<String>,
    lower_files: HashMap<String, String>,
    entities: HashSet<String>,
    items: HashSet<String>,
    blocks: HashSet<String>,
    features: HashSet<String>,
    animations: HashSet<String>,
    controllers: HashSet<String>,
    /// Namespaces some pack here defines identifiers in.
    namespaces: HashSet<String>,
    /// A pack depends on a pack that was not found.
    pub incomplete: bool,
}

/// The identifier of a definition: `{"minecraft:item": {"description": {"identifier"}}}`.
fn defined_id<'a>(root: &'a Json, top: &str) -> Option<&'a str> {
    root.get(top)?
        .get("description")?
        .get("identifier")?
        .as_str()
}

impl Scope {
    fn define(set: &mut HashSet<String>, namespaces: &mut HashSet<String>, id: &str) {
        let id = id.to_ascii_lowercase();
        if let Some((ns, _)) = id.split_once(':') {
            namespaces.insert(ns.to_string());
        }
        set.insert(id);
    }

    pub fn of(packs: &[LoadedPack], incomplete: bool) -> Self {
        let mut s = Scope {
            incomplete,
            ..Default::default()
        };
        for pack in packs {
            for path in &pack.paths {
                let path = logical(path);
                s.files.insert(path.to_string());
                s.lower_files
                    .insert(path.to_ascii_lowercase(), path.to_string());
            }
            for file in &pack.files {
                let rel = logical(&file.rel);
                let folder = rel.split('/').next().unwrap_or("");
                if rel.ends_with(".mcfunction") || rel.ends_with(".js") {
                    continue;
                }
                let Ok(root) = parsed(file) else { continue };
                match folder {
                    "entities" => {
                        if let Some(id) = defined_id(&root, "minecraft:entity") {
                            Self::define(&mut s.entities, &mut s.namespaces, id);
                        }
                    }
                    "items" => {
                        if let Some(id) = defined_id(&root, "minecraft:item") {
                            Self::define(&mut s.items, &mut s.namespaces, id);
                        }
                    }
                    "blocks" => {
                        if let Some(id) = defined_id(&root, "minecraft:block") {
                            Self::define(&mut s.blocks, &mut s.namespaces, id);
                        }
                    }
                    "features" => {
                        for value in root.as_object().iter().flat_map(|o| o.values()) {
                            if let Some(id) = value
                                .get("description")
                                .and_then(|d| d.get("identifier"))
                                .and_then(Json::as_str)
                            {
                                Self::define(&mut s.features, &mut s.namespaces, id);
                            }
                        }
                    }
                    "animations" => {
                        if let Some(o) = root.get("animations").and_then(Json::as_object) {
                            s.animations
                                .extend(o.keys().map(|k| k.to_ascii_lowercase()));
                        }
                    }
                    "animation_controllers" => {
                        if let Some(o) = root.get("animation_controllers").and_then(Json::as_object)
                        {
                            s.controllers
                                .extend(o.keys().map(|k| k.to_ascii_lowercase()));
                        }
                    }
                    _ => {}
                }
            }
        }
        s
    }
}

/// What one file's check found.
#[derive(Default)]
struct Found {
    refs: usize,
    resolved: usize,
    external: usize,
    unverified: usize,
    missing: usize,
    dead: usize,
    case: usize,
    unreadable: usize,
    issues: Vec<Issue>,
    seen: BTreeSet<(&'static str, String)>,
}

/// What the game carries on from: see the top of this file.
fn is_dead_kind(kind: &str) -> bool {
    matches!(
        kind,
        "component group"
            | "event"
            | "loot table"
            | "trade table"
            | "item"
            | "block"
            | "entity"
            | "feature"
    )
}

/// One file being checked, and what it can reach.
struct Check<'a> {
    owner: &'a str,
    scope: &'a Scope,
    catalog: &'a VanillaCatalog,
    out: Found,
}

impl Check<'_> {
    fn missing(&mut self, kind: &'static str, reference: &str) {
        // a name written ten times is one thing to fix
        if !self.out.seen.insert((kind, reference.to_string())) {
            return;
        }
        let (owner, reference) = (self.owner.to_string(), reference.to_string());
        if is_dead_kind(kind) {
            self.out.dead += 1;
            self.out.issues.push(Issue::Dead {
                kind,
                owner,
                reference,
            });
        } else {
            self.out.missing += 1;
            self.out.issues.push(Issue::Missing {
                kind,
                owner,
                reference,
            });
        }
    }

    /// Something the file itself defines, or does not.
    fn inside(&mut self, kind: &'static str, reference: &str, found: bool) {
        self.out.refs += 1;
        if found {
            self.out.resolved += 1;
        } else {
            self.missing(kind, reference);
        }
    }

    /// A path in the packs, or (`game`) in the game's tables.
    fn file(&mut self, kind: &'static str, raw: &str, game: bool, extra_ext: Option<&str>) {
        let path = raw.trim().replace('\\', "/");
        if path.is_empty() {
            return;
        }
        self.out.refs += 1;
        let with_ext = extra_ext.map(|e| format!("{path}{e}"));
        let candidates: Vec<&str> = std::iter::once(path.as_str())
            .chain(with_ext.as_deref())
            .collect();
        if candidates.iter().any(|c| self.scope.files.contains(*c)) {
            self.out.resolved += 1;
            return;
        }
        if game
            && candidates
                .iter()
                .any(|c| self.catalog.has_behavior_file(&c.to_ascii_lowercase()))
        {
            self.out.external += 1;
            return;
        }
        let on_disk = candidates
            .iter()
            .find_map(|c| self.scope.lower_files.get(&c.to_ascii_lowercase()));
        if let Some(on_disk) = on_disk {
            self.out.case += 1;
            self.out.issues.push(Issue::CaseMismatch {
                owner: self.owner.to_string(),
                reference: path.clone(),
                on_disk: on_disk.clone(),
            });
            return;
        }
        if self.scope.incomplete || (game && self.catalog.is_empty()) {
            self.out.unverified += 1;
            return;
        }
        self.missing(kind, &path);
    }

    /// `ns:name`, with what may follow it (`ns:name<event>`, `ns:name:3`).
    fn id(&mut self, kind: &'static str, raw: &str) {
        let id = raw.trim();
        // a value only known when it runs: nothing to say about it
        if id.is_empty() || id.contains(|c: char| c.is_whitespace() || "$@{}[]\"".contains(c)) {
            return;
        }
        let id = id.split('<').next().unwrap_or(id).to_ascii_lowercase();
        let mut parts = id.splitn(3, ':');
        let (ns, name) = (parts.next().unwrap_or(""), parts.next());
        let Some(name) = name else {
            // no namespace: the game's own
            self.out.refs += 1;
            self.out.external += 1;
            return;
        };
        self.out.refs += 1;
        if ns == "minecraft" {
            self.out.external += 1;
            return;
        }
        let bare = format!("{ns}:{name}");
        let s = self.scope;
        let found = match kind {
            "entity" => s.entities.contains(&bare),
            "block" => s.blocks.contains(&bare),
            "feature" => s.features.contains(&bare),
            _ => {
                s.items.contains(&bare)
                    || s.blocks.contains(&bare)
                    || bare
                        .strip_suffix("_spawn_egg")
                        .is_some_and(|e| s.entities.contains(e))
            }
        };
        if found {
            self.out.resolved += 1;
        } else if s.incomplete || !s.namespaces.contains(ns) {
            self.out.unverified += 1;
        } else {
            self.missing(kind, &bare);
        }
    }

    /// An item as recipes and trades write it: text, or `{"item": ...}`.
    fn item_value(&mut self, v: &Json) {
        match v {
            Json::Str(s) => self.id("item", s),
            Json::Obj(_) => {
                if let Some(s) = v.get("item").and_then(Json::as_str) {
                    self.id("item", s);
                }
            }
            Json::Arr(list) => {
                for x in list {
                    self.item_value(x);
                }
            }
            _ => {}
        }
    }

    fn command(&mut self, line: &str) {
        for c in commands::references(line) {
            match c {
                commands::Ref::Function(path) => self.file(
                    "function",
                    &format!("functions/{path}.mcfunction"),
                    false,
                    None,
                ),
                commands::Ref::Item(id) => self.id("item", &id),
                commands::Ref::Entity(id) => self.id("entity", &id),
            }
        }
    }

    /// Paths to tables, wherever a definition names them: no key lists to go stale.
    fn tables_in(&mut self, v: &Json) {
        match v {
            Json::Str(s) => {
                let lower = s.trim().replace('\\', "/").to_ascii_lowercase();
                if lower.starts_with("loot_tables/") {
                    self.file("loot table", s, true, Some(".json"));
                } else if lower.starts_with("trading/") {
                    self.file("trade table", s, true, Some(".json"));
                }
            }
            Json::Arr(list) => list.iter().for_each(|x| self.tables_in(x)),
            Json::Obj(o) => o.values().for_each(|x| self.tables_in(x)),
            _ => {}
        }
    }

    fn entity(&mut self, root: &Json) {
        let Some(entity) = root.get("minecraft:entity") else {
            return;
        };
        self.tables_in(entity);

        let name_set = |key: &str| -> HashSet<String> {
            entity
                .get(key)
                .and_then(Json::as_object)
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default()
        };
        let groups = name_set("component_groups");
        let events = name_set("events");

        if let Some(all) = entity.get("events").and_then(Json::as_object) {
            for event in all.values() {
                self.event_body(event, &groups, &events);
            }
        }
        if let Some(c) = entity.get("components") {
            self.triggers(c, &events);
            self.entity_refs(c);
        }
        if let Some(all) = entity.get("component_groups").and_then(Json::as_object) {
            for g in all.values() {
                self.triggers(g, &events);
                self.entity_refs(g);
            }
        }
        self.server_animations(entity);
    }

    /// `add` and `remove` name groups; `trigger` names events; `queue_command` runs.
    fn event_body(&mut self, v: &Json, groups: &HashSet<String>, events: &HashSet<String>) {
        match v {
            Json::Arr(list) => list.iter().for_each(|x| self.event_body(x, groups, events)),
            Json::Obj(o) => {
                for (key, value) in o.iter() {
                    match key.as_str() {
                        "add" | "remove" => {
                            let named = value.get("component_groups").and_then(Json::as_array);
                            for g in named.into_iter().flatten().filter_map(Json::as_str) {
                                self.inside("component group", g, groups.contains(g));
                            }
                        }
                        "trigger" => self.trigger_value(value, events),
                        "queue_command" => match value.get("command") {
                            Some(Json::Str(s)) => self.command(s),
                            Some(Json::Arr(list)) => {
                                for s in list.iter().filter_map(Json::as_str) {
                                    self.command(s);
                                }
                            }
                            _ => {}
                        },
                        // a filter's values are not references
                        "filters" => {}
                        _ => self.event_body(value, groups, events),
                    }
                }
            }
            _ => {}
        }
    }

    /// An event this entity must define: on itself, with a name of its own. The
    /// game's own events (`minecraft:entity_spawned`) it may never define at all.
    fn own_event(&mut self, name: &str, target: Option<&str>, events: &HashSet<String>) {
        if name.is_empty() || name.starts_with("minecraft:") {
            return;
        }
        if target.is_none_or(|t| t == "self") {
            self.inside("event", name, events.contains(name));
        }
    }

    fn trigger_value(&mut self, v: &Json, events: &HashSet<String>) {
        match v {
            Json::Str(name) => self.own_event(name, None, events),
            Json::Obj(_) => {
                if let Some(name) = v.get("event").and_then(Json::as_str) {
                    let target = v.get("target").and_then(Json::as_str);
                    self.own_event(name, target, events);
                }
            }
            _ => {}
        }
    }

    /// `{"event": ..., "target": ...}` anywhere in the components. Not in
    /// `send_event`: it goes to other entities, and `target` does not say so.
    fn triggers(&mut self, v: &Json, events: &HashSet<String>) {
        match v {
            Json::Arr(list) => list.iter().for_each(|x| self.triggers(x, events)),
            Json::Obj(o) => {
                if let Some(name) = o.get("event").and_then(Json::as_str) {
                    let target = o.get("target").and_then(Json::as_str);
                    self.own_event(name, target, events);
                }
                for (key, value) in o.iter() {
                    if key != "minecraft:behavior.send_event" && key != "filters" {
                        self.triggers(value, events);
                    }
                }
            }
            _ => {}
        }
    }

    /// Other definitions the components name.
    fn entity_refs(&mut self, components: &Json) {
        if let Some(into) = components
            .get("minecraft:transformation")
            .and_then(|t| t.get("into"))
            .and_then(Json::as_str)
        {
            self.id("entity", into);
        }
        let spawned = components
            .get("minecraft:spawn_entity")
            .and_then(|s| s.get("entities"));
        let one;
        let list: &[Json] = match spawned {
            Some(Json::Arr(list)) => list,
            Some(single @ Json::Obj(_)) => {
                one = [single.clone()];
                &one
            }
            _ => &[],
        };
        for e in list {
            if let Some(id) = e.get("spawn_entity").and_then(Json::as_str) {
                self.id("entity", id);
            }
            if let Some(id) = e.get("spawn_item").and_then(Json::as_str) {
                self.id("item", id);
            }
        }
    }

    /// `description.animations` and `scripts.animate`: what the server runs.
    fn server_animations(&mut self, entity: &Json) {
        let Some(description) = entity.get("description") else {
            return;
        };
        let declared = description.get("animations").and_then(Json::as_object);
        for id in declared
            .iter()
            .flat_map(|o| o.values())
            .filter_map(Json::as_str)
        {
            let lower = id.to_ascii_lowercase();
            if lower.starts_with("controller.animation.") {
                let found = self.scope.controllers.contains(&lower);
                self.scoped("behavior controller", id, found);
            } else if lower.starts_with("animation.") {
                let found = self.scope.animations.contains(&lower);
                self.scoped("behavior animation", id, found);
            }
        }
        let animate = description
            .get("scripts")
            .and_then(|s| s.get("animate"))
            .and_then(Json::as_array);
        for entry in animate.into_iter().flatten() {
            let names: Vec<&str> = match entry {
                Json::Str(s) => vec![s.as_str()],
                Json::Obj(o) => o.keys().map(String::as_str).collect(),
                _ => Vec::new(),
            };
            for name in names {
                let found = declared.is_some_and(|d| d.contains_key(name));
                self.inside("animation name", name, found);
            }
        }
    }

    /// In the packs here or nowhere: the game has none of its own.
    fn scoped(&mut self, kind: &'static str, reference: &str, found: bool) {
        self.out.refs += 1;
        if found {
            self.out.resolved += 1;
        } else if self.scope.incomplete {
            self.out.unverified += 1;
        } else {
            self.missing(kind, reference);
        }
    }

    fn item(&mut self, root: &Json) {
        let Some(item) = root.get("minecraft:item") else {
            return;
        };
        self.tables_in(item);
        let Some(c) = item.get("components") else {
            return;
        };
        let text = |component: &str, key: &str| {
            c.get(component)
                .and_then(|x| x.get(key))
                .and_then(Json::as_str)
        };
        if let Some(id) = text("minecraft:block_placer", "block") {
            self.id("block", id);
        }
        if let Some(id) = text("minecraft:entity_placer", "entity") {
            self.id("entity", id);
        }
        if let Some(id) = text("minecraft:projectile", "projectile_entity") {
            self.id("entity", id);
        }
        let ammunition = c
            .get("minecraft:shooter")
            .and_then(|s| s.get("ammunition"))
            .and_then(Json::as_array);
        for a in ammunition.into_iter().flatten() {
            if let Some(id) = a.get("item").and_then(Json::as_str) {
                self.id("item", id);
            }
        }
        let repairs = c
            .get("minecraft:repairable")
            .and_then(|r| r.get("repair_items"))
            .and_then(Json::as_array);
        for r in repairs.into_iter().flatten() {
            let items = r.get("items").and_then(Json::as_array);
            for id in items.into_iter().flatten().filter_map(Json::as_str) {
                self.id("item", id);
            }
        }
    }

    fn block(&mut self, root: &Json) {
        if let Some(block) = root.get("minecraft:block") {
            self.tables_in(block);
        }
    }

    fn recipe(&mut self, root: &Json) {
        let Some(o) = root.as_object() else { return };
        for (kind, r) in o.iter() {
            if !kind.starts_with("minecraft:recipe_") {
                continue;
            }
            if let Some(pattern) = r.get("pattern").and_then(Json::as_array) {
                let key = r.get("key").and_then(Json::as_object);
                let symbols: BTreeSet<char> = pattern
                    .iter()
                    .filter_map(Json::as_str)
                    .flat_map(str::chars)
                    .filter(|c| *c != ' ')
                    .collect();
                for symbol in symbols {
                    let mut buffer = [0u8; 4];
                    let name: &str = symbol.encode_utf8(&mut buffer);
                    self.inside(
                        "recipe key",
                        name,
                        key.is_some_and(|k| k.contains_key(name)),
                    );
                }
                for v in key.iter().flat_map(|k| k.values()) {
                    self.item_value(v);
                }
            }
            for field in [
                "ingredients",
                "input",
                "output",
                "reagent",
                "result",
                "template",
                "base",
                "addition",
            ] {
                if let Some(v) = r.get(field) {
                    self.item_value(v);
                }
            }
        }
    }

    /// `entries`, nested as deep as the table goes.
    fn loot(&mut self, v: &Json) {
        match v {
            Json::Arr(list) => list.iter().for_each(|x| self.loot(x)),
            Json::Obj(o) => {
                let name = o.get("name").and_then(Json::as_str);
                match (o.get("type").and_then(Json::as_str), name) {
                    (Some("item"), Some(name)) => self.id("item", name),
                    (Some("loot_table"), Some(name)) => {
                        self.file("loot table", name, true, Some(".json"))
                    }
                    _ => {}
                }
                // a table's functions and conditions name no entries
                for key in ["pools", "entries"] {
                    if let Some(inner) = o.get(key) {
                        self.loot(inner);
                    }
                }
            }
            _ => {}
        }
    }

    fn trade(&mut self, v: &Json) {
        match v {
            Json::Arr(list) => list.iter().for_each(|x| self.trade(x)),
            Json::Obj(o) => {
                for (key, value) in o.iter() {
                    match (key.as_str(), value) {
                        ("item", Json::Str(s)) => self.id("item", s),
                        ("functions", _) => {}
                        _ => self.trade(value),
                    }
                }
            }
            _ => {}
        }
    }

    fn spawn_rule(&mut self, root: &Json) {
        if let Some(id) = defined_id(root, "minecraft:spawn_rules") {
            self.id("entity", id);
        }
    }

    fn feature_rule(&mut self, root: &Json) {
        let placed = root
            .get("minecraft:feature_rules")
            .and_then(|r| r.get("description"))
            .and_then(|d| d.get("places_feature"))
            .and_then(Json::as_str);
        if let Some(id) = placed {
            self.id("feature", id);
        }
    }

    /// The commands an animation or a controller runs: text that starts with `/`.
    fn slash_commands(&mut self, v: &Json) {
        match v {
            Json::Str(s) if s.trim_start().starts_with('/') => self.command(s),
            Json::Arr(list) => list.iter().for_each(|x| self.slash_commands(x)),
            Json::Obj(o) => o.values().for_each(|x| self.slash_commands(x)),
            _ => {}
        }
    }

    fn tick(&mut self, root: &Json) {
        let values = root.get("values").and_then(Json::as_array);
        for f in values.into_iter().flatten().filter_map(Json::as_str) {
            self.file(
                "function",
                &format!("functions/{}.mcfunction", f.trim().trim_start_matches('/')),
                false,
                None,
            );
        }
    }
}

/// Checks one file. `text` files are not JSON; every other is.
fn check_file(
    file: &PackFile,
    pack: &LoadedPack,
    scope: &Scope,
    catalog: &VanillaCatalog,
) -> Found {
    let mut check = Check {
        owner: &file.rel,
        scope,
        catalog,
        out: Found::default(),
    };
    let rel = logical(&file.rel);
    let folder = rel.split('/').next().unwrap_or("");

    if rel.ends_with(".mcfunction") || rel.ends_with(".js") {
        match &file.text {
            Ok(text) if folder == "functions" => {
                for line in text.lines() {
                    check.command(line);
                }
            }
            Ok(text) => check_script(&mut check, rel, text, pack),
            Err(why) => unreadable(&mut check, why),
        }
        return check.out;
    }
    match parsed(file) {
        Err(why) => unreadable(&mut check, &why),
        Ok(root) => match folder {
            "entities" => check.entity(&root),
            "items" => check.item(&root),
            "blocks" => check.block(&root),
            "recipes" => check.recipe(&root),
            "loot_tables" => check.loot(&root),
            "trading" => check.trade(&root),
            "spawn_rules" => check.spawn_rule(&root),
            "feature_rules" => check.feature_rule(&root),
            "animations" | "animation_controllers" => check.slash_commands(&root),
            "functions" => check.tick(&root),
            _ => {}
        },
    }
    check.out
}

fn unreadable(check: &mut Check, why: &str) {
    check.out.unreadable += 1;
    check.out.issues.push(Issue::Unreadable {
        file: check.owner.to_string(),
        why: why.to_string(),
    });
}

/// A script's imports: relative ones must be in this very pack, and the game's
/// modules must be asked for in its manifest.
fn check_script(check: &mut Check, rel: &str, text: &str, pack: &LoadedPack) {
    let dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
    for import in scripts::imports(text) {
        if import.starts_with("./") || import.starts_with("../") {
            let Some(target) = scripts::join(dir, &import) else {
                continue;
            };
            check.out.refs += 1;
            // the game adds the extension to `./utils/utils`
            let with_js = format!("{target}.js");
            let found = pack
                .paths
                .iter()
                .any(|p| logical(p) == target || logical(p) == with_js);
            if found {
                check.out.resolved += 1;
            } else if let Some(on_disk) = pack
                .paths
                .iter()
                .map(|p| logical(p))
                .find(|p| p.eq_ignore_ascii_case(&target) || p.eq_ignore_ascii_case(&with_js))
            {
                check.out.case += 1;
                check.out.issues.push(Issue::CaseMismatch {
                    owner: check.owner.to_string(),
                    reference: target.clone(),
                    on_disk: on_disk.to_string(),
                });
            } else {
                check.missing("script file", &target);
            }
        } else if import.starts_with("@minecraft/") {
            check.inside(
                "script module",
                &import,
                pack.script_modules.contains(&import),
            );
        }
    }
}

/// Checks one pack against the scope.
pub fn check_pack(pack: &LoadedPack, scope: &Scope, catalog: &VanillaCatalog) -> AnalysisReport {
    let mut report = AnalysisReport {
        behavior_pack: true,
        behavior_files: pack.files.len(),
        ..Default::default()
    };
    let mut merge = |f: Found| {
        report.behavior_refs += f.refs;
        report.behavior_resolved += f.resolved;
        report.behavior_external += f.external;
        report.behavior_unverified += f.unverified;
        report.behavior_missing += f.missing;
        report.behavior_dead += f.dead;
        report.case_mismatches += f.case;
        report.unreadable += f.unreadable;
        report.issues.extend(f.issues);
    };
    // the entry is what makes the scripts run at all
    let mut entries = Found::default();
    for entry in &pack.script_entries {
        entries.refs += 1;
        report_entry(entry, pack, &mut entries);
    }
    merge(entries);
    for found in par_map(&pack.files, |f| check_file(f, pack, scope, catalog)) {
        merge(found);
    }
    report
}

fn report_entry(entry: &str, pack: &LoadedPack, out: &mut Found) {
    if pack.paths.iter().any(|p| p == entry) {
        out.resolved += 1;
    } else if let Some(on_disk) = pack.paths.iter().find(|p| p.eq_ignore_ascii_case(entry)) {
        out.case += 1;
        out.issues.push(Issue::CaseMismatch {
            owner: "manifest.json".to_string(),
            reference: entry.to_string(),
            on_disk: on_disk.clone(),
        });
    } else {
        out.missing += 1;
        out.issues.push(Issue::Missing {
            kind: "script file",
            owner: "manifest.json".to_string(),
            reference: entry.to_string(),
        });
    }
}

/// Where the game keeps packs, around a pack that is in one of those folders.
fn packs_near(root: &Path) -> Vec<Pack> {
    let Some(parent) = root.parent() else {
        return Vec::new();
    };
    let mut dirs = vec![parent.to_path_buf()];
    if let Some(mojang) = parent.parent() {
        for sub in [
            "behavior_packs",
            "development_behavior_packs",
            "resource_packs",
            "development_resource_packs",
        ] {
            dirs.push(mojang.join(sub));
        }
    }
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if let Some(manifest) = read_manifest(&path) {
                let folder = e.file_name().to_string_lossy().to_string();
                out.push(Pack {
                    root: path,
                    folder,
                    manifest,
                });
            }
        }
    }
    out
}

/// The packs `first` needs, found nearby, and whether any is missing. A resource
/// pack it needs adds nothing a behavior pack can name.
fn with_dependencies(first: &Path) -> (Vec<PathBuf>, bool) {
    let mut roots = vec![first.to_path_buf()];
    let mut incomplete = false;
    let mut pending: Vec<String> = pack_dependencies(first);
    if pending.is_empty() {
        return (roots, false);
    }
    let near = packs_near(first);
    let mut done: HashSet<String> = HashSet::new();
    while let Some(uuid) = pending.pop() {
        if !done.insert(uuid.clone()) {
            continue;
        }
        match near.iter().find(|p| p.manifest.uuid == uuid) {
            Some(p) if p.manifest.kind == PackKind::Data => {
                if !roots.contains(&p.root) {
                    roots.push(p.root.clone());
                    pending.extend(pack_dependencies(&p.root));
                }
            }
            Some(_) => {}
            None => incomplete = true,
        }
    }
    (roots, incomplete)
}

fn pack_dependencies(root: &Path) -> Vec<String> {
    read_manifest(root)
        .map(|m| {
            m.dependencies
                .into_iter()
                .filter_map(|d| match d {
                    Dependency::Pack { uuid } => Some(uuid),
                    Dependency::Module { .. } => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One behavior pack, with the behavior packs it depends on in reach.
pub fn check_alone(root: &Path, catalog: &VanillaCatalog) -> AnalysisReport {
    let (roots, incomplete) = with_dependencies(root);
    let loaded: Vec<LoadedPack> = roots.iter().map(|r| load(r)).collect();
    let scope = Scope::of(&loaded, incomplete);
    check_pack(&loaded[0], &scope, catalog)
}

/// Every behavior pack a world uses, against all of them at once.
pub fn check_world(world: &Path, catalog: &VanillaCatalog) -> Vec<(String, AnalysisReport)> {
    let packs = world_packs(world);
    let present: HashSet<&str> = packs.iter().map(|p| p.manifest.uuid.as_str()).collect();
    let behavior: Vec<&Pack> = packs
        .iter()
        .filter(|p| p.manifest.kind == PackKind::Data)
        .collect();
    let incomplete = behavior.iter().any(|p| {
        p.manifest
            .dependencies
            .iter()
            .any(|d| matches!(d, Dependency::Pack { uuid } if !present.contains(uuid.as_str())))
    });
    let loaded: Vec<LoadedPack> = behavior.iter().map(|p| load(&p.root)).collect();
    let scope = Scope::of(&loaded, incomplete);
    behavior
        .iter()
        .zip(&loaded)
        .map(|(p, pack)| (p.folder.clone(), check_pack(pack, &scope, catalog)))
        .collect()
}
