//! The compatibility report: a verdict for every file of a Java resource pack.

use crate::content::{self, BedrockPixels, JavaPixels, Proof};
use crate::java::JavaJar;
use crate::lang::{self, BedrockLang, LangResult};
use crate::sounds::{self, SoundEvent};
use crate::{flipbook, names};
use kebitx_core::codec::png::Rgba;
use kebitx_core::filesystem::{all_files, asset_stem};
use kebitx_core::json::Json;
use kebitx_core::vanilla::VanillaCatalog;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Files the conversion generates rather than copies.
pub const MANIFEST: &str = "manifest.json";
pub const FLIPBOOKS: &str = "textures/flipbook_textures.json";
pub const SOUND_DEFINITIONS: &str = "sounds/sound_definitions.json";
pub const LANGUAGES: &str = "texts/languages.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    Converts,
    Adapts,
    Rebuild,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Converts => "converts",
            Verdict::Adapts => "adapts",
            Verdict::Rebuild => "rebuild",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path in the Java pack, `/` separators.
    pub java: String,
    pub verdict: Verdict,
    /// Where it goes in the Bedrock pack, for `Converts`: a copy, or a file
    /// generated from it (manifest, flipbooks, sound definitions, language
    /// files, a cut or composed picture); none when nothing needs writing.
    pub bedrock: Option<String>,
    /// `Converts` by the same path (true), or by weaker evidence (false): a
    /// rename the game confirmed, identical pixels, or a unique file name —
    /// so a path always wins a target.
    pub exact: bool,
    pub reason: &'static str,
}

#[derive(Debug, Default)]
pub struct MigrationReport {
    /// `pack.mcmeta` was found: this is a Java resource pack.
    pub is_java_pack: bool,
    /// What the pack is called: its folder, or the archive it came in.
    pub name: String,
    pub description: String,
    pub pack_format: Option<u32>,
    pub entries: Vec<Entry>,
    /// Flipbook entries for the animated textures that convert.
    pub flipbooks: Vec<Json>,
    /// What became of each event of the pack's `sounds.json`.
    pub sound_events: Vec<SoundEvent>,
    /// Bedrock sound event -> its new definition.
    pub sound_definitions: BTreeMap<String, Json>,
    /// Pictures made by cutting a pack texture, or pasting pack sprites into
    /// a copy of a game sheet: Bedrock path -> picture.
    pub images: BTreeMap<String, Rgba>,
    /// Bedrock language code -> key -> text.
    pub texts: BTreeMap<String, BTreeMap<String, String>>,
    /// Each Java language file and what became of it.
    pub languages: Vec<(String, LangResult)>,
    /// Changed or new Java models with cubes, converted to Bedrock geometry.
    pub models: Vec<ModelDraft>,
    /// Behavior-pack files: path -> content (recipes).
    pub behavior: BTreeMap<String, Json>,
    /// OptiFine entity models and the Bedrock bones their parts are proven to be.
    pub cem: Vec<crate::cem::CemReport>,
    /// The mod, when the input is one: (loader, name).
    pub mod_info: Option<(String, String)>,
    /// Item atlas key -> texture (mods' items).
    pub item_atlas: BTreeMap<String, String>,
    /// Terrain atlas key -> texture (mods' blocks).
    pub terrain: BTreeMap<String, String>,
    /// Other generated resource-pack files: path -> content.
    pub resource: BTreeMap<String, Json>,
    /// What a person has to do (mods).
    pub tasks: Vec<String>,
    /// Task lists: path under `kebitx_drafts/` -> text (functions).
    pub task_files: BTreeMap<String, String>,
    /// Drafts for a person to check and move: path under `kebitx_drafts/` -> content.
    pub draft_files: BTreeMap<String, Json>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Block,
    Item,
}

/// A Java model converted to Bedrock geometry. The geometry follows the
/// rules Mojang's own conversion shows; what uses it — a custom block, an
/// attachable — is a draft for a person to finish.
#[derive(Debug, Clone)]
pub struct ModelDraft {
    /// The Java model file.
    pub java: String,
    pub kind: ModelKind,
    /// A file-safe name (`block/oak_planks` -> `oak_planks`).
    pub name: String,
    pub geometry: Json,
    /// material instance -> the Bedrock texture it shows (no extension), or
    /// `None` when the Java texture has no proven Bedrock file
    pub textures: BTreeMap<String, Option<String>>,
    /// Elements whose rotation or rescale is converted without proof.
    pub unverified: usize,
    /// Culling rules (bone, face, direction) from Java's `cullface`.
    pub culling: Vec<(String, String, String)>,
}

impl ModelDraft {
    pub fn geometry_id(&self) -> String {
        format!("geometry.kebitx.{}", self.name)
    }
}

impl MigrationReport {
    pub fn count(&self, v: Verdict) -> usize {
        self.entries.iter().filter(|e| e.verdict == v).count()
    }
    /// verdict -> reason -> how many files
    pub fn by_reason(&self) -> BTreeMap<(Verdict, &'static str), usize> {
        let mut m = BTreeMap::new();
        for e in &self.entries {
            *m.entry((e.verdict, e.reason)).or_insert(0) += 1;
        }
        m
    }
    /// Is this Bedrock path one the conversion generates?
    pub fn is_generated(&self, path: &str) -> bool {
        [MANIFEST, FLIPBOOKS, SOUND_DEFINITIONS, LANGUAGES].contains(&path)
            || self.images.contains_key(path)
            || path.starts_with("behavior_pack/")
            || self.texts.keys().any(|c| path == lang_path(c))
    }
}

pub fn lang_path(code: &str) -> String {
    format!("texts/{code}.lang")
}

/// What the report is checked against. Only the catalog is required; each
/// other source makes more provable, and its absence only means less does.
#[derive(Clone, Copy)]
pub struct References<'a> {
    pub catalog: &'a VanillaCatalog,
    /// The Java game's own `sounds.json`.
    pub java_sounds: Option<&'a Json>,
    /// The Java game's `.jar`: its own textures and English strings.
    pub java_jar: Option<&'a JavaJar>,
    pub java_pixels: Option<&'a JavaPixels>,
    pub bedrock_pixels: Option<&'a BedrockPixels>,
    pub bedrock_lang: Option<&'a BedrockLang>,
    /// The two games' own recipes.
    pub recipes: Option<&'a crate::recipes::RecipeRefs>,
    /// The two games' own loot tables.
    pub loot: Option<&'a crate::loot::LootRefs>,
    /// Bedrock's entity geometries, for OptiFine entity models.
    pub bedrock_geometry: Option<&'a crate::cem::BedrockGeometry>,
    /// Components Bedrock's own items and blocks use, for mods' adapters.
    pub bedrock_defs: Option<&'a crate::adapters::BedrockDefs>,
}

