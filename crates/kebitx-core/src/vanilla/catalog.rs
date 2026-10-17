//! Builds the catalog of what the game itself provides.
//!
//! Both loose files and `.brarchive` bundles are read, because a single install
//! mixes the two: the base layer is packed, the patch layers are loose.

mod cache;

use super::brarchive::{self, Brarchive};
use crate::filesystem::{asset_stem, is_asset};
use crate::json::{self, Json};
use crate::par::par_map;
use crate::parser::material::material_names;
use crate::parser::render_controller::{parse_render_controller, RenderControllerDef};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq)]
pub struct VanillaCatalog {
    /// Pack-relative paths WITH extension, lowercased: `textures/entity/cow/cow.png`.
    /// Lowercased on purpose — the catalog only answers "does the game provide
    /// this", never "how is it spelled".
    textures: HashSet<String>,
    /// Identifier space: `geometry.cow`, `animation.cow.walk`,
    /// `controller.animation.cow.move`, `controller.render.cow`.
    geometries: HashSet<String>,
    animations: HashSet<String>,
    animation_controllers: HashSet<String>,
    render_controllers: HashSet<String>,
    /// Bodies of the render controllers the game defines in files, one per
    /// layer that defines it (patch layers redefine base ones).
    render_defs: HashMap<String, Vec<RenderControllerDef>>,
    /// Texture and sound paths, lowercased and WITHOUT extension, that the game
    /// ships or that its own files name. See `asset_stem`.
    asset_stems: HashSet<String>,
    /// Width and height of the game's textures, by asset stem.
    texture_sizes: HashMap<String, (u32, u32)>,
    /// Texture stem -> the terrain-atlas tile of the game's own flipbook that
    /// animates it (`textures/flipbook_textures.json`).
    flipbooks: HashMap<String, TileRef>,
    /// Texture stem -> every terrain-atlas tile that shows it
    /// (`textures/terrain_texture.json`).
    terrain_tiles: HashMap<String, Vec<TileRef>>,
    /// Texture stems the item atlas shows (`textures/item_texture.json`).
    item_atlas: HashSet<String>,
    /// Sound event -> its definition in the game's `sound_definitions.json`, as
    /// JSON text (later layers replace earlier ones, as in the game).
    sound_defs: HashMap<String, String>,
    /// Material names (lowercased) the game defines in `materials/*.material`,
    /// or that its own entities use.
    materials: HashSet<String>,
    pub version: String,
    pub bundles_read: usize,
    pub bundles_bad: usize,
    /// Loaded from the on-disk cache instead of read from the game. Set by
    /// `discover` after loading; never stored.
    pub from_cache: bool,
}

impl VanillaCatalog {
    /// Empty catalog: every lookup says "the game does not provide it". Used when
    /// the install cannot be found, so behaviour degrades to the old one instead
    /// of inventing answers.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The installed game's catalog, from the on-disk cache when it is still
    /// valid (see `cache`), built and stored otherwise. `None` if no install
    /// is found. `KEBITX_NO_CACHE=1` always rebuilds.
    pub fn discover() -> Option<Self> {
        let install = super::install_dir()?;
        let use_cache = std::env::var_os("KEBITX_NO_CACHE").is_none();
        if use_cache {
            if let Some(mut c) = cache::load(&install) {
                c.from_cache = true;
                return Some(c);
            }
        }
        let c = Self::from_install(&install);
        if use_cache {
            // best effort: a cache that cannot be written only costs time
            let _ = cache::store(&install, &c);
        }
        Some(c)
    }

