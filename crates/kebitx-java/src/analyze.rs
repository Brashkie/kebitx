//! `kebitx analyze` for Java Edition: a resource pack or a mod, checked
//! against itself and the installed game's `.jar`.
//!
//! What is proven, and against what:
//!   - the `minecraft` namespace: the pack, then the game's `.jar` (without an
//!     installed game, nothing in it is claimed);
//!   - a mod's own namespace (its id): the mod, which ships all of it;
//!   - any other namespace (another mod's) cannot be proven from here: it is
//!     counted as unchecked, never reported.
//!
//! Checks: models whose `parent` does not exist; textures a model names that do
//! not exist (atlas sprites the game generates without a file — trims,
//! renamed and unstitched sprites — count as existing); blockstates and item
//! definitions naming models that do not exist; a texture variable (`#side`)
//! a used model's faces need and its chain never sets, which the game draws as
//! the missing texture; animated textures whose image does not divide into
//! frames, which the game refuses. Language files missing keys `en_us` has are
//! information: the game falls back to English.
//!
//! The test that keeps this honest: the game's own assets, analysed as a pack,
//! give no finding at all.

use crate::game::JavaJar;
use crate::source::Source;
use kebitx_core::json::{self, Json};
use std::collections::{BTreeMap, BTreeSet};

/// What the analysed thing is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A folder or archive with `pack.mcmeta`.
    /// The formats it declares, `(min, max)`.
    ResourcePack { formats: Option<(u32, u32)> },
    /// A mod: its loader, and the ids it declares (its own namespaces).
    Mod {
        loader: &'static str,
        ids: Vec<String>,
        /// The Minecraft versions it declares it is for, as written
        /// (`[1.21.1]`, `>=1.21 <1.22`).
        minecraft: Option<String>,
        /// The ids of the mods and libraries it declares it needs.
        depends: Vec<String>,
    },
    /// Assets with no pack metadata (the game's own `.jar`).
    Assets,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Finding {
    /// A blockstate, item definition or model names a model that does not exist.
    MissingModel { file: String, model: String },
    /// A model names a texture that does not exist.
    MissingTexture { file: String, texture: String },
    /// A used model's face asks for `#variable` and its chain never sets it:
    /// the game draws the missing texture.
    UnboundTexture {
        model: String,
        variable: String,
        used_by: String,
    },
    /// An animated texture whose image does not divide into its frames.
    BadAnimation {
        texture: String,
        size: (u32, u32),
        frame: (u32, u32),
    },
    /// A texture named `.png` that is not a PNG: the game checks the
    /// signature (`PngInfo.validateHeader`, "Bad PNG Signature") and shows
    /// the missing texture.
    NotPng { texture: String },
    /// A file that could not be read or parsed.
    Unreadable { file: String, why: String },
    /// Information: keys `en_us` has and this language lacks (English shows).
    MissingLangKeys {
        file: String,
        missing: usize,
        of: usize,
    },
    /// Information: a replacement whose proportions differ from the game's
    /// texture at the same path (neither animated). A uniform HD scale keeps
    /// them; an entity texture drawn on another layout would not fit its model.
    Proportions {
        texture: String,
        size: (u32, u32),
        game: (u32, u32),
    },
}