impl<'a> References<'a> {
    pub fn catalog_only(catalog: &'a VanillaCatalog) -> Self {
        References {
            catalog,
            java_sounds: None,
            java_jar: None,
            java_pixels: None,
            bedrock_pixels: None,
            bedrock_lang: None,
            recipes: None,
            loot: None,
            bedrock_geometry: None,
            bedrock_defs: None,
        }
    }
}

/// Java texture folders and their Bedrock names. Only where the folder holds
/// the same kind of texture in both editions; anything else is not mapped.
const TEXTURE_FOLDERS: [(&str, &str); 9] = [
    ("block", "blocks"),
    ("item", "items"),
    ("entity", "entity"),
    ("environment", "environment"),
    ("particle", "particle"),
    ("painting", "painting"),
    ("misc", "misc"),
    ("colormap", "colormap"),
    ("map", "map"),
];

struct Ctx<'a> {
    java_root: &'a Path,
    refs: References<'a>,
    by_name: HashMap<(String, String), Vec<String>>,
    files: Vec<String>,
    /// Items and blocks the mods convert to: their recipes and loot may name them.
    mod_ids: HashSet<String>,
}

impl Ctx<'_> {
    fn has(&self, p: &str) -> bool {
        self.files.binary_search_by(|f| f.as_str().cmp(p)).is_ok()
    }
}

/// A picture to generate, decided per texture and made once all are known.
enum Make {
    Cut {
        target: String,
        img: Rgba,
    },
    Paste {
        sheet: String,
        x: u32,
        y: u32,
        img: Rgba,
    },
    /// A converted model; its textures are mapped once every texture's
    /// verdict is known: material instance -> Java texture id.
    Model(ModelDraft, BTreeMap<String, String>),
    /// A behavior-pack file.
    Behavior(Vec<(String, Json)>),
    /// An OptiFine entity model, inspected.
    Cem(crate::cem::CemReport),
    /// A task list for a person: path under `kebitx_drafts/` -> text.
    Task(String, String),
}

/// A data-pack file (`data/<namespace>/<kind>/...`).
fn data_file(f: &str, parts: &[&str], ctx: &Ctx) -> (Entry, Option<Json>, Option<Make>) {
    let entry = |verdict, bedrock: Option<String>, exact, reason| Entry {
        java: f.to_string(),
        verdict,
        bedrock,
        exact,
        reason,
    };
    let plain = |e| (e, None, None);
    if in_java(f, ctx) == InJava::Same {
        return plain(entry(
            Verdict::Converts,
            None,
            true,
            "the Java game's own file, unchanged: nothing for Bedrock to change",
        ));
    }
    let kind = parts[2];
    if matches!(kind, "function" | "functions") && f.ends_with(".mcfunction") {
        let Some(bl) = ctx.refs.bedrock_lang else {
            return plain(entry(
                Verdict::Adapts,
                None,
                false,
                "function: checking its commands needs the Bedrock game's command list",
            ));
        };
        let Ok(text) = kebitx_core::filesystem::read_text(&ctx.java_root.join(f)) else {
            return plain(entry(
                Verdict::Adapts,
                None,
                false,
                "function that cannot be read",
            ));
        };
        let known = crate::functions::bedrock_commands(&bl.en);
        let lines = crate::functions::analyze(&text, &known);
        let all_ok = lines
            .iter()
            .all(|l| l.unknown.is_empty() && l.notes.is_empty());
        let reason = if all_ok {
            "function: every command exists in Bedrock (its arguments still to check): list in kebitx_drafts/functions/"
        } else {
            "function: commands Bedrock does not have, or NBT and macros: list in kebitx_drafts/functions/"
        };
        let path = format!(
            "functions/{}.txt",
            parts[1..]
                .join("/")
                .trim_end_matches(".mcfunction")
                .replace("/function/", "/")
                .replace("/functions/", "/")
        );
        return (
            entry(Verdict::Adapts, None, false, reason),
            None,
            Some(Make::Task(path, crate::functions::task_file(f, &lines))),
        );
    }
    if matches!(kind, "loot_table" | "loot_tables")
        && f.ends_with(".json")
        && ctx.refs.loot.is_some()
    {
        return loot_file(f, parts, ctx);
    }
    if !matches!(kind, "recipe" | "recipes") || !f.ends_with(".json") {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            match kind {
                "loot_table" | "loot_tables" => {
                    "loot table: Bedrock's loot tables have another format (no rule yet)"
                }
                "tags" => "tag: Bedrock cannot add items to the game's tags; recipes that use it are spelled out with its items",
                "advancement" | "advancements" => {
                    "advancement: Bedrock has no advancements (achievements are not data)"
                }
                "function" | "functions" => {
                    "function: commands differ between editions, each needs checking"
                }
                "worldgen" | "dimension" | "dimension_type" => {
                    "world generation: Bedrock's is another system"
                }
                _ => "data file with no Bedrock equivalent known",
            },
        ));
    }
    let Some(refs) = ctx.refs.recipes else {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "recipe: proving it needs both installed games' own recipes",
        ));
    };
    let Some(java) = read_json(&ctx.java_root.join(f)) else {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "recipe that cannot be read",
        ));
    };
    let Some(recipe) = crate::recipes::parse_java(&java) else {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "recipe of a kind with no Bedrock form here (smithing, special crafting, ...)",
        ));
    };
    // tags Bedrock does not have become their Java items (from the pack's
    // tags, else the Java game's), named as Bedrock names them
    let renamed = refs.to_bedrock_names(&recipe);
    let items_of = |t: &str| crate::recipes::tag_items(t, ctx.java_root, ctx.refs.java_jar);
    let Some(expanded) = crate::recipes::expand_unknown_tags(&renamed, &refs.known, &items_of)
    else {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "recipe using a tag neither Bedrock nor the pack nor the Java game defines",
        ));
    };
    let bedrock = refs.to_bedrock_names(&expanded);
    let unknown: Vec<crate::recipes::Alt> = refs
        .unknown(&bedrock)
        .into_iter()
        .filter(|a| !matches!(a, crate::recipes::Alt::Item(i) if ctx.mod_ids.contains(i)))
        .collect();
    if !unknown.is_empty() {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "recipe naming an item or tag Bedrock's own recipes never name: the name must be mapped by a person",
        ));
    }
    if refs.bedrock.contains_key(&bedrock) {
        return plain(entry(
            Verdict::Converts,
            None,
            true,
            "a recipe Bedrock already has, the same: nothing to write",
        ));
    }
    // one item or tag per ingredient: a choice of several is several recipes
    let Some(singles) = crate::recipes::singles(&bedrock, 64) else {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "recipe whose alternatives make more than 64 recipes in Bedrock",
        ));
    };
    // a changed vanilla recipe takes the identifier of Bedrock's own, so it
    // replaces it instead of adding a second way to craft
    let overrides = refs.same_as.get(f).filter(|_| {
        !matches!(recipe, crate::recipes::Recipe::Cooking { .. }) && singles.len() == 1
    });
    let (ns, rest) = (parts[1], parts[3..].join("/"));
    let base = format!("{ns}:{}", rest.trim_end_matches(".json").replace('/', "_"));
    let mut files = Vec::new();
    for (k, single) in singles.iter().enumerate() {
        let id = match (overrides, singles.len()) {
            (Some(o), _) => o.clone(),
            (None, 1) => base.clone(),
            (None, _) => format!("{base}_{}", k + 1),
        };
        let path = format!("recipes/{}.json", id.replace([':', '/'], "_"));
        files.push((path, crate::recipes::to_bedrock(&id, single)));
    }
    let reason = match (overrides.is_some(), singles.len()) {
        (true, _) => "changed recipe: written with the identifier of Bedrock's own, which it replaces",
        (false, 1) => "recipe converted: means the same in Bedrock (the converter reproduces the games' own recipes)",
        _ => "recipe converted as one Bedrock recipe per alternative (tags and choices spelled out)",
    };
    let first = files.first().map(|(p, _)| format!("{BEHAVIOR}/{p}"));
    (
        entry(Verdict::Converts, first, true, reason),
        None,
        Some(Make::Behavior(files)),
    )
}