    pub fn from_install(install: &Path) -> Self {
        let mut c = Self {
            version: super::game_version(install),
            ..Default::default()
        };
        // Resource packs only. Everything the catalog answers — textures,
        // geometry, animations, controllers — is resolved by the CLIENT, and the
        // client never reads behavior packs: a behavior-pack animation
        // controller must not make a client reference look satisfied.
        //
        // Every loose file and every bundle is a source; each is read into a
        // partial catalog on its own core, and the partials are merged in the
        // order the sources were listed (layer by layer, loose files first) —
        // the same order the sequential version read them in.
        let mut sources = Vec::new();
        for layer in super::files::ordered_layers(install) {
            for file in loose_files(&layer) {
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
        for part in par_map(&sources, |s| {
            let mut part = Self::default();
            match s {
                Source::Loose { layer, file } => part.add_loose_file(layer, file),
                Source::Bundle { layer, bundle } => part.add_bundle(layer, bundle),
            }
            part
        }) {
            c.merge(part);
        }
        c
    }

    /// Fold a partial catalog into this one.
    fn merge(&mut self, o: VanillaCatalog) {
        self.textures.extend(o.textures);
        self.geometries.extend(o.geometries);
        self.animations.extend(o.animations);
        self.animation_controllers.extend(o.animation_controllers);
        self.render_controllers.extend(o.render_controllers);
        for (id, defs) in o.render_defs {
            self.render_defs.entry(id).or_default().extend(defs);
        }
        self.asset_stems.extend(o.asset_stems);
        self.texture_sizes.extend(o.texture_sizes);
        self.flipbooks.extend(o.flipbooks);
        for (stem, tiles) in o.terrain_tiles {
            let mine = self.terrain_tiles.entry(stem).or_default();
            for t in tiles {
                if !mine.contains(&t) {
                    mine.push(t);
                }
            }
        }
        self.item_atlas.extend(o.item_atlas);
        self.sound_defs.extend(o.sound_defs);
        self.materials.extend(o.materials);
        self.bundles_read += o.bundles_read;
        self.bundles_bad += o.bundles_bad;
    }

    /// Every definition the game has of a render controller; empty if it is
    /// engine-provided (named by vanilla entities, defined in no file).
    pub fn render_controller_defs(&self, id: &str) -> &[RenderControllerDef] {
        self.render_defs.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Does the game ship or name this texture/sound path? Takes an
    /// `asset_stem` (lowercased, no extension): a pack's `.png` replaces the
    /// game's `.tga` at the same path.
    pub fn has_asset_stem(&self, stem: &str) -> bool {
        self.asset_stems.contains(stem)
    }

    /// Every texture and sound stem the game ships or names (lowercased, no
    /// extension), in no particular order.
    pub fn asset_stems(&self) -> impl Iterator<Item = &String> {
        self.asset_stems.iter()
    }

    /// Size of one of the game's textures, by asset stem (`textures/blocks/stone`).
    pub fn texture_size(&self, stem: &str) -> Option<(u32, u32)> {
        self.texture_sizes.get(stem).copied()
    }

    /// The terrain tile of the game's own flipbook for this texture stem.
    pub fn flipbook_tile(&self, stem: &str) -> Option<&TileRef> {
        self.flipbooks.get(stem)
    }

    /// Every terrain-atlas tile that shows this texture stem.
    pub fn terrain_tiles(&self, stem: &str) -> &[TileRef] {
        self.terrain_tiles
            .get(stem)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn flipbooks(&self) -> impl Iterator<Item = (&String, &TileRef)> {
        self.flipbooks.iter()
    }
    pub fn flipbook_count(&self) -> usize {
        self.flipbooks.len()
    }
    pub fn terrain_textures(&self) -> impl Iterator<Item = (&String, &Vec<TileRef>)> {
        self.terrain_tiles.iter()
    }
    pub fn terrain_texture_count(&self) -> usize {
        self.terrain_tiles.len()
    }
    /// Does the game's item atlas show this texture stem?
    pub fn on_item_atlas(&self, stem: &str) -> bool {
        self.item_atlas.contains(stem)
    }
    pub fn item_atlas_count(&self) -> usize {
        self.item_atlas.len()
    }

    /// The game's definition of a sound event, as JSON text.
    pub fn sound_definition(&self, event: &str) -> Option<&str> {
        self.sound_defs.get(event).map(String::as_str)
    }

    /// Every sound event the game defines, in no particular order.
    pub fn sound_events(&self) -> impl Iterator<Item = (&String, &String)> {
        self.sound_defs.iter()
    }

    pub fn has_material(&self, name: &str) -> bool {
        self.materials.contains(&name.to_ascii_lowercase())
    }
    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    pub fn has_texture(&self, path_with_ext: &str) -> bool {
        self.textures.contains(&path_with_ext.to_ascii_lowercase())
    }
    pub fn has_geometry(&self, id: &str) -> bool {
        self.geometries.contains(id)
    }
    pub fn has_animation(&self, id: &str) -> bool {
        self.animations.contains(id)
    }
    pub fn has_animation_controller(&self, id: &str) -> bool {
        self.animation_controllers.contains(id)
    }
    pub fn has_render_controller(&self, id: &str) -> bool {
        self.render_controllers.contains(id)
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }
    pub fn geometry_count(&self) -> usize {
        self.geometries.len()
    }
    pub fn animation_count(&self) -> usize {
        self.animations.len()
    }
    pub fn animation_controller_count(&self) -> usize {
        self.animation_controllers.len()
    }
    pub fn render_controller_count(&self) -> usize {
        self.render_controllers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty() && self.geometries.is_empty() && self.animations.is_empty()
    }

    // ---------------------------------------------------------------- loose files

    fn add_loose_file(&mut self, layer: &Path, p: &Path) {
        let Ok(rel) = p.strip_prefix(layer) else {
            return;
        };
        let lower = rel
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        if is_asset(&lower) {
            self.asset_stems.insert(asset_stem(&lower));
        }
        if is_texture(&lower) {
            if let Some(size) = crate::image::dimensions(p) {
                self.texture_sizes.insert(asset_stem(&lower), size);
            }
            self.textures.insert(lower);
        } else if lower.ends_with(".json") || lower.ends_with(".material") {
            if let Ok(src) = crate::filesystem::read_text(p) {
                self.harvest(&src);
            }
        }
    }

    // ---------------------------------------------------------------- bundles

    fn add_bundle(&mut self, layer: &Path, bundle: &Path) {
        let archive = match Brarchive::open(bundle) {
            Ok(a) => a,
            Err(_) => {
                self.bundles_bad += 1;
                return;
            }
        };
        self.bundles_read += 1;
        let folder = brarchive::folder_of(layer, bundle);
        // names first: textures and sounds need nothing but their path
        for name in archive.entries.keys() {
            let lower = format!("{folder}/{name}").to_ascii_lowercase();
            if is_asset(&lower) {
                self.asset_stems.insert(asset_stem(&lower));
            }
            if is_texture(&lower) {
                self.textures.insert(lower);
            }
        }
        // then only the contents worth reading, the bundle opened once
        let wanted = |n: &str| {
            let n = n.to_ascii_lowercase();
            n.ends_with(".json") || n.ends_with(".material")
        };
        for (_, bytes) in archive.read_matching(wanted) {
            self.harvest(&String::from_utf8_lossy(&bytes));
        }
        // texture sizes, from each image's header (the catalog is cached, so
        // this is paid once per game version)
        let image = |n: &str| {
            let n = n.to_ascii_lowercase();
            n.ends_with(".png") || n.ends_with(".tga")
        };
        if folder.to_ascii_lowercase().starts_with("textures") {
            for (name, bytes) in archive.read_matching(image) {
                let is_tga = name.to_ascii_lowercase().ends_with(".tga");
                if let Some(size) = crate::image::size_of_bytes(&bytes, is_tga) {
                    self.texture_sizes
                        .insert(asset_stem(&format!("{folder}/{name}")), size);
                }
            }
        }
    }

    /// Pull every identifier a vanilla JSON file defines — or USES.
    fn harvest(&mut self, src: &str) {
        let Ok(root) = json::parse(src) else { return };

        // ---- asset paths the game's own files name (UI, atlases, sound
        // definitions). A pack that ships a file at one of these paths is
        // replacing it: used, even though nothing in the pack names it.
        collect_asset_refs(&root, &mut self.asset_stems);
        self.harvest_atlas_and_sounds(&root);
        self.materials
            .extend(material_names(&root).iter().map(|m| m.to_ascii_lowercase()));

        // ---- definitions
        if let Some(arr) = root.get("minecraft:geometry").and_then(Json::as_array) {
            for g in arr {
                if let Some(id) = g
                    .get("description")
                    .and_then(|d| d.get("identifier"))
                    .and_then(Json::as_str)
                {
                    self.geometries.insert(id.to_string());
                }
            }
        }
        if let Some(obj) = root.as_object() {
            for key in obj.keys() {
                if key.starts_with("geometry.") {
                    // old format may carry its parent after a colon: `geometry.a:geometry.b`
                    self.geometries
                        .insert(key.split(':').next().unwrap_or(key).to_string());
                }
            }
        }
        insert_keys(&mut self.animations, root.get("animations"));
        insert_keys(
            &mut self.animation_controllers,
            root.get("animation_controllers"),
        );
        insert_keys(&mut self.render_controllers, root.get("render_controllers"));
        if let Some(obj) = root.get("render_controllers").and_then(Json::as_object) {
            for (id, body) in obj {
                self.render_defs
                    .entry(id.clone())
                    .or_default()
                    .push(parse_render_controller(body));
            }
        }

        // ---- uses
        //
        // Vanilla references identifiers it never defines in a file, because the
        // engine provides them: `controller.animation.player.base` is named by
        // the player entity and defined nowhere on disk. Whatever vanilla's own
        // client entities use, the game supplies. A pack that copies the player
        // entity references the same identifier and must not look broken.
        let Some(desc) = root
            .get("minecraft:client_entity")
            .and_then(|c| c.get("description"))
        else {
            return;
        };
        if let Some(obj) = desc.get("animations").and_then(Json::as_object) {
            for v in obj.values().filter_map(Json::as_str) {
                if v.starts_with("controller.") {
                    self.animation_controllers.insert(v.to_string());
                } else {
                    self.animations.insert(v.to_string());
                }
            }
        }
        if let Some(obj) = desc.get("materials").and_then(Json::as_object) {
            self.materials.extend(
                obj.values()
                    .filter_map(Json::as_str)
                    .map(|m| m.to_ascii_lowercase()),
            );
        }
        if let Some(arr) = desc.get("render_controllers").and_then(Json::as_array) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    self.render_controllers.insert(s.to_string());
                } else if let Some(obj) = item.as_object() {
                    for k in obj.keys() {
                        self.render_controllers.insert(k.clone());
                    }
                }
            }
        }
    }
}

impl VanillaCatalog {
    /// Flipbooks, terrain tiles and sound definitions, each told by its
    /// content (the file name is not passed down here).
    fn harvest_atlas_and_sounds(&mut self, root: &Json) {
        if let Some(arr) = root.as_array() {
            for e in arr {
                let (Some(tex), Some(tile)) = (
                    e.get("flipbook_texture").and_then(Json::as_str),
                    e.get("atlas_tile").and_then(Json::as_str),
                ) else {
                    continue;
                };
                let index = e.get("atlas_index").and_then(as_u32);
                self.flipbooks.insert(
                    asset_stem(tex),
                    TileRef {
                        tile: tile.to_string(),
                        index,
                    },
                );
            }
        }
        let atlas = root.get("texture_name").and_then(Json::as_str);
        let is_terrain = atlas == Some("atlas.terrain");
        if is_terrain || atlas == Some("atlas.items") {
            if let Some(data) = root.get("texture_data").and_then(Json::as_object) {
                for (tile, def) in data.iter() {
                    let mut add = |path: &str, index: Option<u32>| {
                        let stem = asset_stem(path);
                        if !is_terrain {
                            self.item_atlas.insert(stem);
                            return;
                        }
                        let t = TileRef {
                            tile: tile.clone(),
                            index,
                        };
                        let v = self.terrain_tiles.entry(stem).or_default();
                        if !v.contains(&t) {
                            v.push(t);
                        }
                    };
                    // a texture, `{"path": ...}`, or a list of either (the
                    // tile's `atlas_index` picks one); `variations` are random
                    // picks inside one entry and are not addressable
                    let path_of = |t: &Json| {
                        t.as_str()
                            .or_else(|| t.get("path").and_then(Json::as_str))
                            .map(str::to_string)
                    };
                    match def.get("textures") {
                        Some(Json::Arr(a)) => {
                            for (i, t) in a.iter().enumerate() {
                                if let Some(p) = path_of(t) {
                                    add(&p, Some(i as u32));
                                }
                            }
                        }
                        Some(t) => {
                            if let Some(p) = path_of(t) {
                                add(&p, None);
                            }
                        }
                        None => {}
                    }
                }
            }
        }
        if let Some(defs) = root.get("sound_definitions").and_then(Json::as_object) {
            for (event, def) in defs.iter() {
                self.sound_defs.insert(event.clone(), json::to_string(def));
            }
        }
    }
}

fn as_u32(v: &Json) -> Option<u32> {
    match v {
        Json::Num(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= u32::MAX as f64 => Some(*n as u32),
        _ => None,
    }
}

/// A tile of the terrain atlas, and which of its textures (`atlas_index`)
/// when it lists several.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileRef {
    pub tile: String,
    pub index: Option<u32>,
}

/// Where a partial catalog comes from.
enum Source {
    Loose { layer: PathBuf, file: PathBuf },
    Bundle { layer: PathBuf, bundle: PathBuf },
}

/// The loose files of a layer, in the order the sequential reader visited
/// them (a stack: the last folder listed is entered first). `__brarchive`
/// folders are skipped: bundles are sources of their own.
pub(crate) fn loose_files(layer: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![layer.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let is_dir = match e.file_type() {
                Ok(t) if !t.is_symlink() => t.is_dir(),
                _ => p.is_dir(),
            };
            if is_dir {
                if p.file_name().map(|n| n == "__brarchive").unwrap_or(false) {
                    continue;
                }
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

fn is_texture(lower: &str) -> bool {
    lower.starts_with("textures/") && (lower.ends_with(".png") || lower.ends_with(".tga"))
}

/// Every string in `v` that names a texture or sound path.
fn collect_asset_refs(v: &Json, out: &mut HashSet<String>) {
    match v {
        Json::Str(s) => {
            let stem = asset_stem(s);
            if stem.starts_with("textures/") || stem.starts_with("sounds/") {
                out.insert(stem);
            }
        }
        Json::Arr(a) => a.iter().for_each(|x| collect_asset_refs(x, out)),
        Json::Obj(m) => m.values().for_each(|x| collect_asset_refs(x, out)),
        _ => {}
    }
}

fn insert_keys(set: &mut HashSet<String>, v: Option<&Json>) {
    if let Some(obj) = v.and_then(Json::as_object) {
        for k in obj.keys() {
            set.insert(k.clone());
        }
    }
}
