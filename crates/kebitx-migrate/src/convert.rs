//! Writing the Bedrock resource pack: only what the report says CONVERTS.

use crate::report::{MigrationReport, Verdict, FLIPBOOKS, LANGUAGES, MANIFEST, SOUND_DEFINITIONS};
use kebitx_core::codec::png;
use kebitx_core::json::Json;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Converted {
    pub out: PathBuf,
    /// Files written, the manifest included.
    pub files: usize,
    /// Java files whose Bedrock path another file had already taken.
    pub skipped_duplicates: usize,
    /// The behavior pack written next to `out` (`<out>_behavior_pack`), if
    /// the Java pack had data that converts.
    pub behavior_out: Option<PathBuf>,
}

/// Write the Bedrock pack into `out`, which must not exist or be empty: a
/// conversion never deletes or overwrites anything that was there.
pub fn convert(
    java_root: &Path,
    report: &MigrationReport,
    out: &Path,
) -> Result<Converted, String> {
    if out.exists()
        && fs::read_dir(out)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
    {
        return Err(format!(
            "{} exists and is not empty; choose a new folder",
            out.display()
        ));
    }
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;

    // exact matches first, so a file matched only by name never takes a path
    // an exact match needs
    let mut entries: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.verdict == Verdict::Converts)
        .collect();
    entries.sort_by_key(|e| !e.exact);

    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut converted = Converted {
        out: out.to_path_buf(),
        files: 0,
        skipped_duplicates: 0,
        behavior_out: None,
    };
    let mkdir = |dst: &Path| match dst.parent() {
        Some(dir) => fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display())),
        None => Ok(()),
    };
    for e in entries {
        let Some(target) = &e.bedrock else { continue };
        if target.starts_with("behavior_pack/") {
            continue; // written with the behavior pack, below
        }
        let generated = report.is_generated(target);
        if !taken.insert(target.to_ascii_lowercase()) {
            // several animations feed one flipbook file, several sprites one
            // sheet: not duplicates
            if !generated {
                converted.skipped_duplicates += 1;
            }
            continue;
        }
        let dst = out.join(target);
        mkdir(&dst)?;
        let write = |bytes: Vec<u8>| {
            fs::write(&dst, bytes).map_err(|err| format!("{}: {err}", dst.display()))
        };
        match target.as_str() {
            MANIFEST => write(manifest(report).into_bytes())?,
            FLIPBOOKS => write(crate::flipbook::flipbooks_file(&report.flipbooks).into_bytes())?,
            SOUND_DEFINITIONS => {
                write(crate::sounds::definitions_file(&report.sound_definitions).into_bytes())?
            }
            t if report.images.contains_key(t) => write(png::encode(&report.images[t]))?,
            t if t.starts_with("texts/") && t.ends_with(".lang") => {
                let code = t.trim_start_matches("texts/").trim_end_matches(".lang");
                write(crate::lang::lang_file(&report.texts[code]).into_bytes())?
            }
            _ => {
                fs::copy(java_root.join(&e.java), &dst)
                    .map_err(|err| format!("{}: {err}", e.java))?;
            }
        }
        converted.files += 1;
    }
    // the languages a pack provides are listed, as the game expects
    if !report.texts.is_empty() {
        let dst = out.join(LANGUAGES);
        mkdir(&dst)?;
        let list = Json::Arr(report.texts.keys().map(|c| Json::Str(c.clone())).collect());
        fs::write(&dst, kebitx_core::json::to_string(&list))
            .map_err(|err| format!("{}: {err}", dst.display()))?;
        converted.files += 1;
    }
    converted.files += write_models(report, out)?;
    converted.files += write_cem(report, out)?;
    // the mods' items and blocks: their resource files, item atlas, names
    let put = |rel: &str, text: String| -> Result<(), String> {
        let dst = out.join(rel);
        mkdir(&dst)?;
        fs::write(&dst, text).map_err(|e| format!("{}: {e}", dst.display()))
    };
    for (path, json) in &report.resource {
        put(path, kebitx_core::json::to_string(json))?;
        converted.files += 1;
    }
    if !report.item_atlas.is_empty() {
        put(
            "textures/item_texture.json",
            kebitx_core::json::to_string(&crate::adapters::atlas_file(
                "atlas.items",
                &report.item_atlas,
            )),
        )?;
        converted.files += 1;
    }
    for (code, lines) in &report.texts {
        let path = format!("texts/{code}.lang");
        if !out.join(&path).exists() {
            put(&path, crate::lang::lang_file(lines))?;
            converted.files += 1;
        }
    }
    for (path, json) in &report.draft_files {
        put(
            &format!("kebitx_drafts/{path}"),
            kebitx_core::json::to_string(json),
        )?;
        converted.files += 1;
    }
    for (path, text) in &report.task_files {
        put(&format!("kebitx_drafts/{path}"), text.clone())?;
        converted.files += 1;
    }
    if !report.tasks.is_empty() || report.mod_info.is_some() {
        let mut text = match &report.mod_info {
            Some((loader, name)) => {
                format!("{name} ({loader} mod): what a person has to do in Bedrock\n\n")
            }
            None => "What a person has to do in Bedrock\n\n".to_string(),
        };
        let classes = report
            .entries
            .iter()
            .filter(|e| e.java.to_ascii_lowercase().ends_with(".class"))
            .count();
        if classes > 0 {
            text.push_str(&format!(
                "- {classes} Java class file(s): the mod's behaviour is code; reimplement it with Bedrock scripts and components (never automatic)\n"
            ));
        }
        for t in &report.tasks {
            text.push_str(&format!("- {t}\n"));
        }
        put("kebitx_drafts/mod_tasks.txt", text)?;
        converted.files += 1;
    }
    if !report.behavior.is_empty() {
        let name = out
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "converted".into());
        let bp = out.with_file_name(format!("{name}_behavior_pack"));
        if bp.exists()
            && fs::read_dir(&bp)
                .map(|mut d| d.next().is_some())
                .unwrap_or(true)
        {
            return Err(format!(
                "{} exists and is not empty; choose a new folder",
                bp.display()
            ));
        }
        for (path, json) in &report.behavior {
            let dst = bp.join(path);
            mkdir(&dst)?;
            fs::write(&dst, kebitx_core::json::to_string(json))
                .map_err(|e| format!("{}: {e}", dst.display()))?;
            converted.files += 1;
        }
        fs::write(bp.join(MANIFEST), behavior_manifest(report)).map_err(|e| e.to_string())?;
        converted.files += 1;
        converted.behavior_out = Some(bp);
    }
    Ok(converted)
}

