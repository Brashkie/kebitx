use crate::graph::ControllerId;
use crate::json::{self, Json};
use crate::par::par_map;
use crate::parser::render_controller::{parse_render_controller, RenderControllerDef};
use std::collections::HashMap;
use std::path::PathBuf;

/// Index of render controllers (`controller.render.*`), by identifier, with
/// what each one asks of the entities that use it.
///
/// One identifier may be defined by more than one pack. Which definition the
/// game keeps depends on pack order, so every definition is kept, and a slot
/// counts as missing only if it is missing under ALL of them.
#[derive(Debug, Default)]
pub struct ControllerRegistry {
    by_identifier: HashMap<String, ControllerId>,
    defs: Vec<Vec<RenderControllerDef>>,
}

impl ControllerRegistry {
    pub fn build(files: &[PathBuf]) -> Self {
        // parse on every core; register in file order, so ids are as before
        let per_file: Vec<Vec<(String, RenderControllerDef)>> = par_map(files, |path| {
            let Ok(src) = crate::snapshot::read_to_string(path) else {
                return Vec::new();
            };
            let Ok(root) = json::parse(&src) else {
                return Vec::new();
            };
            root.get("render_controllers")
                .and_then(Json::as_object)
                .map(|obj| {
                    obj.iter()
                        .map(|(k, body)| (k.clone(), parse_render_controller(body)))
                        .collect()
                })
                .unwrap_or_default()
        });
        let mut reg = Self::default();
        for (key, def) in per_file.into_iter().flatten() {
            let id = match reg.by_identifier.get(&key) {
                Some(&id) => id,
                None => {
                    let id = ControllerId(reg.defs.len() as u32);
                    reg.by_identifier.insert(key, id);
                    reg.defs.push(Vec::new());
                    id
                }
            };
            reg.defs[id.0 as usize].push(def);
        }
        reg
    }
    pub fn resolve(&self, id: &str) -> Option<ControllerId> {
        self.by_identifier.get(id).copied()
    }
    /// Every definition of a render controller (one per pack that defines it).
    pub fn definitions(&self, id: ControllerId) -> &[RenderControllerDef] {
        self.defs
            .get(id.0 as usize)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    pub fn len(&self) -> usize {
        self.by_identifier.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_identifier.is_empty()
    }
}
