//! Mods' items and blocks → Bedrock custom items and blocks.
//!
//! A Java mod is code plus data. Its code (`.class`) is behaviour to
//! reimplement: a task list, never claimed as automatic. Its data says what
//! its simple items and blocks LOOK like, and that converts:
//!
//! - a flat item (`item/generated`, `item/handheld`, one layer) becomes a
//!   custom item with `minecraft:icon`, `minecraft:display_name` (a language
//!   key, as Bedrock's own apple does) and, held like a tool,
//!   `minecraft:hand_equipped` — components Bedrock's own items use;
//! - a block without states becomes a custom block whose geometry is its
//!   Java model, converted with the rules of Mojang's own conversion (culling
//!   from `cullface` included), with `minecraft:material_instances` and, when
//!   the mod's loot table converts, `minecraft:loot` — components Bedrock's
//!   own blocks use. `render_method` is `alpha_test` only when the texture
//!   has transparent pixels.
//!
//! Blocks with states (facing, age, ...) need permutations; items with cubes
//! need attachables: both go to the task list.

use crate::java::JavaJar;
use crate::models;
use kebitx_core::json::{Json, JsonMap};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Components Bedrock's own item and block definitions use.
#[derive(Default)]
pub struct BedrockDefs {
    pub item_components: HashSet<String>,
    pub block_components: HashSet<String>,
}

impl BedrockDefs {
    pub fn from_install(install: &Path) -> Self {
        use kebitx_core::vanilla::files::{map_files_in, PackKind};
        let files = map_files_in(
            install,
            PackKind::Behavior,
            |p| p.starts_with("items/") || p.starts_with("blocks/"),
            |_, b| kebitx_core::json::parse(&String::from_utf8_lossy(b)).ok(),
        );
        let mut d = BedrockDefs::default();
        for j in files.values() {
            for (root, set) in [
                ("minecraft:item", &mut d.item_components),
                ("minecraft:block", &mut d.block_components),
            ] {
                if let Some(c) = j
                    .get(root)
                    .and_then(|i| i.get("components"))
                    .and_then(Json::as_object)
                {
                    set.extend(c.keys().cloned());
                }
            }
        }
        d
    }
}