impl Finding {
    /// Information only: does not fail a run.
    pub fn is_info(&self) -> bool {
        matches!(
            self,
            Self::MissingLangKeys { .. } | Self::Proportions { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaReport {
    pub kind: Kind,
    /// The game version checked against, and whether it is the pack's own.
    pub game: Option<(String, bool)>,
    pub namespaces: BTreeSet<String>,
    pub blockstates: usize,
    pub items: usize,
    pub models: usize,
    pub animations: usize,
    pub textures: usize,
    pub languages: usize,
    /// References checked against what can prove them.
    pub checked: usize,
    /// References into a namespace nothing here can prove (another mod's,
    /// or the game's when it is not installed).
    pub unchecked: usize,
    pub findings: Vec<Finding>,
    /// The mod's code against the game (a mod with classes, and a game to
    /// compare with).
    pub code: Option<crate::code::CodeReport>,
}

impl JavaReport {
    pub fn broken(&self) -> usize {
        self.findings.iter().filter(|f| !f.is_info()).count()
            + self.code.as_ref().map_or(0, |c| c.broken())
    }
}

/// What a source is: a mod (by its metadata), a resource pack, or bare assets.
pub fn kind_of(src: &Source, files: &BTreeSet<String>) -> Kind {
    let text = |name: &str| {
        src.read(name)
            .map(|b| String::from_utf8_lossy(&b).to_string())
    };
    // a version requirement: a string, or a list of alternatives
    let versions = |v: Option<&Json>| match v? {
        Json::Str(s) => Some(s.clone()),
        Json::Arr(a) => {
            let all: Vec<&str> = a.iter().filter_map(Json::as_str).collect();
            (!all.is_empty()).then(|| all.join(" || "))
        }
        _ => None,
    };
    if let Some(t) = text("fabric.mod.json") {
        let j = json::parse(&t).ok();
        let j = j.as_ref();
        return Kind::Mod {
            loader: "Fabric",
            ids: j
                .and_then(|j| j.get("id").and_then(Json::as_str))
                .map(str::to_string)
                .into_iter()
                .collect(),
            minecraft: versions(j.and_then(|j| j.get("depends")?.get("minecraft"))),
            depends: j
                .and_then(|j| j.get("depends")?.as_object())
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default(),
        };
    }
    if let Some(t) = text("quilt.mod.json") {
        let j = json::parse(&t).ok();
        let q = j.as_ref().and_then(|j| j.get("quilt_loader"));
        let minecraft = q
            .and_then(|q| q.get("depends"))
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .find(|d| d.get("id").and_then(Json::as_str) == Some("minecraft"))
            .and_then(|d| versions(d.get("versions")));
        let depends = q
            .and_then(|q| q.get("depends"))
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|d| d.as_str().or_else(|| d.get("id").and_then(Json::as_str)))
            .map(str::to_string)
            .collect();
        return Kind::Mod {
            loader: "Quilt",
            ids: q
                .and_then(|q| q.get("id").and_then(Json::as_str))
                .map(str::to_string)
                .into_iter()
                .collect(),
            minecraft,
            depends,
        };
    }
    for (file, loader) in [
        ("META-INF/neoforge.mods.toml", "NeoForge"),
        ("META-INF/mods.toml", "Forge"),
    ] {
        if let Some(t) = text(file) {
            return Kind::Mod {
                loader,
                ids: toml_mod_ids(&t),
                minecraft: toml_minecraft_range(&t),
                depends: toml_dependency_ids(&t),
            };
        }
    }
    if files.contains("pack.mcmeta") {
        return Kind::ResourcePack {
            formats: text("pack.mcmeta").and_then(|t| pack_formats(&t)),
        };
    }
    Kind::Assets
}

/// The `modId` of every `[[dependencies.<id>]]` table.
pub fn toml_dependency_ids(t: &str) -> Vec<String> {
    let mut in_dep = false;
    let mut out = Vec::new();
    for l in t.lines().map(str::trim) {
        if l.starts_with('[') {
            let head = l.split('#').next().unwrap_or("").trim();
            in_dep = head
                .trim_start_matches('[')
                .trim_end_matches(']')
                .starts_with("dependencies");
            continue;
        }
        if !in_dep {
            continue;
        }
        if let Some(v) = l
            .strip_prefix("modId")
            .and_then(|r| r.trim_start().strip_prefix('='))
        {
            let v = v
                .split('#')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(['"', '\'']);
            if !v.is_empty() && !out.iter().any(|o| o == v) {
                out.push(v.to_string());
            }
        }
    }
    out
}

/// The `versionRange` of the `[[dependencies.<id>]]` table whose `modId` is
/// `minecraft`.
pub fn toml_minecraft_range(t: &str) -> Option<String> {
    let value = |l: &str, key: &str| -> Option<String> {
        let v = l.strip_prefix(key)?.trim_start().strip_prefix('=')?;
        let v = v.split('#').next()?.trim().trim_matches(['"', '\'']);
        Some(v.to_string())
    };
    let (mut in_dep, mut id, mut range) = (false, None, None);
    for l in t.lines().map(str::trim).chain(["[end]"]) {
        if l.starts_with('[') {
            if in_dep && id.as_deref() == Some("minecraft") && range.is_some() {
                return range;
            }
            let head = l.split('#').next().unwrap_or("").trim();
            in_dep = head
                .trim_start_matches('[')
                .trim_end_matches(']')
                .starts_with("dependencies");
            (id, range) = (None, None);
            continue;
        }
        if in_dep {
            id = id.or_else(|| value(l, "modId"));
            range = range.or_else(|| value(l, "versionRange"));
        }
    }
    None
}

/// The `modId` of every `[[mods]]` table of a `mods.toml`. The same key in
/// `[[dependencies.<id>]]` names the mods it NEEDS (`neoforge`, `minecraft`):
/// those are not its own.
pub fn toml_mod_ids(t: &str) -> Vec<String> {
    let mut in_mods = false;
    let mut out = Vec::new();
    for l in t.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            let head = l.split('#').next().unwrap_or("").trim();
            in_mods = head.trim_start_matches('[').trim_end_matches(']').trim() == "mods";
            continue;
        }
        if !in_mods {
            continue;
        }
        let Some(rest) = l.strip_prefix("modId") else {
            continue;
        };
        let Some(v) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let v = v
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches(['"', '\'']);
        if !v.is_empty() {
            out.push(v.to_string());
        }
    }
    out
}

/// The resource pack formats a `pack.mcmeta` declares, as `(min, max)`:
/// `min_format` / `max_format` (a number or `[major, minor]`), else
/// `pack_format` widened by `supported_formats` (`n`, `[min, max]` or
/// `{"min_inclusive", "max_inclusive"}`).
pub fn pack_formats(mcmeta: &str) -> Option<(u32, u32)> {
    let j = json::parse(mcmeta).ok()?;
    let pack = j.get("pack")?;
    let major = |v: &Json| -> Option<u32> {
        let n = match v {
            Json::Num(n) => *n,
            Json::Arr(a) => match a.first()? {
                Json::Num(n) => *n,
                _ => return None,
            },
            _ => return None,
        };
        (n >= 0.0 && n.fract() == 0.0).then_some(n as u32)
    };
    let single = pack.get("pack_format").and_then(major);
    let (s_min, s_max) = match pack.get("supported_formats") {
        Some(Json::Arr(a)) if a.len() == 2 => (major(&a[0]), major(&a[1])),
        Some(o @ Json::Obj(_)) => (
            o.get("min_inclusive").and_then(major),
            o.get("max_inclusive").and_then(major),
        ),
        Some(n @ Json::Num(_)) => (major(n), major(n)),
        _ => (None, None),
    };
    let min = pack
        .get("min_format")
        .and_then(major)
        .or(s_min.min(single).or(s_min).or(single))?;
    let max = pack
        .get("max_format")
        .and_then(major)
        .or(s_max.max(single))
        .unwrap_or(min);
    Some((min, max.max(min)))
}

/// Can a file be proven to exist?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exists {
    Yes,
    No,
    Unknown,
}