/// Behavior-pack outputs are named under this prefix in `Entry::bedrock`.
pub const BEHAVIOR: &str = "behavior_pack";

/// `pack.mcmeta`: is it there, its description and its resource format
/// (`pack_format`, or the major of `min_format`).
pub fn pack_meta(java_root: &Path) -> (bool, String, Option<u32>) {
    let Some(meta) = std::fs::read_to_string(java_root.join("pack.mcmeta"))
        .ok()
        .and_then(|s| kebitx_core::json::parse(&s).ok())
    else {
        return (false, String::new(), None);
    };
    let pack = meta.get("pack");
    let description = pack
        .and_then(|p| p.get("description"))
        .map(description_text)
        .unwrap_or_default();
    let number = |v: &Json| match v {
        Json::Num(n) if *n >= 0.0 => Some(*n as u32),
        Json::Arr(a) => match a.first() {
            Some(Json::Num(n)) if *n >= 0.0 => Some(*n as u32),
            _ => None,
        },
        _ => None,
    };
    let format = pack
        .and_then(|p| p.get("pack_format").or_else(|| p.get("min_format")))
        .and_then(number);
    (true, description, format)
}

/// The report for the Java pack at `java_root`.
pub fn inspect(java_root: &Path, refs: References) -> MigrationReport {
    let mut report = MigrationReport::default();
    (report.is_java_pack, report.description, report.pack_format) = pack_meta(java_root);
    report.name = java_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "converted".into());
    if !report.is_java_pack {
        if let Some((loader, name)) = crate::input::mod_meta(java_root) {
            report.is_java_pack = true;
            report.description = name.clone();
            report.mod_info = Some((loader.to_string(), name));
        }
    }

    // (Bedrock texture folder, file name) -> its stems, to match by a name that
    // exists once and only once IN THE SAME KIND of texture. Across kinds a
    // name proves nothing: Java's `block/bricks` is not Bedrock's shield
    // pattern `bricks`, a boat's `bamboo` is not the bamboo item.
    let mut by_name: HashMap<(String, String), Vec<String>> = HashMap::new();
    for stem in refs
        .catalog
        .asset_stems()
        .filter(|s| s.starts_with("textures/"))
    {
        let folder = stem.split('/').nth(1).unwrap_or("").to_string();
        let name = stem.rsplit('/').next().unwrap_or(stem).to_string();
        by_name
            .entry((folder, name))
            .or_default()
            .push(stem.clone());
    }
    let mut files: Vec<String> = all_files(java_root)
        .iter()
        .filter_map(|f| {
            f.strip_prefix(java_root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    files.sort();
    // the mods' items and blocks first: what they convert to is what their
    // recipes and loot tables may name
    let mut mods = crate::adapters::ModOutput::default();
    if let Some(defs) = refs.bedrock_defs {
        let namespaces: std::collections::BTreeSet<String> = files
            .iter()
            .filter_map(|f| {
                f.strip_prefix("assets/")?
                    .split_once('/')
                    .map(|(ns, _)| ns.to_string())
            })
            .filter(|ns| ns != "minecraft")
            .collect();
        for ns in &namespaces {
            crate::adapters::adapt(ns, java_root, &files, refs.java_jar, defs, &mut mods);
        }
    }
    let mod_ids: HashSet<String> = mods
        .behavior
        .iter()
        .filter_map(|(_, j)| {
            ["minecraft:item", "minecraft:block"]
                .iter()
                .find_map(|k| j.get(k))?
                .get("description")?
                .get("identifier")?
                .as_str()
                .map(str::to_string)
        })
        .collect();
    let ctx = Ctx {
        java_root,
        refs,
        by_name,
        files,
        mod_ids,
    };

    // sounds first: a pack sound with no Bedrock file converts when a
    // converted event plays it
    let sounds_json = "assets/minecraft/sounds.json";
    let sounds = ctx.has(sounds_json).then(|| {
        let pack = read_json(&java_root.join(sounds_json)).unwrap_or(Json::Null);
        sounds::convert(&pack, refs.java_sounds, refs.catalog, &|p| ctx.has(p))
    });
    let used_sounds = sounds.as_ref().map(|s| &s.used_files);

    // Java key -> Bedrock key, proven by identical English
    let key_map = match (refs.java_jar, refs.bedrock_lang) {
        (Some(jar), Some(bl)) => jar
            .read("assets/minecraft/lang/en_us.json")
            .and_then(|b| kebitx_core::json::parse(&String::from_utf8_lossy(&b)).ok())
            .map(|en| lang::key_map(&en, bl)),
        _ => None,
    };

    let mut animations: HashMap<String, Json> = HashMap::new();
    let mut makes: HashMap<String, Make> = HashMap::new();
    for f in &ctx.files {
        let entry = |verdict, bedrock: Option<String>, exact, reason| Entry {
            java: f.clone(),
            verdict,
            bedrock,
            exact,
            reason,
        };
        let lower = f.to_ascii_lowercase();
        let e = match f.as_str() {
            "pack.mcmeta" => {
                entry(Verdict::Converts, Some(MANIFEST.into()), true, "pack metadata: a Bedrock manifest is generated")
            }
            "pack.png" => entry(Verdict::Converts, Some("pack_icon.png".into()), true, "pack icon"),
            "assets/minecraft/sounds.json" => match &sounds {
                Some(s) if !s.definitions.is_empty() => entry(
                    Verdict::Converts,
                    Some(SOUND_DEFINITIONS.into()),
                    true,
                    "sounds.json: the events proven to be Bedrock ones go to sound_definitions.json",
                ),
                Some(s) if !s.events.is_empty() && s.events.iter().all(|e| e.verdict == Verdict::Converts) => {
                    entry(Verdict::Converts, None, true, "sounds.json: nothing Bedrock does not already play")
                }
                _ => entry(Verdict::Adapts, None, false, "sounds.json: no event could be proven to be a Bedrock one"),
            },
            _ if lower.starts_with("assets/minecraft/lang/") && lower.ends_with(".json") => {
                let code = lower.trim_start_matches("assets/minecraft/lang/").trim_end_matches(".json").to_string();
                language(f, &code, &ctx, key_map.as_ref(), &mut report)
            }
            _ => {
                let (e, anim, make) = classify(f, &ctx, used_sounds);
                if let Some(a) = anim {
                    animations.insert(f.clone(), a);
                }
                if let Some(m) = make {
                    makes.insert(f.clone(), m);
                }
                e
            }
        };
        report.entries.push(e);
    }
    let sheets: HashSet<String> = makes
        .values()
        .filter_map(|m| match m {
            Make::Paste { sheet, .. } => Some(format!("{sheet}.png")),
            _ => None,
        })
        .collect();
    drop_contested(&mut report.entries, &sheets);

    // the mods' items and blocks: their files convert, their blocks drop
    // what their loot tables say once those are known to convert
    mods.mark_languages(&ctx.files);
    for e in report.entries.iter_mut() {
        if let Some(reason) = mods.adapted.get(&e.java) {
            e.verdict = Verdict::Adapts;
            e.reason = reason;
        }
        if let Some((target, reason)) = mods.converted.get(&e.java) {
            e.verdict = Verdict::Converts;
            e.bedrock = target.clone();
            e.exact = true;
            e.reason = reason;
        }
    }
    let loot_ok: HashMap<String, String> = report
        .entries
        .iter()
        .filter(|e| e.verdict == Verdict::Converts)
        .filter_map(|e| {
            Some((
                e.java.clone(),
                e.bedrock
                    .as_ref()?
                    .strip_prefix("behavior_pack/")?
                    .to_string(),
            ))
        })
        .collect();
    for (block, loot) in &mods.block_loot {
        let Some(path) = loot_ok.get(loot) else {
            continue;
        };
        if let Some(e) = report.entries.iter_mut().find(|e| &e.java == loot) {
            e.reason = "mod block loot table: converted, and the custom block drops it";
        }
        if let Some((_, def)) = mods.behavior.iter_mut().find(|(p, _)| p == block) {
            if let Some(Json::Obj(components)) = def
                .get("minecraft:block")
                .and_then(|b| b.get("components"))
                .cloned()
            {
                let mut c: Vec<(String, Json)> = components
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                c.push(("minecraft:loot".into(), Json::Str(path.clone())));
                let mut block_obj: Vec<(String, Json)> = def
                    .get("minecraft:block")
                    .and_then(Json::as_object)
                    .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                block_obj.retain(|(k, _)| k != "components");
                block_obj.push((
                    "components".into(),
                    Json::Obj(kebitx_core::json::JsonMap::from_entries(c)),
                ));
                let mut top: Vec<(String, Json)> = def
                    .as_object()
                    .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                top.retain(|(k, _)| k != "minecraft:block");
                top.push((
                    "minecraft:block".into(),
                    Json::Obj(kebitx_core::json::JsonMap::from_entries(block_obj)),
                ));
                *def = Json::Obj(kebitx_core::json::JsonMap::from_entries(top));
            }
        }
    }
    report.behavior.extend(mods.behavior);
    report.resource.extend(mods.resource);
    report.item_atlas.extend(mods.item_atlas);
    report.terrain.extend(mods.terrain);
    report.tasks.extend(mods.tasks);
    report.draft_files.extend(mods.drafts);
    // the mods' 3D items: drafted as attachables, textures as converted above
    for f in &mods.model_items {
        if let Ok(Make::Model(mut d, materials)) = convert_model(f, ModelKind::Item, &ctx) {
            for (instance, java_id) in &materials {
                d.textures.insert(
                    instance.clone(),
                    bedrock_texture(java_id, &report.entries, refs.catalog),
                );
            }
            report.models.push(d);
        }
    }
    for (java_code, lines) in mods.texts {
        let code = refs
            .bedrock_lang
            .and_then(|bl| bl.language(&java_code))
            .map(str::to_string)
            .or_else(|| (java_code == "en_us").then(|| "en_US".to_string()));
        if let Some(code) = code {
            report.texts.entry(code).or_default().extend(lines);
        }
    }

    let converted: HashSet<String> = report
        .entries
        .iter()
        .filter(|e| e.verdict == Verdict::Converts)
        .map(|e| e.java.clone())
        .collect();

    // pictures: cuts as they are, pastes onto a copy of their sheet
    let mut pasted: BTreeMap<String, Rgba> = BTreeMap::new();
    // models: their textures mapped now that every texture has its verdict
    let mut models: Vec<ModelDraft> = Vec::new();
    for m in makes.values() {
        if let Make::Model(draft, materials) = m {
            let mut d = draft.clone();
            for (instance, java_id) in materials {
                d.textures.insert(
                    instance.clone(),
                    bedrock_texture(java_id, &report.entries, refs.catalog),
                );
            }
            models.push(d);
        }
        if let Make::Cem(c) = m {
            report.cem.push(c.clone());
        }
        if let Make::Task(path, text) = m {
            report.task_files.insert(path.clone(), text.clone());
        }
    }
    models.sort_by(|a, b| a.java.cmp(&b.java));
    report.cem.sort_by(|a, b| a.entity.cmp(&b.entity));
    report.models.extend(models);
    report.models.sort_by(|a, b| a.java.cmp(&b.java));
    let mut makes: Vec<(String, Make)> = makes
        .into_iter()
        .filter(|(f, m)| {
            converted.contains(f) && !matches!(m, Make::Model(..) | Make::Cem(..) | Make::Task(..))
        })
        .collect();
    makes.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, m) in makes {
        match m {
            Make::Model(..) | Make::Cem(..) | Make::Task(..) => {}
            Make::Behavior(files) => {
                report.behavior.extend(files);
            }
            Make::Cut { target, img } => {
                report.images.insert(target, img);
            }
            Make::Paste { sheet, x, y, img } => {
                let base = refs.bedrock_pixels.and_then(|bp| bp.sheets.get(&sheet));
                if let Some(base) = base {
                    let canvas = pasted
                        .entry(format!("{sheet}.png"))
                        .or_insert_with(|| base.clone());
                    canvas.paste(&img, x, y);
                }
            }
        }
    }
    report.images.extend(pasted);

    // an animation converts with its texture, and only then
    let mut flipbooks: Vec<(String, Json)> = animations
        .into_iter()
        .filter(|(png, _)| converted.contains(png))
        .collect();
    flipbooks.sort_by(|a, b| a.0.cmp(&b.0));
    let animated: HashSet<String> = flipbooks
        .iter()
        .map(|(png, _)| format!("{png}.mcmeta"))
        .collect();
    for e in report
        .entries
        .iter_mut()
        .filter(|e| animated.contains(&e.java))
    {
        e.verdict = Verdict::Converts;
        e.bedrock = Some(FLIPBOOKS.into());
        e.exact = true;
        e.reason = "texture animation: becomes a flipbook entry";
    }
    report.flipbooks = flipbooks.into_iter().map(|(_, j)| j).collect();
    if let Some(s) = sounds {
        report.sound_events = s.events;
        report.sound_definitions = s.definitions;
    }
    report
}

fn read_json(p: &Path) -> Option<Json> {
    kebitx_core::filesystem::read_text(p)
        .ok()
        .and_then(|s| kebitx_core::json::parse(&s).ok())
}

/// A Java language file.
fn language(
    f: &str,
    code: &str,
    ctx: &Ctx,
    key_map: Option<&HashMap<String, String>>,
    report: &mut MigrationReport,
) -> Entry {
    let entry = |verdict, bedrock: Option<String>, exact, reason| Entry {
        java: f.to_string(),
        verdict,
        bedrock,
        exact,
        reason,
    };
    let (Some(map), Some(bl)) = (key_map, ctx.refs.bedrock_lang) else {
        return entry(
            Verdict::Adapts,
            None,
            false,
            "language file: proving its keys needs the installed Java game's en_us.json and Bedrock's en_US.lang",
        );
    };
    let Some(pack) = read_json(&ctx.java_root.join(f)) else {
        return entry(
            Verdict::Adapts,
            None,
            false,
            "language file that cannot be read",
        );
    };
    let result = lang::convert(code, &pack, map, bl);
    let e = match &result.bedrock {
        None => entry(
            Verdict::Adapts,
            None,
            false,
            "a language the Bedrock game does not have",
        ),
        Some(_) if result.mapped == 0 => entry(
            Verdict::Adapts,
            None,
            false,
            "language file: no string could be proven to be a Bedrock one",
        ),
        Some(_) if result.lines.is_empty() => entry(
            Verdict::Converts,
            None,
            true,
            "language file: the same strings Bedrock already has",
        ),
        Some(b) => {
            report
                .texts
                .entry(b.clone())
                .or_default()
                .extend(result.lines.clone());
            entry(
                Verdict::Converts,
                Some(lang_path(b)),
                true,
                "language file: strings proven by identical English",
            )
        }
    };
    report.languages.push((f.to_string(), result));
    e
}

fn classify(
    f: &str,
    ctx: &Ctx,
    used_sounds: Option<&HashSet<String>>,
) -> (Entry, Option<Json>, Option<Make>) {
    let entry = |verdict, bedrock: Option<String>, exact, reason| Entry {
        java: f.to_string(),
        verdict,
        bedrock,
        exact,
        reason,
    };
    let plain = |e| (e, None, None);
    let parts: Vec<&str> = f.split('/').collect();
    if parts.len() >= 3 && parts[0] == "data" {
        return data_file(f, &parts, ctx);
    }
    if f.to_ascii_lowercase().ends_with(".class") {
        return plain(entry(
            Verdict::Rebuild,
            None,
            false,
            "Java code: behaviour to reimplement in Bedrock (scripts, components): see kebitx_drafts/mod_tasks.txt",
        ));
    }
    if matches!(
        f,
        "fabric.mod.json" | "quilt.mod.json" | "META-INF/mods.toml" | "META-INF/neoforge.mods.toml"
    ) {
        return plain(entry(
            Verdict::Converts,
            Some(MANIFEST.into()),
            true,
            "mod metadata: its name goes into the generated manifests",
        ));
    }
    if parts.len() < 3 || parts[0] != "assets" {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "outside assets/: no automatic rule",
        ));
    }
    let (namespace, kind) = (parts[1], parts[2]);
    if namespace != "minecraft" {
        return plain(entry(
            Verdict::Rebuild,
            None,
            false,
            "another namespace: content of a mod, to be rebuilt in Bedrock first",
        ));
    }
    let lower = f.to_ascii_lowercase();
    // A data file the pack did not change is the game's own: Bedrock needs
    // nothing for it. Only what the pack changed or added needs a person.
    let in_java = if matches!(kind, "textures" | "sounds" | "lang") {
        InJava::Unknown
    } else {
        in_java(f, ctx)
    };
    if in_java == InJava::Same {
        return plain(entry(
            Verdict::Converts,
            None,
            true,
            "the Java game's own file, unchanged: nothing for Bedrock to change",
        ));
    }
    let by_change = |changed: &'static str, new: &'static str, unknown: &'static str| match in_java
    {
        InJava::Changed => changed,
        InJava::New => new,
        _ => unknown,
    };
    plain(match kind {
        "shaders" | "post_effect" => {
            entry(Verdict::Rebuild, None, false, "Java shaders: no Bedrock equivalent (Vibrant Visuals is the closest)")
        }
        "models" if matches!(in_java, InJava::Changed | InJava::New) && model_kind(f).is_some() => {
            let kind = model_kind(f).unwrap_or(ModelKind::Block);
            return match convert_model(f, kind, ctx) {
                Ok(m) => {
                    let reason = match kind {
                        ModelKind::Block => "model converted to Bedrock geometry; a custom block is drafted in kebitx_drafts/ (it adds a block: Bedrock cannot reshape the game's own)",
                        ModelKind::Item => "model converted to Bedrock geometry; an attachable is drafted in kebitx_drafts/ (how it sits in the hand is not checked)",
                    };
                    (entry(Verdict::Adapts, None, false, reason), None, Some(m))
                }
                Err(why) => (entry(Verdict::Adapts, None, false, why), None, None),
            };
        }
        "models" => entry(
            Verdict::Adapts,
            None,
            false,
            by_change(
                "changed model: a Bedrock resource pack cannot reshape the game's blocks and items (a custom block or an attachable can)",
                "new model: becomes the geometry of a custom Bedrock block or item",
                "Java model: becomes Bedrock geometry or a block definition",
            ),
        ),
        "blockstates" => entry(
            Verdict::Adapts,
            None,
            false,
            by_change(
                "changed blockstate: the game's block states are fixed in Bedrock (a custom block's permutations can do it)",
                "new blockstate: becomes the permutations of a custom Bedrock block",
                "blockstate: becomes Bedrock block permutations",
            ),
        ),
        "items" => entry(
            Verdict::Adapts,
            None,
            false,
            by_change(
                "changed item definition: needs a Bedrock attachable or custom item",
                "new item definition: becomes a custom Bedrock item or attachable",
                "item definition: becomes a Bedrock item or attachable",
            ),
        ),
"optifine" if lower.contains("/cem/") && lower.ends_with(".jem") => {            let Some(bg) = ctx.refs.bedrock_geometry else {                return plain(entry(Verdict::Adapts, None, false, "OptiFine entity model: matching its parts needs the Bedrock game's geometry"));            };            return match crate::cem::inspect(&ctx.java_root.join(f), bg) {                Some(r) if r.geometry.is_some() => (                    entry(Verdict::Adapts, None, false, "OptiFine entity model: parts proven to be Bedrock bones are listed in kebitx_drafts/cem/; positions and animations are not converted"),                    None,                    Some(Make::Cem(r)),                ),                Some(r) => (                    entry(Verdict::Adapts, None, false, "OptiFine entity model: no part paints what one Bedrock bone paints (no geometry of that texture size, or no part proven)"),                    None,                    Some(Make::Cem(r)),                ),                None => plain(entry(Verdict::Adapts, None, false, "OptiFine entity model that cannot be read")),            };        }        "optifine" if lower.ends_with(".jpm") => entry(Verdict::Adapts, None, false, "OptiFine model part (.jpm): read with the .jem that names it"),        "optifine" => entry(Verdict::Adapts, None, false, "OptiFine feature (connected textures, item rules, skies, ...): no Bedrock equivalent"),
        "lang" => entry(
            Verdict::Adapts,
            None,
            false,
            "language file in the old .lang format: no rule",
        ),
        "textures" if lower.ends_with(".png.mcmeta") => {
            if is_animation(&ctx.java_root.join(f)) {
                entry(
                    Verdict::Adapts,
                    None,
                    false,
                    "texture animation of a texture that does not convert",
                )
            } else {
                entry(Verdict::Adapts, None, false, "texture settings (.mcmeta: mipmaps, blur, ...): rendering hints with no Bedrock file")
            }
        }
        "textures" if lower.ends_with(".png") => return texture(f, &parts[3..], ctx),
        "sounds" if lower.ends_with(".ogg") => {
            let target = format!("sounds/{}", parts[3..].join("/"));
            if ctx.refs.catalog.has_asset_stem(&asset_stem(&target)) {
                entry(
                    Verdict::Converts,
                    Some(target),
                    true,
                    "sound at the same path in Bedrock",
                )
            } else if used_sounds.is_some_and(|u| u.contains(f)) {
                entry(
                    Verdict::Converts,
                    Some(target),
                    true,
                    "sound a converted sound event plays",
                )
            } else {
                entry(
                    Verdict::Adapts,
                    None,
                    false,
                    "sound no converted sound event plays: needs a sound definition",
                )
            }
        }
        _ => entry(
            Verdict::Adapts,
            None,
            false,
            "no automatic rule yet (fonts, atlases, equipment, ...)",
        ),
    })
}