/// What the adapters made.
#[derive(Debug, Default)]
pub struct ModOutput {
    /// behavior-pack path -> content
    pub behavior: Vec<(String, Json)>,
    /// resource-pack path -> content (geometry, culling)
    pub resource: Vec<(String, Json)>,
    /// item atlas key -> texture path
    pub item_atlas: BTreeMap<String, String>,
    /// terrain atlas key -> texture path
    pub terrain: BTreeMap<String, String>,
    /// Java language code -> Bedrock key -> text
    pub texts: BTreeMap<String, BTreeMap<String, String>>,
    /// pack file -> (Bedrock target to copy it to, or none; reason)
    pub converted: BTreeMap<String, (Option<String>, &'static str)>,
    /// pack file -> why it stays for a person (more precise than the default)
    pub adapted: BTreeMap<String, &'static str>,
    /// what a person has to do
    pub tasks: Vec<String>,
    /// (block definition, the mod's loot table for it): linked once the
    /// table is known to convert
    pub block_loot: Vec<(String, String)>,
    /// drafts for a person to check and move: path under `kebitx_drafts/` -> content
    pub drafts: Vec<(String, Json)>,
    /// 3D item models to draft as attachables
    pub model_items: Vec<String>,
}

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

fn obj(e: Vec<(&str, Json)>) -> Json {
    Json::Obj(JsonMap::from_entries(
        e.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
    ))
}

/// `demo:item/ruby` -> `assets/demo/textures/item/ruby.png`.
fn texture_file(id: &str) -> String {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("assets/{ns}/textures/{path}.png")
}

/// Does the parent chain of the model at `path` pass through `needle`?
fn chain_has(path: &str, root: &Path, jar: Option<&JavaJar>, needle: &str) -> bool {
    let mut next = Some(path.to_string());
    for _ in 0..32 {
        let Some(p) = next.take() else { return false };
        let text = std::fs::read(root.join(&p))
            .ok()
            .or_else(|| jar.and_then(|j| j.read(&p)));
        let Some(parent) = text
            .and_then(|t| kebitx_core::json::parse(&String::from_utf8_lossy(&t)).ok())
            .and_then(|j| j.get("parent").and_then(Json::as_str).map(str::to_string))
        else {
            return false;
        };
        if parent.trim_start_matches("minecraft:") == needle {
            return true;
        }
        next = models::model_path(&parent);
    }
    false
}

fn has_transparency(root: &Path, file: &str) -> bool {
    crate::content::decode_file(&root.join(file))
        .is_some_and(|img| img.pixels.chunks_exact(4).any(|p| p[3] < 255))
}

/// Adapt the items and blocks of namespace `ns`.
pub fn adapt(
    ns: &str,
    root: &Path,
    files: &[String],
    jar: Option<&JavaJar>,
    defs: &BedrockDefs,
    out: &mut ModOutput,
) {
    let has = |p: &str| files.binary_search_by(|f| f.as_str().cmp(p)).is_ok();
    let lang: Vec<(String, Json)> = files
        .iter()
        .filter(|f| f.starts_with(&format!("assets/{ns}/lang/")) && f.ends_with(".json"))
        .filter_map(|f| {
            let code = f.rsplit('/').next()?.trim_end_matches(".json").to_string();
            Some((
                code,
                kebitx_core::json::parse(&kebitx_core::filesystem::read_text(&root.join(f)).ok()?)
                    .ok()?,
            ))
        })
        .collect();
    let name_lines = |java_key: &str, bedrock_key: &str, out: &mut ModOutput| {
        for (code, j) in &lang {
            if let Some(text) = j
                .get(java_key)
                .and_then(Json::as_str)
                .filter(|t| !t.contains(['\n', '\r']))
            {
                out.texts
                    .entry(code.clone())
                    .or_default()
                    .insert(bedrock_key.to_string(), text.to_string());
            }
        }
    };
    let comp_ok = |set: &HashSet<String>, names: &[&str]| names.iter().all(|n| set.contains(*n));

    // ---- blocks: those without states
    let mut block_names = HashSet::new();
    for f in files
        .iter()
        .filter(|f| f.starts_with(&format!("assets/{ns}/blockstates/")) && f.ends_with(".json"))
    {
        let name = f
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim_end_matches(".json")
            .to_string();
        block_names.insert(name.clone());
        let Some(state) = kebitx_core::filesystem::read_text(&root.join(f))
            .ok()
            .and_then(|t| kebitx_core::json::parse(&t).ok())
        else {
            continue;
        };
        let variants = state.get("variants").and_then(Json::as_object);
        let single = match variants {
            Some(v) if v.len() == 1 && v.keys().next().is_some_and(|k| k.is_empty()) => {
                v.values().next().cloned()
            }
            _ => None,
        };
        let model_id = single.as_ref().and_then(|v| match v {
            Json::Arr(a) if a.len() == 1 => {
                a[0].get("model").and_then(Json::as_str).map(str::to_string)
            }
            Json::Obj(_) if v.get("x").is_none() && v.get("y").is_none() => {
                v.get("model").and_then(Json::as_str).map(str::to_string)
            }
            _ => None,
        });
        let Some(model_id) = model_id else {
            // states: drafted with permutations, as Bedrock's own shelf mushroom
            let drafted = variants.map(|v| stateful_block(ns, &name, v, root, &has, jar, out));
            match drafted {
                Some(Ok(def)) => {
                    out.drafts
                        .push((format!("behavior_pack/blocks/{ns}_{name}.json"), def));
                    out.tasks.push(format!(
                        "block {ns}:{name}: drafted with permutations in kebitx_drafts/behavior_pack/blocks/ \
                         (conditions and rotations as Bedrock's own shelf mushroom writes them; its states \
                         declaration is Bedrock's documented form, which no definition of the game shows: check it, \
                         then move it to the behavior pack; Java changes these states in code: reimplement that)"
                    ));
                    out.adapted.insert(f.clone(), "mod block with states: drafted with permutations (kebitx_drafts/), to check and move");
                }
                Some(Err(why)) => {
                    out.tasks
                        .push(format!("block {ns}:{name}: has states, not drafted: {why}"));
                    out.adapted.insert(
                        f.clone(),
                        "mod block with states that has no Bedrock form here (see mod_tasks.txt)",
                    );
                }
                None => {
                    out.tasks.push(format!(
                        "block {ns}:{name}: a multipart blockstate: needs a person"
                    ));
                    out.adapted.insert(
                        f.clone(),
                        "mod block with a multipart blockstate: needs a person",
                    );
                }
            }
            continue;
        };
        let Some(model_file) = models::model_path(&model_id) else {
            continue;
        };
        let Ok(model) = models::resolve(&model_file, root, jar) else {
            out.tasks.push(format!(
                "block {ns}:{name}: its model {model_id} cannot be read"
            ));
            continue;
        };
        let flat = format!("{ns}_{name}");
        let geo_id = format!("geometry.kebitx.{flat}");
        let Ok(g) = models::to_geometry(&geo_id, &model) else {
            out.tasks
                .push(format!("block {ns}:{name}: its model has no cubes"));
            continue;
        };
        if g.unverified > 0 {
            out.tasks.push(format!(
                "block {ns}:{name}: {} rotated element(s): check their angle in game",
                g.unverified
            ));
        }
        // every texture must be the mod's own file
        let mut instances = Vec::new();
        let mut missing = false;
        for (instance, tex) in &g.materials {
            let file = texture_file(tex);
            if tex.is_empty() || !has(&file) {
                missing = true;
                continue;
            }
            let rel = file
                .trim_start_matches(&format!("assets/{ns}/textures/"))
                .trim_end_matches(".png")
                .to_string();
            let target = format!("textures/blocks/{ns}/{}", rel.trim_start_matches("block/"));
            let tile = format!("{flat}_{instance}");
            out.terrain.insert(tile.clone(), target.clone());
            out.converted.insert(
                file.clone(),
                (
                    Some(format!("{target}.png")),
                    "mod texture, used by a converted custom block",
                ),
            );
            let mut m = vec![("texture", s(&tile))];
            if has_transparency(root, &file) {
                m.push(("render_method", s("alpha_test")));
            }
            instances.push((instance.clone(), obj(m)));
        }
        if missing || instances.is_empty() {
            out.tasks.push(format!(
                "block {ns}:{name}: a texture of its model is not in the mod"
            ));
            continue;
        }
        if let Some((_, first)) = instances.first().cloned() {
            instances.push(("*".into(), first));
        }
        let mut geometry = vec![("identifier", s(&geo_id))];
        if !g.culling.is_empty() {
            let culling_id = format!("kebitx:culling.{flat}");
            out.resource.push((
                format!("block_culling/kebitx_{flat}.culling.json"),
                models::culling_file(&culling_id, &g.culling),
            ));
            geometry.push(("culling", s(&culling_id)));
        }
        let components = vec![
            ("minecraft:geometry", obj(geometry)),
            (
                "minecraft:material_instances",
                Json::Obj(JsonMap::from_entries(instances)),
            ),
        ];
        let names: Vec<&str> = components.iter().map(|(k, _)| *k).collect();
        if !comp_ok(&defs.block_components, &names) {
            out.tasks.push(format!(
                "block {ns}:{name}: a component Bedrock's own blocks do not use"
            ));
            continue;
        }
        out.resource
            .push((format!("models/blocks/kebitx_{flat}.geo.json"), g.json));
        out.block_loot.push((
            format!("blocks/{flat}.json"),
            format!("data/{ns}/loot_table/blocks/{name}.json"),
        ));
        out.behavior.push((
            format!("blocks/{flat}.json"),
            obj(vec![
                ("format_version", s("1.21.0")),
                (
                    "minecraft:block",
                    obj(vec![
                        (
                            "description",
                            obj(vec![
                                ("identifier", s(&format!("{ns}:{name}"))),
                                ("menu_category", obj(vec![("category", s("construction"))])),
                            ]),
                        ),
                        ("components", obj(components)),
                    ]),
                ),
            ]),
        ));
        name_lines(
            &format!("block.{ns}.{name}"),
            &format!("tile.{ns}:{name}.name"),
            out,
        );
        out.converted.insert(
            f.clone(),
            (
                None,
                "mod block: a Bedrock custom block, its geometry from its model",
            ),
        );
        out.converted.insert(
            model_file,
            (
                None,
                "mod block model: converted to the custom block's geometry",
            ),
        );
    }

    // ---- items: flat ones
    for f in files
        .iter()
        .filter(|f| f.starts_with(&format!("assets/{ns}/models/item/")) && f.ends_with(".json"))
    {
        let name = f
            .trim_start_matches(&format!("assets/{ns}/models/item/"))
            .trim_end_matches(".json")
            .to_string();
        if name.contains('/') {
            continue;
        }
        let Ok(model) = models::resolve(f, root, jar) else {
            continue;
        };
        if model.elements.is_some() || !model.generated {
            if block_names.contains(&name) {
                out.converted.insert(
                    f.clone(),
                    (
                        None,
                        "block item model: Bedrock gives a custom block its item",
                    ),
                );
            } else {
                // drafted as an attachable, like a changed vanilla item model;
                // its textures go with it
                for tex in model.textures.values().filter(|t| !t.starts_with('#')) {
                    let file = texture_file(tex);
                    if has(&file) {
                        let rel = file
                            .trim_start_matches(&format!("assets/{ns}/textures/"))
                            .trim_end_matches(".png")
                            .to_string();
                        let target = format!(
                            "textures/items/{ns}/{}.png",
                            rel.trim_start_matches("item/")
                        );
                        out.converted.insert(
                            file,
                            (Some(target), "mod texture, used by a drafted attachable"),
                        );
                    }
                }
                out.model_items.push(f.clone());
                out.tasks.push(format!("item {ns}:{name}: a 3D model: an attachable is drafted in kebitx_drafts/ (how it sits in the hand is not checked)"));
                out.adapted.insert(
                    f.clone(),
                    "mod item with a 3D model: an attachable is drafted in kebitx_drafts/",
                );
            }
            continue;
        }
        if model.textures.contains_key("layer1") {
            out.tasks.push(format!(
                "item {ns}:{name}: several layers (tinted?): needs a person"
            ));
            continue;
        }
        let Some(tex) = model.textures.get("layer0") else {
            continue;
        };
        let file = texture_file(tex);
        if !has(&file) {
            out.tasks.push(format!(
                "item {ns}:{name}: its texture {tex} is not in the mod"
            ));
            continue;
        }
        let flat = format!("{ns}_{name}");
        let rel = file
            .trim_start_matches(&format!("assets/{ns}/textures/"))
            .trim_end_matches(".png")
            .to_string();
        let target = format!("textures/items/{ns}/{}", rel.trim_start_matches("item/"));
        let key = format!("item.{ns}.{name}.name");
        let mut components = vec![
            (
                "minecraft:icon",
                obj(vec![("textures", obj(vec![("default", s(&flat))]))]),
            ),
            ("minecraft:display_name", obj(vec![("value", s(&key))])),
        ];
        if chain_has(f, root, jar, "item/handheld") {
            components.push(("minecraft:hand_equipped", Json::Bool(true)));
        }
        let names: Vec<&str> = components.iter().map(|(k, _)| *k).collect();
        if !comp_ok(&defs.item_components, &names) {
            out.tasks.push(format!(
                "item {ns}:{name}: a component Bedrock's own items do not use"
            ));
            continue;
        }
        out.item_atlas.insert(flat.clone(), target.clone());
        out.converted.insert(
            file.clone(),
            (
                Some(format!("{target}.png")),
                "mod texture, used by a converted custom item",
            ),
        );
        out.behavior.push((
            format!("items/{flat}.json"),
            obj(vec![
                ("format_version", s("1.21.0")),
                (
                    "minecraft:item",
                    obj(vec![
                        (
                            "description",
                            obj(vec![
                                ("identifier", s(&format!("{ns}:{name}"))),
                                ("menu_category", obj(vec![("category", s("items"))])),
                            ]),
                        ),
                        ("components", obj(components)),
                    ]),
                ),
            ]),
        ));
        name_lines(&format!("item.{ns}.{name}"), &key, out);
        out.converted
            .insert(f.clone(), (None, "mod item: a Bedrock custom item"));
    }
}

/// The `item_texture.json` / `terrain_texture.json` text.
pub fn atlas_file(name: &str, entries: &BTreeMap<String, String>) -> Json {
    let data: Vec<(String, Json)> = entries
        .iter()
        .map(|(k, v)| (k.clone(), obj(vec![("textures", s(v))])))
        .collect();
    let mut e = vec![
        ("resource_pack_name", s("kebitx")),
        ("texture_name", s(name)),
    ];
    if name == "atlas.terrain" {
        e.push(("padding", Json::Num(8.0)));
        e.push(("num_mip_levels", Json::Num(4.0)));
    }
    e.push(("texture_data", Json::Obj(JsonMap::from_entries(data))));
    obj(e)
}

impl ModOutput {
    /// The mod's language files feed the names written: mark them.
    pub fn mark_languages(&mut self, files: &[String]) {
        if self.texts.is_empty() {
            return;
        }
        for f in files.iter().filter(|f| {
            f.contains("/lang/") && f.ends_with(".json") && !f.starts_with("assets/minecraft/")
        }) {
            self.converted.insert(
                f.clone(),
                (
                    None,
                    "mod language file: the names of its converted items and blocks",
                ),
            );
        }
    }
}

/// One block model as a geometry component and material instances: its
/// geometry and culling files go to `out.resource`, its textures to the
/// terrain atlas. `flat` names the geometry.
fn block_geometry(
    ns: &str,
    flat: &str,
    model_id: &str,
    root: &Path,
    has: &dyn Fn(&str) -> bool,
    jar: Option<&JavaJar>,
    out: &mut ModOutput,
) -> Result<(Json, Json), String> {
    let model_file = models::model_path(model_id).ok_or("bad model id")?;
    let model = models::resolve(&model_file, root, jar)
        .map_err(|_| format!("its model {model_id} cannot be read"))?;
    let geo_id = format!("geometry.kebitx.{flat}");
    let g = models::to_geometry(&geo_id, &model)
        .map_err(|_| format!("its model {model_id} has no cubes"))?;
    if g.unverified > 0 {
        out.tasks.push(format!(
            "model {model_id}: {} rotated element(s): check their angle in game",
            g.unverified
        ));
    }
    let mut instances = Vec::new();
    for (instance, tex) in &g.materials {
        let file = texture_file(tex);
        if tex.is_empty() || !has(&file) {
            return Err(format!("a texture of {model_id} is not in the mod"));
        }
        let rel = file
            .trim_start_matches(&format!("assets/{ns}/textures/"))
            .trim_end_matches(".png")
            .to_string();
        let target = format!("textures/blocks/{ns}/{}", rel.trim_start_matches("block/"));
        let tile = format!("{flat}_{instance}");
        out.terrain.insert(tile.clone(), target.clone());
        out.converted.insert(
            file.clone(),
            (
                Some(format!("{target}.png")),
                "mod texture, used by a converted custom block",
            ),
        );
        let mut m = vec![("texture", s(&tile))];
        if has_transparency(root, &file) {
            m.push(("render_method", s("alpha_test")));
        }
        instances.push((instance.clone(), obj(m)));
    }
    if let Some((_, first)) = instances.first().cloned() {
        instances.push(("*".into(), first));
    }
    let mut geometry = vec![("identifier", s(&geo_id))];
    if !g.culling.is_empty() {
        let culling_id = format!("kebitx:culling.{flat}");
        out.resource.push((
            format!("block_culling/kebitx_{flat}.culling.json"),
            models::culling_file(&culling_id, &g.culling),
        ));
        geometry.push(("culling", s(&culling_id)));
    }
    out.resource
        .push((format!("models/blocks/kebitx_{flat}.geo.json"), g.json));
    out.converted.insert(
        model_file,
        (
            None,
            "mod block model: converted to the custom block's geometry",
        ),
    );
    Ok((obj(geometry), Json::Obj(JsonMap::from_entries(instances))))
}

const CARDINAL: [&str; 4] = ["north", "south", "east", "west"];

/// One variant: its states, its model, its rotation about y.
type Case = (Vec<(String, String)>, String, u32);

/// A block with states, drafted with permutations as Bedrock's own
/// `shelf_mushroom` writes them (its Java blockstate is the pair): Java's
/// `facing` is `minecraft:cardinal_direction`, a variant's `y` is the
/// rotation `[0, (360 - y) mod 360, 0]`, each combination of states one
/// permutation with its geometry. How custom states are DECLARED
/// (`description.states`, `traits`) no definition of the game shows: that
/// part is Bedrock's documented form, to check.
pub fn stateful_block(
    ns: &str,
    name: &str,
    variants: &JsonMap,
    root: &Path,
    has: &dyn Fn(&str) -> bool,
    jar: Option<&JavaJar>,
    out: &mut ModOutput,
) -> Result<Json, String> {
    // property -> values, in the order they appear
    let mut props: Vec<(String, Vec<String>)> = Vec::new();
    let mut cases: Vec<Case> = Vec::new();
    for (key, v) in variants.iter() {
        let v = match v {
            Json::Arr(a) if a.len() == 1 => &a[0],
            Json::Obj(_) => v,
            _ => return Err("random models (a list per state): no Bedrock form".into()),
        };
        let model = v
            .get("model")
            .and_then(Json::as_str)
            .ok_or("a variant without a model")?
            .to_string();
        let y = match v.get("y") {
            None => 0,
            Some(Json::Num(n)) if [0.0, 90.0, 180.0, 270.0].contains(n) => *n as u32,
            _ => return Err("a rotation other than 0/90/180/270 about y".into()),
        };
        if v.get("x")
            .is_some_and(|x| !matches!(x, Json::Num(n) if *n == 0.0))
        {
            return Err("a rotation about x: Mojang's pair shows only y".into());
        }
        if matches!(v.get("uvlock"), Some(Json::Bool(true))) && y != 0 {
            return Err(
                "uvlock with a rotation: Bedrock rotates the textures with the block".into(),
            );
        }
        let state: Vec<(String, String)> = key
            .split(',')
            .filter(|kv| !kv.is_empty())
            .map(|kv| {
                kv.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .ok_or("bad variant key")
            })
            .collect::<Result<_, _>>()?;
        for (k, val) in &state {
            match props.iter_mut().find(|(p, _)| p == k) {
                Some((_, vals)) if !vals.contains(val) => vals.push(val.clone()),
                Some(_) => {}
                None => props.push((k.clone(), vec![val.clone()])),
            }
        }
        cases.push((state, model, y));
    }
    let combos: usize = props.iter().map(|(_, v)| v.len()).product();
    if cases.is_empty() || cases.len() != combos {
        return Err("variants that do not cover every combination of states".into());
    }
    let is_cardinal = |p: &str, vals: &[String]| {
        p == "facing" && vals.iter().all(|v| CARDINAL.contains(&v.as_str()))
    };
    let state_name = |p: &str| -> String {
        match props.iter().find(|(k, _)| k == p) {
            Some((k, vals)) if is_cardinal(k, vals) => "minecraft:cardinal_direction".into(),
            _ => format!("{ns}:{p}"),
        }
    };
    let typed = |v: &str| -> (Json, String) {
        if let Ok(n) = v.parse::<i64>() {
            (Json::Num(n as f64), n.to_string())
        } else if v == "true" || v == "false" {
            (Json::Bool(v == "true"), v.to_string())
        } else {
            (s(v), format!("'{v}'"))
        }
    };
    // one geometry per distinct model
    let mut geometries: BTreeMap<String, (Json, Json)> = BTreeMap::new();
    for (_, model, _) in &cases {
        if !geometries.contains_key(model) {
            let flat = format!("{ns}_{}", model.rsplit(['/', ':']).next().unwrap_or(model));
            let g = block_geometry(ns, &flat, model, root, has, jar, out)?;
            geometries.insert(model.clone(), g);
        }
    }
    let mut permutations = Vec::new();
    if props.is_empty() {
        // one model, rotated: the rotation is the block's own, no states
        let (geometry, materials) = geometries[&cases[0].1].clone();
        let mut components = vec![
            ("minecraft:geometry", geometry),
            ("minecraft:material_instances", materials),
        ];
        let y = cases[0].2;
        if y != 0 {
            let rotation = Json::Arr(vec![
                Json::Num(0.0),
                Json::Num(bedrock_rotation(y) as f64),
                Json::Num(0.0),
            ]);
            components.push((
                "minecraft:transformation",
                obj(vec![("rotation", rotation)]),
            ));
        }
        return Ok(obj(vec![
            ("format_version", s("1.21.0")),
            (
                "minecraft:block",
                obj(vec![
                    (
                        "description",
                        obj(vec![
                            ("identifier", s(&format!("{ns}:{name}"))),
                            ("menu_category", obj(vec![("category", s("construction"))])),
                        ]),
                    ),
                    ("components", obj(components)),
                ]),
            ),
        ]));
    }
    for (state, model, y) in &cases {
        let condition: Vec<String> = state
            .iter()
            .map(|(k, v)| format!("query.block_state('{}')=={}", state_name(k), typed(v).1))
            .collect();
        let (geometry, materials) = geometries[model].clone();
        let mut components = vec![
            ("minecraft:geometry", geometry),
            ("minecraft:material_instances", materials),
        ];
        if *y != 0 {
            let rotation = Json::Arr(vec![
                Json::Num(0.0),
                Json::Num(bedrock_rotation(*y) as f64),
                Json::Num(0.0),
            ]);
            components.push((
                "minecraft:transformation",
                obj(vec![("rotation", rotation)]),
            ));
        }
        permutations.push(obj(vec![
            ("condition", s(&condition.join("&&"))),
            ("components", obj(components)),
        ]));
    }
    let mut states = Vec::new();
    let mut traits = Vec::new();
    for (p, vals) in &props {
        if is_cardinal(p, vals) {
            let enabled = obj(vec![(
                "enabled_states",
                Json::Arr(vec![s("minecraft:cardinal_direction")]),
            )]);
            traits.push(("minecraft:placement_direction".to_string(), enabled));
        } else {
            states.push((
                format!("{ns}:{p}"),
                Json::Arr(vals.iter().map(|v| typed(v).0).collect()),
            ));
        }
    }
    let (base_geometry, base_materials) = geometries[&cases[0].1].clone();
    let mut description = vec![
        ("identifier", s(&format!("{ns}:{name}"))),
        ("menu_category", obj(vec![("category", s("construction"))])),
    ];
    if !states.is_empty() {
        description.push(("states", Json::Obj(JsonMap::from_entries(states))));
    }
    if !traits.is_empty() {
        description.push(("traits", Json::Obj(JsonMap::from_entries(traits))));
    }
    Ok(obj(vec![
        ("format_version", s("1.21.0")),
        (
            "minecraft:block",
            obj(vec![
                ("description", obj(description)),
                (
                    "components",
                    obj(vec![
                        ("minecraft:geometry", base_geometry),
                        ("minecraft:material_instances", base_materials),
                    ]),
                ),
                ("permutations", Json::Arr(permutations)),
            ]),
        ),
    ]))
}

/// A Java blockstate's `y` as Bedrock's `minecraft:transformation` rotation
/// about y — the rule Bedrock's own `shelf_mushroom` follows for every
/// direction of its Java blockstate (`adapters_real` test).
pub fn bedrock_rotation(java_y: u32) -> u32 {
    (360 - java_y % 360) % 360
}