struct Ctx<'a> {
    src: &'a Source,
    files: &'a BTreeSet<String>,
    jar: Option<&'a JavaJar>,
    /// namespaces this source ships whole (a mod's own)
    own: BTreeSet<String>,
    /// `ns:path` sprites the atlases generate without a file of that name
    sprites: BTreeSet<String>,
    checked: usize,
    unchecked: usize,
}

/// `minecraft:block/stone` or `block/stone` -> (`minecraft`, `block/stone`).
fn split_id(id: &str) -> Option<(&str, &str)> {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    let ok =
        !ns.is_empty() && !path.is_empty() && !path.split('/').any(|p| p == ".." || p.is_empty());
    ok.then_some((ns, path))
}

fn model_file(ns: &str, path: &str) -> String {
    format!("assets/{ns}/models/{path}.json")
}

impl Ctx<'_> {
    fn exists(&self, ns: &str, file: &str) -> Exists {
        if self.files.contains(file) {
            return Exists::Yes;
        }
        if ns == "minecraft" {
            return match self.jar {
                Some(j) if j.zip.contains(file) => Exists::Yes,
                Some(_) => Exists::No,
                None => Exists::Unknown,
            };
        }
        if self.own.contains(ns) {
            Exists::No
        } else {
            Exists::Unknown
        }
    }

    /// Count a reference; `true` when it is proven missing.
    fn missing(&mut self, e: Exists) -> bool {
        match e {
            Exists::Unknown => {
                self.unchecked += 1;
                false
            }
            other => {
                self.checked += 1;
                other == Exists::No
            }
        }
    }

    fn model_exists(&self, id: &str) -> Exists {
        let Some((ns, path)) = split_id(id) else {
            return Exists::No;
        };
        if path.starts_with("builtin/") {
            return Exists::Yes;
        }
        self.exists(ns, &model_file(ns, path))
    }

    fn texture_exists(&self, id: &str) -> Exists {
        let Some((ns, path)) = split_id(id) else {
            return Exists::No;
        };
        if self.sprites.contains(&format!("{ns}:{path}")) {
            return Exists::Yes;
        }
        self.exists(ns, &format!("assets/{ns}/textures/{path}.png"))
    }

    /// A file from the source, else from the game.
    fn read(&self, file: &str) -> Option<Json> {
        let bytes = self.src.read(file).or_else(|| self.jar?.read(file))?;
        json::parse(&String::from_utf8_lossy(&bytes)).ok()
    }
}