/// `assets/<ns>/models/block/...` or `.../item/...`.
fn model_kind(f: &str) -> Option<ModelKind> {
    let rest = f
        .strip_prefix("assets/")?
        .split_once('/')?
        .1
        .strip_prefix("models/")?;
    match rest.split('/').next()? {
        "block" => Some(ModelKind::Block),
        "item" => Some(ModelKind::Item),
        _ => None,
    }
}

fn convert_model(f: &str, kind: ModelKind, ctx: &Ctx) -> Result<Make, &'static str> {
    let model =
        crate::models::resolve(f, ctx.java_root, ctx.refs.java_jar).map_err(|_| match kind {
            ModelKind::Block => {
                "changed block model whose parents cannot all be read: needs a custom block"
            }
            ModelKind::Item => {
                "changed item model whose parents cannot all be read: needs an attachable"
            }
        })?;
    if model.elements.is_none() {
        return Err(match kind {
            ModelKind::Item => {
                "changed flat item model (a sprite, no cubes): points the item at other textures"
            }
            ModelKind::Block => {
                "changed block model without cubes of its own: needs a custom block"
            }
        });
    }
    let (ns, rest) = f
        .trim_start_matches("assets/")
        .split_once('/')
        .ok_or("bad model path")?;
    let rest = rest
        .trim_start_matches("models/")
        .trim_start_matches("block/")
        .trim_start_matches("item/");
    let flat = rest
        .trim_end_matches(".json")
        .replace(['/', '.', '-'], "_")
        .to_ascii_lowercase();
    let name = if ns == "minecraft" {
        flat
    } else {
        format!("{}_{flat}", ns.to_ascii_lowercase())
    };
    let id = format!("geometry.kebitx.{name}");
    let g = crate::models::to_geometry(&id, &model)?;
    let draft = ModelDraft {
        java: f.to_string(),
        kind,
        name,
        geometry: g.json,
        textures: BTreeMap::new(),
        unverified: g.unverified,
        culling: g.culling,
    };
    Ok(Make::Model(draft, g.materials))
}