/// Converted models: block geometry and its terrain tiles go into the pack
/// (inert until a block uses them); what uses them — a custom block for the
/// behavior pack, an attachable — goes to `kebitx_drafts/`, outside what
/// the game loads, for a person to finish and move.
fn write_models(report: &MigrationReport, out: &Path) -> Result<usize, String> {
    use crate::report::ModelKind;
    if report.models.is_empty() && report.terrain.is_empty() {
        return Ok(0);
    }

    let put = |rel: &str, text: String| -> Result<(), String> {
        let dst = out.join(rel);
        if let Some(dir) = dst.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&dst, text).map_err(|e| format!("{}: {e}", dst.display()))
    };
    let s = |v: &str| Json::Str(v.to_string());
    let obj = |entries: Vec<(&str, Json)>| {
        Json::Obj(kebitx_core::json::JsonMap::from_entries(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        ))
    };
    let mut files = 0;
    // the mods' blocks share the terrain atlas file with converted models
    let mut tiles: Vec<(String, Json)> = report
        .terrain
        .iter()
        .map(|(k, v)| (k.clone(), obj(vec![("textures", s(v))])))
        .collect();
    let mut notes = String::from(
        "kebitx drafts: what converted Java models need, for a person to finish.\n\
         The geometry follows the rules of Mojang's own conversion (checked against the game);\n\
         what uses it is a draft. Nothing here is loaded by the game until it is moved.\n\n",
    );
    for m in &report.models {
        let geo_text = kebitx_core::json::to_string(&m.geometry);
        let missing: Vec<&String> = m
            .textures
            .iter()
            .filter(|(_, t)| t.is_none())
            .map(|(i, _)| i)
            .collect();
        let mut line = format!("{}  ->  {}", m.java, m.geometry_id());
        if m.unverified > 0 {
            line.push_str(&format!(
                "; {} rotated/rescaled element(s): check their angle in game",
                m.unverified
            ));
        }
        if !missing.is_empty() {
            line.push_str(&format!(
                "; textures with no proven Bedrock file: {missing:?}"
            ));
        }
        notes.push_str(&line);
        notes.push('\n');
        match m.kind {
            ModelKind::Block => {
                put(
                    &format!("models/blocks/kebitx_{}.geo.json", m.name),
                    geo_text,
                )?;
                files += 1;
                // Java's cullface as Bedrock's culling rules (as Mojang's own)
                let culling_id = format!("kebitx:culling.{}", m.name);
                if !m.culling.is_empty() {
                    put(
                        &format!("block_culling/kebitx_{}.culling.json", m.name),
                        kebitx_core::json::to_string(&crate::models::culling_file(
                            &culling_id,
                            &m.culling,
                        )),
                    )?;
                    files += 1;
                }
                let mut geometry = vec![("identifier", s(&m.geometry_id()))];
                if !m.culling.is_empty() {
                    geometry.push(("culling", s(&culling_id)));
                }
                let mut instances = Vec::new();
                for (instance, tex) in &m.textures {
                    let tile = format!("kebitx_{}_{instance}", m.name);
                    if let Some(t) = tex {
                        tiles.push((tile.clone(), obj(vec![("textures", s(t))])));
                    }
                    instances.push((
                        instance.clone(),
                        obj(vec![
                            ("texture", s(&tile)),
                            ("render_method", s("alpha_test")),
                        ]),
                    ));
                }
                if let Some((_, first)) = instances.first().cloned() {
                    instances.push(("*".into(), first));
                }
                let block = obj(vec![
                    ("format_version", s("1.21.0")),
                    (
                        "minecraft:block",
                        obj(vec![
                            (
                                "description",
                                obj(vec![
                                    ("identifier", s(&format!("kebitx:{}", m.name))),
                                    ("menu_category", obj(vec![("category", s("construction"))])),
                                ]),
                            ),
                            (
                                "components",
                                obj(vec![
                                    ("minecraft:geometry", obj(geometry)),
                                    (
                                        "minecraft:material_instances",
                                        Json::Obj(kebitx_core::json::JsonMap::from_entries(
                                            instances,
                                        )),
                                    ),
                                ]),
                            ),
                        ]),
                    ),
                ]);
                put(
                    &format!("kebitx_drafts/behavior_pack/blocks/{}.json", m.name),
                    kebitx_core::json::to_string(&block),
                )?;
                files += 1;
            }
            ModelKind::Item => {
                put(
                    &format!(
                        "kebitx_drafts/resource_pack/models/entity/kebitx_{}.geo.json",
                        m.name
                    ),
                    geo_text,
                )?;
                let texture = m
                    .textures
                    .values()
                    .flatten()
                    .next()
                    .cloned()
                    .unwrap_or_default();
                let attachable = obj(vec![
                    ("format_version", s("1.10.0")),
                    (
                        "minecraft:attachable",
                        obj(vec![(
                            "description",
                            obj(vec![
                                ("identifier", s(&item_identifier(m))),
                                ("materials", obj(vec![("default", s("entity_alphatest"))])),
                                ("textures", obj(vec![("default", s(&texture))])),
                                ("geometry", obj(vec![("default", s(&m.geometry_id()))])),
                                (
                                    "render_controllers",
                                    Json::Arr(vec![s("controller.render.item_default")]),
                                ),
                            ]),
                        )]),
                    ),
                ]);
                put(
                    &format!("kebitx_drafts/resource_pack/attachables/{}.json", m.name),
                    kebitx_core::json::to_string(&attachable),
                )?;
                files += 2;
                if m.textures.len() > 1 {
                    notes.push_str(
                        "    (an attachable shows ONE texture; this model uses several)\n",
                    );
                }
            }
        }
    }
    if !tiles.is_empty() {
        let terrain = obj(vec![
            ("resource_pack_name", s("kebitx")),
            ("texture_name", s("atlas.terrain")),
            ("padding", Json::Num(8.0)),
            ("num_mip_levels", Json::Num(4.0)),
            (
                "texture_data",
                Json::Obj(kebitx_core::json::JsonMap::from_entries(tiles)),
            ),
        ]);
        put(
            "textures/terrain_texture.json",
            kebitx_core::json::to_string(&terrain),
        )?;
        files += 1;
    }
    if report.models.is_empty() {
        return Ok(files);
    }
    put("kebitx_drafts/README.txt", notes)?;
    Ok(files + 1)
}