/// `assets/<ns>/<kind>/...` -> `<ns>`, when the file is of that kind.
fn namespace_of<'f>(file: &'f str, kind: &str) -> Option<&'f str> {
    let mut parts = file.splitn(4, '/');
    (parts.next()? == "assets").then_some(())?;
    let ns = parts.next()?;
    (parts.next()? == kind).then_some(ns)
}

/// Sprites the atlases make without a file of their own name: paletted
/// permutations (armour trims), `single` sources with a `sprite` name,
/// `unstitch` regions.
fn atlas_sprites(atlas: &Json, out: &mut BTreeSet<String>) {
    let full = |id: &str| split_id(id).map(|(ns, p)| format!("{ns}:{p}"));
    for s in atlas
        .get("sources")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let kind = s.get("type").and_then(Json::as_str).unwrap_or("");
        match kind.trim_start_matches("minecraft:") {
            "paletted_permutations" => {
                let sep = s.get("separator").and_then(Json::as_str).unwrap_or("_");
                let keys: Vec<&String> = s
                    .get("permutations")
                    .and_then(Json::as_object)
                    .map(|o| o.keys().collect())
                    .unwrap_or_default();
                for t in s
                    .get("textures")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Json::as_str)
                {
                    if let Some(base) = full(t) {
                        for k in &keys {
                            out.insert(format!("{base}{sep}{k}"));
                        }
                    }
                }
            }
            "single" => {
                if let Some(sprite) = s.get("sprite").and_then(Json::as_str).and_then(full) {
                    out.insert(sprite);
                }
            }
            "unstitch" => {
                for r in s
                    .get("regions")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(sprite) = r.get("sprite").and_then(Json::as_str).and_then(full) {
                        out.insert(sprite);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Every model a blockstate names.
fn blockstate_models(b: &Json, out: &mut Vec<String>) {
    let mut take = |v: &Json| {
        let list: Vec<&Json> = match v {
            Json::Arr(a) => a.iter().collect(),
            other => vec![other],
        };
        for m in list {
            if let Some(id) = m.get("model").and_then(Json::as_str) {
                out.push(id.to_string());
            }
        }
    };
    if let Some(vs) = b.get("variants").and_then(Json::as_object) {
        for (_, v) in vs.iter() {
            take(v);
        }
    }
    for case in b
        .get("multipart")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(apply) = case.get("apply") {
            take(apply);
        }
    }
}

/// Every model an item definition (`assets/<ns>/items/*.json`) names, at any
/// depth: `minecraft:model` nodes' `model`, `minecraft:special` nodes' `base`.
fn item_models(j: &Json, out: &mut Vec<String>) {
    match j {
        Json::Obj(o) => {
            let kind = o
                .get("type")
                .and_then(Json::as_str)
                .map(|t| t.trim_start_matches("minecraft:"));
            let field = match kind {
                Some("model") => Some("model"),
                Some("special") => Some("base"),
                _ => None,
            };
            if let Some(id) = field.and_then(|f| o.get(f)).and_then(Json::as_str) {
                out.push(id.to_string());
            }
            for (_, v) in o.iter() {
                item_models(v, out);
            }
        }
        Json::Arr(a) => a.iter().for_each(|v| item_models(v, out)),
        _ => {}
    }
}

/// A texture value of a model: `"block/stone"` or `{"sprite": "block/stone"}`.
fn texture_value(v: &Json) -> Option<&str> {
    match v {
        Json::Str(s) => Some(s),
        Json::Obj(o) => o.get("sprite").and_then(Json::as_str),
        _ => None,
    }
}

/// The variables a used model's faces ask for that its chain never sets.
/// `None` when the chain cannot be followed to the end (a parent in a
/// namespace nothing here proves): nothing is claimed then.
fn unbound(ctx: &Ctx, model: &str) -> Option<Vec<String>> {
    let mut textures: BTreeMap<String, String> = BTreeMap::new();
    let mut elements: Option<Json> = None;
    let mut next = Some(model.to_string());
    let mut depth = 0;
    while let Some(id) = next.take() {
        depth += 1;
        let (ns, path) = split_id(&id)?;
        if depth > 32 {
            // a loop or an absurd chain: nothing is claimed
            return None;
        }
        if path.starts_with("builtin/") {
            break;
        }
        if ctx.exists(ns, &model_file(ns, path)) != Exists::Yes {
            return None;
        }
        let m = ctx.read(&model_file(ns, path))?;
        for (k, v) in m
            .get("textures")
            .and_then(Json::as_object)
            .into_iter()
            .flat_map(|o| o.iter())
        {
            if let Some(v) = texture_value(v) {
                textures.entry(k.clone()).or_insert_with(|| v.to_string());
            }
        }
        if elements.is_none() {
            elements = m.get("elements").cloned();
        }
        next = m.get("parent").and_then(Json::as_str).map(str::to_string);
    }
    let mut out = BTreeSet::new();
    for face in elements
        .as_ref()
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| e.get("faces").and_then(Json::as_object))
        .flat_map(|f| f.iter().map(|(_, v)| v))
    {
        let Some(mut t) = face.get("texture").and_then(Json::as_str) else {
            continue;
        };
        let mut hops = 0;
        while let Some(var) = t.strip_prefix('#') {
            hops += 1;
            match textures.get(var) {
                Some(v) if hops < 16 => t = v,
                _ => {
                    out.insert(var.to_string());
                    break;
                }
            }
        }
    }
    Some(out.into_iter().collect())
}

pub fn analyze(src: &Source, jar: Option<&JavaJar>) -> JavaReport {
    analyze_in(src, jar, None, &crate::code::Context::default())
}

/// As `analyze`, with the game's class index already built (a set of mods
/// shares one) and what the other mods of a set bring.
pub fn analyze_in(
    src: &Source,
    jar: Option<&JavaJar>,
    game: Option<&crate::code::GameIndex>,
    set: &crate::code::Context,
) -> JavaReport {
    let files = src.files();
    let kind = kind_of(src, &files);
    let namespaces: BTreeSet<String> = files
        .iter()
        .filter_map(|f| {
            let mut p = f.split('/');
            (p.next()? == "assets").then_some(())?;
            let ns = p.next()?;
            p.next()?;
            Some(ns.to_string())
        })
        .collect();
    let own: BTreeSet<String> = match &kind {
        Kind::Mod { ids, .. } => ids.iter().filter(|i| *i != "minecraft").cloned().collect(),
        _ => BTreeSet::new(),
    };

    let mut sprites: BTreeSet<String> = ["minecraft:missingno".to_string()].into();
    let jar_atlases: Vec<String> = jar
        .map(|j| {
            j.zip
                .names()
                .filter(|n| namespace_of(n, "atlases").is_some() && n.ends_with(".json"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let pack_atlases = files
        .iter()
        .filter(|n| namespace_of(n, "atlases").is_some() && n.ends_with(".json"));
    let mut findings = BTreeSet::new();
    for f in jar_atlases.iter().chain(pack_atlases) {
        let bytes = src.read(f).or_else(|| jar?.read(f));
        if let Some(a) = bytes.and_then(|b| json::parse(&String::from_utf8_lossy(&b)).ok()) {
            atlas_sprites(&a, &mut sprites);
        }
    }

    let mut ctx = Ctx {
        src,
        files: &files,
        jar,
        own,
        sprites,
        checked: 0,
        unchecked: 0,
    };
    let parse = |f: &String, findings: &mut BTreeSet<Finding>| -> Option<Json> {
        let bytes = src.read(f)?;
        match json::parse(&String::from_utf8_lossy(&bytes)) {
            Ok(j) => Some(j),
            Err(e) => {
                findings.insert(Finding::Unreadable {
                    file: f.clone(),
                    why: format!("{e:?}"),
                });
                None
            }
        }
    };

    // blockstates and item definitions: the models the game will draw
    let mut used: BTreeMap<String, String> = BTreeMap::new();
    let (mut blockstates, mut items) = (0, 0);
    for f in &files {
        let is_state = namespace_of(f, "blockstates").is_some();
        let is_item = namespace_of(f, "items").is_some();
        if !(is_state || is_item) || !f.ends_with(".json") {
            continue;
        }
        let Some(j) = parse(f, &mut findings) else {
            continue;
        };
        let mut models = Vec::new();
        if is_state {
            blockstates += 1;
            blockstate_models(&j, &mut models);
        } else {
            items += 1;
            item_models(&j, &mut models);
        }
        for m in models {
            let e = ctx.model_exists(&m);
            if ctx.missing(e) {
                findings.insert(Finding::MissingModel {
                    file: f.clone(),
                    model: m,
                });
            } else if e == Exists::Yes {
                used.entry(m).or_insert_with(|| f.clone());
            }
        }
    }

    // every model of the source: its parent and its textures
    let mut models = 0;
    for f in &files {
        if namespace_of(f, "models").is_none() || !f.ends_with(".json") {
            continue;
        }
        models += 1;
        let Some(j) = parse(f, &mut findings) else {
            continue;
        };
        if let Some(parent) = j.get("parent").and_then(Json::as_str) {
            let e = ctx.model_exists(parent);
            if ctx.missing(e) {
                findings.insert(Finding::MissingModel {
                    file: f.clone(),
                    model: parent.to_string(),
                });
            }
        }
        for (_, v) in j
            .get("textures")
            .and_then(Json::as_object)
            .into_iter()
            .flat_map(|o| o.iter())
        {
            let Some(t) = texture_value(v).filter(|t| !t.starts_with('#')) else {
                continue;
            };
            let e = ctx.texture_exists(t);
            if ctx.missing(e) {
                findings.insert(Finding::MissingTexture {
                    file: f.clone(),
                    texture: t.to_string(),
                });
            }
        }
    }

    // the faces of every used model have a texture
    for (model, used_by) in &used {
        for variable in unbound(&ctx, model).unwrap_or_default() {
            findings.insert(Finding::UnboundTexture {
                model: model.clone(),
                variable,
                used_by: used_by.clone(),
            });
        }
    }

    // animated textures divide into their frames
    let mut animations = 0;
    for meta in &files {
        let Some(png) = meta.strip_suffix(".mcmeta").filter(|p| p.ends_with(".png")) else {
            continue;
        };
        if namespace_of(png, "textures").is_none() || !files.contains(png) {
            continue;
        }
        let Some(j) = parse(meta, &mut findings) else {
            continue;
        };
        let Some(anim) = j.get("animation") else {
            continue;
        };
        animations += 1;
        let Some((w, h)) = src.read(png).and_then(|b| kebitx_core::image::png(&b)) else {
            continue;
        };
        let dim = |k: &str| match anim.get(k) {
            Some(Json::Num(n)) if *n >= 0.0 => Some(*n as u32),
            _ => None,
        };
        // the frame the game uses: as declared, else square strips of the
        // width; a wider-than-tall image with no size declared is not judged
        let frame = match (dim("width"), dim("height")) {
            (Some(fw), Some(fh)) => (fw, fh),
            (None, None) if w <= h => (w, w),
            _ => continue,
        };
        if frame.0 == 0 || frame.1 == 0 || w % frame.0 != 0 || h % frame.1 != 0 {
            findings.insert(Finding::BadAnimation {
                texture: png.to_string(),
                size: (w, h),
                frame,
            });
        }
    }

    // textures: a PNG the game reads, in the game's proportions
    let mut textures = 0;
    for f in &files {
        if namespace_of(f, "textures").is_none() || !f.ends_with(".png") {
            continue;
        }
        textures += 1;
        let Some(bytes) = src.read(f) else {
            continue;
        };
        let Some((w, h)) = kebitx_core::image::png(&bytes) else {
            findings.insert(Finding::NotPng { texture: f.clone() });
            continue;
        };
        // an animated strip's height is its frame count: not comparable
        let meta = format!("{f}.mcmeta");
        if files.contains(&meta) || jar.is_some_and(|j| j.zip.contains(&meta)) {
            continue;
        }
        let Some((gw, gh)) = jar
            .filter(|j| j.zip.contains(f))
            .and_then(|j| j.read(f))
            .and_then(|b| kebitx_core::image::png(&b))
        else {
            continue;
        };
        if u64::from(w) * u64::from(gh) != u64::from(h) * u64::from(gw) {
            findings.insert(Finding::Proportions {
                texture: f.clone(),
                size: (w, h),
                game: (gw, gh),
            });
        }
    }

    // languages: keys English has and another language lacks (information)
    // A language is an object of strings; the game's `deprecated.json` (lists
    // of removed and renamed keys) sits in the same folder and is not one.
    let mut languages = 0;
    let mut by_ns: BTreeMap<&str, BTreeMap<&String, BTreeSet<String>>> = BTreeMap::new();
    for f in &files {
        let Some(ns) = namespace_of(f, "lang").filter(|_| f.ends_with(".json")) else {
            continue;
        };
        let Some(j) = parse(f, &mut findings) else {
            continue;
        };
        let Some(o) = j
            .as_object()
            .filter(|o| o.iter().all(|(_, v)| matches!(v, Json::Str(_))))
        else {
            continue;
        };
        languages += 1;
        by_ns
            .entry(ns)
            .or_default()
            .insert(f, o.keys().cloned().collect());
    }
    for (ns, langs) in &by_ns {
        let en = format!("assets/{ns}/lang/en_us.json");
        let Some(en_keys) = langs.get(&en) else {
            continue;
        };
        for (f, keys) in langs.iter().filter(|(f, _)| ***f != en) {
            let missing = en_keys.difference(keys).count();
            if missing > 0 {
                findings.insert(Finding::MissingLangKeys {
                    file: (*f).clone(),
                    missing,
                    of: en_keys.len(),
                });
            }
        }
    }

    // the code: every game class, member and Mixin target it names
    let (loader, depends): (Option<&str>, &[String]) = match &kind {
        Kind::Mod {
            loader, depends, ..
        } => (Some(loader), depends),
        _ => (None, &[]),
    };
    let has_code = files.iter().any(|f| f.ends_with(".class"));
    let built;
    let index = match (game, jar) {
        (Some(g), _) => Some(g),
        (None, Some(j)) if has_code => {
            built = crate::code::GameIndex::from_jar(j);
            Some(&built)
        }
        _ => None,
    };
    let code = index
        .filter(|_| has_code)
        .and_then(|g| crate::code::check(src, &files, g, loader, depends, set));

    JavaReport {
        kind,
        game: jar.map(|j| (j.id.clone(), j.exact)),
        namespaces,
        blockstates,
        items,
        models,
        animations,
        textures,
        languages,
        checked: ctx.checked,
        unchecked: ctx.unchecked,
        findings: findings.into_iter().collect(),
        code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_mods_tables_name_the_mod() {
        let t = r#"
modLoader="javafml"
[[mods]] # the mod
modId="farmersdelight"
displayName="Farmer's Delight"
[[dependencies.farmersdelight]]
    modId="neoforge"
[[dependencies.farmersdelight]]
    modId = "minecraft"
    versionRange = "[1.21.1]" # exact
"#;
        assert_eq!(toml_mod_ids(t), ["farmersdelight"]);
        assert_eq!(toml_minecraft_range(t).as_deref(), Some("[1.21.1]"));
    }

    #[test]
    fn format_ranges_in_every_form() {
        let f = |s: &str| pack_formats(s);
        assert_eq!(f(r#"{"pack":{"pack_format":34}}"#), Some((34, 34)));
        assert_eq!(
            f(r#"{"pack":{"min_format":84,"max_format":97}}"#),
            Some((84, 97))
        );
        assert_eq!(
            f(r#"{"pack":{"min_format":[88,0],"max_format":[97,1]}}"#),
            Some((88, 97))
        );
        assert_eq!(
            f(r#"{"pack":{"pack_format":34,"supported_formats":[32,46]}}"#),
            Some((32, 46))
        );
        assert_eq!(
            f(
                r#"{"pack":{"pack_format":34,"supported_formats":{"min_inclusive":34,"max_inclusive":48}}}"#
            ),
            Some((34, 48))
        );
        assert_eq!(f(r#"{"pack":{"description":"x"}}"#), None);
    }
}