/// The Bedrock texture (no extension) a Java texture id ends up as: the
/// pack's own converted file, else the game's file at that path or by a
/// confirmed rename.
fn bedrock_texture(java_id: &str, entries: &[Entry], catalog: &VanillaCatalog) -> Option<String> {
    let (ns, path) = java_id.split_once(':').unwrap_or(("minecraft", java_id));
    let file = format!("assets/{ns}/textures/{path}.png");
    if let Some(b) = entries
        .iter()
        .find(|e| e.java == file && e.verdict == Verdict::Converts)
        .and_then(|e| e.bedrock.as_ref())
    {
        return Some(asset_stem(b));
    }
    if ns != "minecraft" {
        return None;
    }
    let (kind, inner) = path.split_once('/')?;
    let folder = TEXTURE_FOLDERS.iter().find(|(j, _)| *j == kind)?.1;
    let exact = format!("textures/{folder}/{inner}");
    if catalog.has_asset_stem(&asset_stem(&exact)) {
        return Some(asset_stem(&exact));
    }
    names::rename(&asset_stem(path), catalog)
}

/// A pack file compared with the Java game's file at the same path.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InJava {
    /// The same bytes, or the same JSON written another way.
    Same,
    Changed,
    /// The game has no such file.
    New,
    /// No `.jar` to compare with.
    Unknown,
}