/// A Bedrock manifest for the converted pack. The UUIDs are derived from the
/// Java pack's name and description, so converting the same pack twice gives
/// the same UUIDs — the game sees an update, not a second pack.
fn manifest(report: &MigrationReport) -> String {
    pack_manifest(report, "resources", "")
}

/// The behavior pack's: its own UUIDs (another seed), module type `data`.
fn behavior_manifest(report: &MigrationReport) -> String {
    pack_manifest(report, "data", "behavior\u{0}")
}

fn pack_manifest(report: &MigrationReport, module: &str, salt: &str) -> String {
    let name = report.name.clone();
    let seed = format!("{salt}{name}\u{0}{}", report.description);
    let esc = |s: &str| kebitx_core::json::to_string(&kebitx_core::json::Json::Str(s.to_string()));
    format!(
        "{{\n  \"format_version\": 2,\n  \"header\": {{\n    \"name\": {},\n    \"description\": {},\n    \"uuid\": \"{}\",\n    \"version\": [1, 0, 0],\n    \"min_engine_version\": [1, 20, 0]\n  }},\n  \"modules\": [\n    {{ \"type\": \"{module}\", \"uuid\": \"{}\", \"version\": [1, 0, 0] }}\n  ]\n}}\n",
        esc(&name),
        esc(&format!("{} (converted from Java by kebitx)", report.description)),
        uuid(&format!("header\u{0}{seed}")),
        uuid(&format!("module\u{0}{seed}")),
    )
}

