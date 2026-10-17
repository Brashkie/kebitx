use crate::graph::{AnimationId, ControllerId};
use crate::json::{self, Json};
use crate::par::par_map;
use std::collections::HashMap;
use std::path::PathBuf;

/// Index of animations (`animation.*`) and, separately, animation controllers
/// (`controller.animation.*`). IDENTIFIER space, like geometry: an entity names
/// them, and the definition can live in any file of any pack.
///
/// The two are kept apart because the files are: animation files declare a
/// top-level `"animations"` object, controller files declare
/// `"animation_controllers"`. A file is read for whichever key it actually has —
/// the folder is a convention, not a guarantee.
#[derive(Debug, Default)]
pub struct AnimationRegistry {
    by_identifier: HashMap<String, AnimationId>,
    controllers: HashMap<String, ControllerId>,
}

impl AnimationRegistry {
    pub fn build(files: &[PathBuf]) -> Self {
        // parse on every core; index in file order, so ids are as before
        let per_file = par_map(files, |path| {
            let keys = |root: &Json, section: &str| -> Vec<String> {
                root.get(section)
                    .and_then(Json::as_object)
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default()
            };
            let Ok(src) = crate::snapshot::read_to_string(path) else {
                return (Vec::new(), Vec::new());
            };
            let Ok(root) = json::parse(&src) else {
                return (Vec::new(), Vec::new());
            };
            (
                keys(&root, "animations"),
                keys(&root, "animation_controllers"),
            )
        });
        let mut reg = Self::default();
        for (animations, controllers) in per_file {
            for key in animations {
                let next = AnimationId(reg.by_identifier.len() as u32);
                reg.by_identifier.entry(key).or_insert(next);
            }
            for key in controllers {
                let next = ControllerId(reg.controllers.len() as u32);
                reg.controllers.entry(key).or_insert(next);
            }
        }
        reg
    }

    pub fn resolve_animation(&self, id: &str) -> Option<AnimationId> {
        self.by_identifier.get(id).copied()
    }
    pub fn resolve_controller(&self, id: &str) -> Option<ControllerId> {
        self.controllers.get(id).copied()
    }
    pub fn animation_count(&self) -> usize {
        self.by_identifier.len()
    }
    pub fn controller_count(&self) -> usize {
        self.controllers.len()
    }
}