fn in_java(f: &str, ctx: &Ctx) -> InJava {
    let Some(jar) = ctx.refs.java_jar else {
        return InJava::Unknown;
    };
    if !jar.zip.contains(f) {
        return InJava::New;
    }
    let (Some(theirs), Ok(ours)) = (jar.read(f), std::fs::read(ctx.java_root.join(f))) else {
        return InJava::Unknown;
    };
    if theirs == ours {
        return InJava::Same;
    }
    let json = |b: &[u8]| kebitx_core::json::parse(&String::from_utf8_lossy(b)).ok();
    if f.ends_with(".json") && json(&theirs).is_some_and(|t| json(&ours) == Some(t)) {
        return InJava::Same;
    }
    InJava::Changed
}

/// How a texture found its Bedrock file.
#[derive(Clone, Copy, PartialEq)]
enum Found {
    Path,
    Rename,
    Pixels,
    Layout,
    Name,
}

/// The Java game's own picture for a pack texture at `f`, if it has one.
fn java_picture(f: &str, ctx: &Ctx) -> Option<Rgba> {
    let bytes = ctx.refs.java_jar?.read(f)?;
    content::decode(f, &bytes)
}

fn texture(f: &str, rest: &[&str], ctx: &Ctx) -> (Entry, Option<Json>, Option<Make>) {
    let entry = |verdict, bedrock: Option<String>, exact, reason| Entry {
        java: f.to_string(),
        verdict,
        bedrock,
        exact,
        reason,
    };
    let adapts = |reason| (entry(Verdict::Adapts, None, false, reason), None, None);
    let Some((&folder, inner)) = rest.split_first() else {
        return adapts("texture outside a folder: no rule");
    };
    let bedrock_folder = TEXTURE_FOLDERS
        .iter()
        .find(|(j, _)| *j == folder)
        .map(|(_, b)| *b);
    let animated =
        ctx.has(&format!("{f}.mcmeta")) && is_animation(&ctx.java_root.join(format!("{f}.mcmeta")));
    let catalog = ctx.refs.catalog;
    let java_rel = asset_stem(&rest.join("/"));

    let mut target: Option<(String, Found)> = None;
    if let Some(bf) = bedrock_folder {
        let exact = format!("textures/{bf}/{}", inner.join("/"));
        if catalog.has_asset_stem(&asset_stem(&exact)) {
            target = Some((exact, Found::Path));
        } else if let Some(stem) = names::rename(&java_rel, catalog) {
            target = Some((format!("{stem}.png"), Found::Rename));
        }
    }
    // the pixels: the Java game's picture is a Bedrock one, whole, cut or as
    // a cell of a sheet
    if target.is_none() {
        if let (Some(java), Some(jp), Some(bp)) = (
            java_picture(f, ctx),
            ctx.refs.java_pixels,
            ctx.refs.bedrock_pixels,
        ) {
            let name = asset_stem(rest.last().unwrap_or(&""));
            let candidates: Vec<String> = bedrock_folder
                .and_then(|bf| ctx.by_name.get(&(bf.to_string(), name)))
                .cloned()
                .unwrap_or_default();
            match content::prove(&java, folder, &candidates, jp, bp) {
                Some(Proof::Whole(stem)) => target = Some((format!("{stem}.png"), Found::Pixels)),
                Some(proof) if !animated => return region(f, ctx, &java, proof),
                _ => {
                    if let Some(stem) = content::prove_layout(&java, jp, bp) {
                        target = Some((format!("{stem}.png"), Found::Layout));
                    }
                }
            }
        }
    }
    if target.is_none() {
        if let Some(bf) = bedrock_folder {
            // the same file name, once and only once among this kind of texture
            let name = asset_stem(rest.last().unwrap_or(&""));
            if let Some([only]) = ctx.by_name.get(&(bf.to_string(), name)).map(Vec::as_slice) {
                if same_subject(inner, only) {
                    target = Some((format!("{only}.png"), Found::Name));
                }
            }
        }
    }
    let Some((target, found)) = target else {
        return adapts(if bedrock_folder.is_some() {
            "texture with no Bedrock file at that path or name: needs a name mapping"
        } else {
            "texture of a kind Bedrock organises differently (GUI, effects, ...): needs a mapping"
        });
    };
    let stem = asset_stem(&target);

    if animated {
        // an animation: the frames' shape is checked by the flipbook, not the
        // whole strip's (a Java strip may hold another number of frames)
        return match flipbook::flipbook(ctx.java_root, f, &stem, catalog) {
            Ok(fb) => (
                entry(
                    Verdict::Converts,
                    Some(target),
                    found == Found::Path,
                    "animated texture: copied, with a flipbook entry",
                ),
                Some(fb),
                None,
            ),
            Err(why) => adapts(why),
        };
    }
    // The same name is not the same picture if the canvas has another shape:
    // the faces are laid out differently, and copying it would scramble the
    // model. A uniform scale (an HD pack) keeps the shape and is fine.
    let java_size = kebitx_core::image::dimensions(&ctx.java_root.join(f));
    if let (Some((jw, jh)), Some((bw, bh))) = (java_size, catalog.texture_size(&stem)) {
        if jw as u64 * bh as u64 != bw as u64 * jh as u64 {
            // another layout: maybe Bedrock's picture is a region of Java's
            if let (Some(java), Some(jp), Some(bp)) = (
                java_picture(f, ctx),
                ctx.refs.java_pixels,
                ctx.refs.bedrock_pixels,
            ) {
                if let Some(proof @ Proof::Crop { .. }) =
                    content::prove(&java, folder, std::slice::from_ref(&stem), jp, bp)
                {
                    return region(f, ctx, &java, proof);
                }
            }
            return adapts(
                "same texture name, other proportions: the layout differs, needs remapping",
            );
        }
    }
    let reason = match found {
        Found::Path => "texture at the same path in Bedrock",
        Found::Rename => "texture renamed by the Java-to-Bedrock name table, confirmed by the game",
        Found::Pixels => {
            "texture proven by its pixels: the Java game's picture is Bedrock's at another path"
        }
        Found::Layout => "texture laid out as Bedrock's: what is drawn has the same shape (Bedrock redrew the colours)",
        Found::Name => "texture matched by its unique file name among its kind",
    };
    (
        entry(
            Verdict::Converts,
            Some(target),
            found == Found::Path,
            reason,
        ),
        None,
        None,
    )
}