/// A version-4-shaped UUID from a 128-bit FNV-1a hash of `s`: stable for the
/// same input, different for different inputs.
fn uuid(s: &str) -> String {
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    for b in s.bytes() {
        h ^= b as u128;
        h = h.wrapping_mul(0x0000000001000000000000000000013B);
    }
    let mut b = h.to_be_bytes();
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// What each OptiFine entity model proves, for a person rebuilding it in
/// Bedrock: which part is which bone.
fn write_cem(report: &MigrationReport, out: &Path) -> Result<usize, String> {
    let mut n = 0;
    for c in &report.cem {
        let mut text = format!(
            "OptiFine model of {} (read by kebitx)\n\nBedrock geometry matched: {}\n\n",
            c.entity,
            c.geometry
                .as_deref()
                .unwrap_or("none (no geometry of this texture size in which a part is proven)")
        );
        text.push_str("part -> Bedrock bone (proven: its boxes paint exactly the pixels the bone's cubes paint)\n");
        for (part, bone) in &c.parts {
            text.push_str(&format!(
                "  {part} -> {}\n",
                bone.as_deref().unwrap_or("not proven")
            ));
        }
        text.push_str(&format!(
            "\nNot converted: box positions (OptiFine's axes and translate have no pair in the games to check)\n\
             and {} animation expression(s) (OptiFine expressions, not Molang).\n",
            c.animations
        ));
        let dst = out
            .join("kebitx_drafts/cem")
            .join(format!("{}.txt", c.entity));
        if let Some(dir) = dst.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(&dst, text).map_err(|e| format!("{}: {e}", dst.display()))?;
        n += 1;
    }
    Ok(n)
}

/// The item an attachable dresses: `minecraft:diamond_sword`, `mymod:gear`.
fn item_identifier(m: &crate::report::ModelDraft) -> String {
    let ns = m
        .java
        .split('/')
        .nth(1)
        .unwrap_or("minecraft")
        .to_ascii_lowercase();
    let name = m
        .name
        .strip_prefix(&format!("{ns}_"))
        .filter(|_| ns != "minecraft")
        .unwrap_or(&m.name);
    format!("{ns}:{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_are_stable_well_formed_and_distinct() {
        let a = uuid("pack");
        assert_eq!(a, uuid("pack"));
        assert_ne!(a, uuid("pack2"));
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4", "version 4: {a}");
        assert!("89ab".contains(&a[19..20]), "RFC 4122 variant: {a}");
    }
}
