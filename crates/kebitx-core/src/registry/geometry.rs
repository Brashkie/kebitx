use crate::graph::GeometryId;
use crate::json::{self, Json};
use crate::par::par_map;
use std::collections::HashMap;
use std::path::PathBuf;

/// `geometry.*` is an IDENTIFIER, not a path: all models must be indexed first (Index Pass).
#[derive(Debug, Default)]
pub struct GeometryRegistry {
    by_identifier: HashMap<String, GeometryId>,
}

impl GeometryRegistry {
    /// Scan model files and index every geometry identifier. Handles BOTH formats and does NOT
    /// trust `format_version` (villager declares 1.8.0 yet uses modern features): it detects the
    /// actual shape of the data.
    ///   new: `"minecraft:geometry": [ { "description": { "identifier": "geometry.x" } } ]`
    ///   old: top-level key `"geometry.x": { ... }`
    pub fn build(model_files: &[PathBuf]) -> Self {
        let mut by_identifier: HashMap<String, GeometryId> = HashMap::new();
        let mut next: u32 = 0;
        let mut intern = |ident: String, map: &mut HashMap<String, GeometryId>| {
            map.entry(ident).or_insert_with(|| {
                let id = GeometryId(next);
                next += 1;
                id
            });
        };

        // parse on every core; intern in file order, so ids are as before
        let per_file = par_map(model_files, |path| {
            let mut ids = Vec::new();
            let Ok(src) = crate::snapshot::read_to_string(path) else {
                return ids;
            };
            let Ok(root) = json::parse(&src) else {
                return ids;
            };
            // New format.
            if let Some(arr) = root.get("minecraft:geometry").and_then(Json::as_array) {
                for g in arr {
                    if let Some(id) = g
                        .get("description")
                        .and_then(|d| d.get("identifier"))
                        .and_then(Json::as_str)
                    {
                        ids.push(id.to_string());
                    }
                }
            }
            // Old format: top-level keys that start with "geometry.".
            if let Some(obj) = root.as_object() {
                ids.extend(obj.keys().filter(|k| k.starts_with("geometry.")).cloned());
            }
            ids
        });
        for id in per_file.into_iter().flatten() {
            intern(id, &mut by_identifier);
        }

        Self { by_identifier }
    }

    pub fn resolve(&self, identifier: &str) -> Option<GeometryId> {
        self.by_identifier.get(identifier).copied()
    }
    pub fn len(&self) -> usize {
        self.by_identifier.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_identifier.is_empty()
    }
}