/// A texture whose picture is a region of a Bedrock one, or the other way.
fn region(f: &str, ctx: &Ctx, java: &Rgba, proof: Proof) -> (Entry, Option<Json>, Option<Make>) {
    let entry = |verdict, bedrock: Option<String>, reason| Entry {
        java: f.to_string(),
        verdict,
        bedrock,
        exact: false,
        reason,
    };
    let adapts = |reason| (entry(Verdict::Adapts, None, reason), None, None);
    let Some(pack) = content::decode_file(&ctx.java_root.join(f)) else {
        return adapts("texture whose pixels cannot be read");
    };
    let Some(k) = content::scale_of((pack.width, pack.height), (java.width, java.height)) else {
        return adapts(
            "texture of another size than the Java game's: the region to cut is not known",
        );
    };
    if &pack == java {
        return (
            entry(
                Verdict::Converts,
                None,
                "the Java game's own picture: Bedrock already has it, nothing to write",
            ),
            None,
            None,
        );
    }
    match proof {
        Proof::Crop { stem, x, y, w, h } => {
            let Some(cut) = pack.crop(x * k, y * k, w * k, h * k) else {
                return adapts("texture whose region cannot be cut");
            };
            let target = format!("{stem}.png");
            (
                entry(
                    Verdict::Converts,
                    Some(target.clone()),
                    "texture cut to Bedrock's layout: the region proven by the game's pixels",
                ),
                None,
                Some(Make::Cut { target, img: cut }),
            )
        }
        Proof::Cell { stem, x, y } => {
            if k != 1 {
                return adapts(
                    "HD sprite of a shared sheet: the game's sheet would have to be scaled",
                );
            }
            (
                entry(
                    Verdict::Converts,
                    Some(format!("{stem}.png")),
                    "sprite pasted into Bedrock's sheet, at the cell its pixels prove",
                ),
                None,
                Some(Make::Paste {
                    sheet: stem,
                    x,
                    y,
                    img: pack,
                }),
            )
        }
        Proof::Whole(_) => adapts("unexpected proof"),
    }
}

