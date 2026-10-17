pub mod atlas;
pub mod material;
pub mod model;
pub mod render_controller;
use crate::json::{self, Json};
use std::collections::{BTreeMap, BTreeSet};

/// Parsed view of `minecraft:client_entity`. Only what we need to resolve references.
/// Villager note: `animations` and `animation_controllers` are SEPARATE fields.
#[derive(Debug, Default)]
pub struct ClientEntityDecl {
    pub identifier: String,
    pub materials: BTreeMap<String, String>,
    pub textures: BTreeMap<String, String>, // shortname -> path WITHOUT extension
    pub geometry: BTreeMap<String, String>, // shortname -> geometry identifier
    pub animations: BTreeMap<String, String>,
    pub animation_controllers: Vec<(String, String)>,
    pub render_controllers: Vec<String>,
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl ClientEntityDecl {
    pub fn declared_texture_count(&self) -> usize {
        self.textures.len()
    }
    pub fn unique_texture_files(&self) -> usize {
        self.textures.values().collect::<BTreeSet<&String>>().len()
    }
}

fn str_map(v: Option<&Json>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(obj) = v.and_then(Json::as_object) {
        for (k, val) in obj {
            if let Some(s) = val.as_str() {
                out.insert(k.clone(), s.to_string());
            }
        }
    }
    out
}

/// Tolerant parsing: `format_version` is NOT reliable, so we read the actual shape of the data.
pub fn parse_client_entity(source: &str) -> Result<ClientEntityDecl, ParseError> {
    let root =
        json::parse(source).map_err(|e| ParseError(format!("{} @ byte {}", e.msg, e.pos)))?;
    let desc = root
        .get("minecraft:client_entity")
        .and_then(|c| c.get("description"))
        .ok_or_else(|| ParseError("missing minecraft:client_entity.description".into()))?;

    let mut decl = ClientEntityDecl::default();
    if let Some(id) = desc.get("identifier").and_then(Json::as_str) {
        decl.identifier = id.to_string();
    }
    decl.materials = str_map(desc.get("materials"));
    decl.textures = str_map(desc.get("textures"));
    decl.geometry = str_map(desc.get("geometry"));
    decl.animations = str_map(desc.get("animations"));

    if let Some(arr) = desc.get("render_controllers").and_then(Json::as_array) {
        for item in arr {
            if let Some(s) = item.as_str() {
                decl.render_controllers.push(s.to_string());
            } else if let Some(obj) = item.as_object() {
                for k in obj.keys() {
                    decl.render_controllers.push(k.clone());
                }
            }
        }
    }
    if let Some(arr) = desc.get("animation_controllers").and_then(Json::as_array) {
        for item in arr {
            if let Some(obj) = item.as_object() {
                for (k, val) in obj {
                    if let Some(s) = val.as_str() {
                        decl.animation_controllers.push((k.clone(), s.to_string()));
                    }
                }
            }
        }
    }
    Ok(decl)
}