/// Does this `.mcmeta` animate its texture? Unreadable counts as yes: an
/// animation that cannot be checked must not be copied as a still picture.
fn is_animation(mcmeta: &Path) -> bool {
    match read_json(mcmeta) {
        Some(j) => j.get("animation").is_some(),
        None => true,
    }
}

/// Whether a name match stays on the same subject. A Java texture directly in
/// its kind folder (`block/red_candle`) may land in any Bedrock subfolder
/// (`blocks/candles/`): those only organise. One in a subfolder names a
/// subject (`equipment/humanoid_leggings/gold`), and the Bedrock file must be
/// at the kind root, or under a folder that is that subject (`horse` ->
/// `horse2`, `shield` -> `shield_patterns`) or the file itself (`hoglin/zoglin`
/// -> `zoglin/zoglin`) — not a rabbit's `gold`.
fn same_subject(inner: &[&str], bedrock_stem: &str) -> bool {
    let Some(java_parent) = inner.len().checked_sub(2).map(|i| inner[i]) else {
        return true;
    };
    let parts: Vec<&str> = bedrock_stem.split('/').collect();
    if parts.len() <= 3 {
        return true; // textures/<kind>/<name>
    }
    let parent = parts[parts.len() - 2];
    let name = parts[parts.len() - 1];
    parent.starts_with(java_parent) || parent == name
}

/// A Bedrock file claimed by two Java files (`player/slim/alex` and
/// `player/wide/alex`) through anything weaker than its exact path is proven
/// for neither: both go back to needing a mapping. Generated files and the
/// sheets sprites are pasted into are shared on purpose.
fn drop_contested(entries: &mut [Entry], sheets: &HashSet<String>) {
    let shared = |b: &str| {
        [MANIFEST, FLIPBOOKS, SOUND_DEFINITIONS, LANGUAGES].contains(&b)
            || sheets.contains(b)
            || b.starts_with("texts/")
    };
    let mut claims: HashMap<String, usize> = HashMap::new();
    for e in entries.iter().filter(|e| e.verdict == Verdict::Converts) {
        if let Some(b) = e.bedrock.as_ref().filter(|b| !shared(b)) {
            *claims.entry(b.to_ascii_lowercase()).or_insert(0) += 1;
        }
    }
    for e in entries
        .iter_mut()
        .filter(|e| e.verdict == Verdict::Converts && !e.exact)
    {
        if e.bedrock
            .as_ref()
            .is_some_and(|b| claims.get(&b.to_ascii_lowercase()).is_some_and(|&n| n > 1))
        {
            *e = Entry {
                java: std::mem::take(&mut e.java),
                verdict: Verdict::Adapts,
                bedrock: None,
                exact: false,
                reason: "two Java textures share that Bedrock name: needs a mapping",
            };
        }
    }
}

/// A Java description: plain text, or a text component (`{"text": ...}`,
/// arrays of them).
fn description_text(v: &Json) -> String {
    match v {
        Json::Str(s) => s.clone(),
        Json::Arr(a) => a.iter().map(description_text).collect(),
        Json::Obj(_) => {
            let mut s = v
                .get("text")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string();
            if let Some(extra) = v.get("extra") {
                s.push_str(&description_text(extra));
            }
            s
        }
        _ => String::new(),
    }
}

/// A data-pack loot table.
fn loot_file(f: &str, parts: &[&str], ctx: &Ctx) -> (Entry, Option<Json>, Option<Make>) {
    use crate::loot;
    let entry = |verdict, bedrock: Option<String>, exact, reason| Entry {
        java: f.to_string(),
        verdict,
        bedrock,
        exact,
        reason,
    };
    let plain = |e| (e, None, None);
    let Some(refs) = ctx.refs.loot else {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "loot table: proving it needs both installed games' own tables",
        ));
    };
    let Some(table) = read_json(&ctx.java_root.join(f)).and_then(|j| loot::parse_java(&j)) else {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "loot table that cannot be read",
        ));
    };
    if !table.others().is_empty() || table.without_survives_explosion() != table {
        return plain(entry(
            Verdict::Adapts,
            None,
            false,
            "loot table using a function or condition with no proven Bedrock form (exploration maps, potions effects, ...)",
        ));
    }
    let bedrock = refs.renamed(&table);
    let known = |i: &String| {
        ctx.mod_ids.contains(i)
            || refs.known.contains(i)
            || ctx
                .refs
                .recipes
                .is_some_and(|r| r.known.contains(&crate::recipes::Alt::Item(i.clone())))
    };
    if !bedrock.items().iter().all(known) {
        return plain(entry(Verdict::Adapts, None, false, "loot table naming an item Bedrock's own data never names: the name must be mapped by a person"));
    }
    let (ns, rel) = (parts[1], parts[3..].join("/"));
    let path = if ns == "minecraft" {
        format!("loot_tables/{rel}")
    } else {
        format!("loot_tables/{ns}/{rel}")
    };
    let bedrock_rel = path.trim_start_matches("loot_tables/").to_string();
    if refs
        .bedrock
        .get(&bedrock_rel)
        .is_some_and(|b| loot::same_as_bedrock(&bedrock, b))
    {
        return plain(entry(
            Verdict::Converts,
            None,
            true,
            "a loot table Bedrock already has, the same: nothing to write",
        ));
    }
    let reason = if ns != "minecraft" {
        "new loot table converted: used by nothing until a block or entity names it"
    } else if refs.same.contains(&bedrock_rel) {
        "changed loot table: replaces Bedrock's at the same path (the Java original means Bedrock's)"
    } else if refs.bedrock.contains_key(&bedrock_rel) {
        "changed loot table: replaces Bedrock's at the same path (the same path only: the originals differ)"
    } else {
        "loot table converted at a path Bedrock does not use: check what should drop it"
    };
    (
        entry(
            Verdict::Converts,
            Some(format!("{BEHAVIOR}/{path}")),
            true,
            reason,
        ),
        None,
        Some(Make::Behavior(vec![(path, loot::to_bedrock(&bedrock))])),
    )
}
