//! The inside of a render controller: which texture, geometry and material
//! slots it asks the entity for.
//!
//! ```json
//! "controller.render.cow": {
//!   "arrays":    { "textures": { "Array.skins": ["Texture.default", "Texture.warm"] } },
//!   "geometry":  "Geometry.default",
//!   "materials": [ { "*": "Material.default" } ],
//!   "textures":  [ "Array.skins[query.variant]" ]
//! }
//! ```
//!
//! Every value is a Molang expression. Three shapes occur in real packs, and
//! each gets the verdict the evidence allows:
//!
//! - `Texture.default` — a direct slot name. Provable: the entity declares it
//!   or it does not.
//! - `Array.skins[<expr>]` — one element, picked at runtime. Bounded: the
//!   candidates are known, the choice is not. Broken only if EVERY candidate is.
//! - anything else (`query.is_baby ? texture.baby : texture.default`) — which
//!   branch runs cannot be proved statically. Opaque, never a finding.
//!
//! Molang is case-insensitive, and real packs mix `Texture.x` with `texture.x`,
//! so slot and array names are lowercased here, once.

use crate::json::Json;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    Texture,
    Geometry,
    Material,
}

impl Slot {
    pub fn name(self) -> &'static str {
        match self {
            Slot::Texture => "texture",
            Slot::Geometry => "geometry",
            Slot::Material => "material",
        }
    }
}

/// One value of a render controller, classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotRef {
    /// `Texture.default` -> (Texture, "default"), name lowercased.
    Direct(Slot, String),
    /// `Array.skins[query.variant]` -> "array.skins", lowercased.
    Array(String),
    /// Any other Molang expression.
    Expression,
}

/// One value a render controller uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotUse {
    pub slot: Slot,
    /// Raw text, for reports.
    pub raw: String,
    /// A material bound to a bone pattern other than `*`. Whether it is ever
    /// looked up depends on the model having a matching bone, and on that bone
    /// being visible — `part_visibility` is Molang. Real case: AdhaCraft binds
    /// `material.blood` to `blood` bones that only the dead variant shows and
    /// only the dead variant declares. So a missing one is not provable.
    pub per_bone: bool,
}

/// A render controller, reduced to the references it makes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderControllerDef {
    /// Every distinct slot value. A value repeated (three bones sharing one
    /// material) is one reference, not three findings.
    pub uses: Vec<SlotUse>,
    /// Array name (lowercased, with the `array.` prefix) -> element values.
    pub arrays: HashMap<String, Vec<String>>,
    /// `uv_anim` rescales and scrolls the UVs at runtime (flipbook strips):
    /// the image's proportions say nothing about stretching then.
    pub uv_anim: bool,
}

pub fn parse_render_controller(v: &Json) -> RenderControllerDef {
    let mut def = RenderControllerDef {
        uv_anim: v.get("uv_anim").is_some(),
        ..Default::default()
    };

    // Arrays may live under `textures`, `geometries` or `materials`. They are
    // looked up by name alone: a name is unique within one controller.
    if let Some(sections) = v.get("arrays").and_then(Json::as_object) {
        for section in sections.values() {
            let Some(arrays) = section.as_object() else {
                continue;
            };
            for (name, elems) in arrays {
                let items = elems
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Json::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                def.arrays.insert(name.trim().to_ascii_lowercase(), items);
            }
        }
    }

    if let Some(g) = v.get("geometry").and_then(Json::as_str) {
        def.add(Slot::Geometry, g, false);
    }
    if let Some(list) = v.get("textures").and_then(Json::as_array) {
        for t in list.iter().filter_map(Json::as_str) {
            def.add(Slot::Texture, t, false);
        }
    }
    // `materials` is a list of { bone pattern: material } objects.
    if let Some(list) = v.get("materials").and_then(Json::as_array) {
        for item in list {
            if let Some(obj) = item.as_object() {
                for (bone, m) in obj {
                    if let Some(m) = m.as_str() {
                        def.add(Slot::Material, m, bone.trim() != "*");
                    }
                }
            }
        }
    }
    def
}

impl RenderControllerDef {
    /// Record a use once. If the same value is used both for every bone and
    /// for one bone, the all-bones use wins: that one is always looked up.
    fn add(&mut self, slot: Slot, raw: &str, per_bone: bool) {
        let key = raw.trim().to_ascii_lowercase();
        if let Some(u) = self
            .uses
            .iter_mut()
            .find(|u| u.slot == slot && u.raw.trim().to_ascii_lowercase() == key)
        {
            u.per_bone &= per_bone;
            return;
        }
        self.uses.push(SlotUse {
            slot,
            raw: raw.to_string(),
            per_bone,
        });
    }
}

/// Classify one Molang value. Conservative: anything not exactly one of the two
/// provable shapes is an `Expression`.
pub fn classify(value: &str) -> SlotRef {
    let lower = value.trim().to_ascii_lowercase();
    for (prefix, slot) in [
        ("texture.", Slot::Texture),
        ("geometry.", Slot::Geometry),
        ("material.", Slot::Material),
    ] {
        if let Some(name) = lower.strip_prefix(prefix) {
            if is_ident(name) {
                return SlotRef::Direct(slot, name.to_string());
            }
        }
    }
    if let Some(rest) = lower.strip_prefix("array.") {
        if let Some(open) = rest.find('[') {
            let name = &rest[..open];
            // the subscript must close exactly at the end: `array.a[0] + 1` is
            // an expression, not an array lookup
            if is_ident(name) && closes_at_end(&rest[open..]) {
                return SlotRef::Array(format!("array.{name}"));
            }
        }
    }
    SlotRef::Expression
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `s` starts with `[`; true if its matching `]` is the last character.
fn closes_at_end(s: &str) -> bool {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth = match depth.checked_sub(1) {
                    Some(d) => d,
                    None => return false,
                };
                if depth == 0 {
                    return i == s.len() - 1;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_the_three_shapes() {
        assert_eq!(
            classify("Texture.default"),
            SlotRef::Direct(Slot::Texture, "default".into())
        );
        assert_eq!(
            classify("geometry.Baby"),
            SlotRef::Direct(Slot::Geometry, "baby".into())
        );
        assert_eq!(
            classify(" Material.default "),
            SlotRef::Direct(Slot::Material, "default".into())
        );
        assert_eq!(
            classify("Array.skins[query.variant]"),
            SlotRef::Array("array.skins".into())
        );
        assert_eq!(
            classify("array.geos[ q.x[0] ]"),
            SlotRef::Array("array.geos".into())
        );
        assert_eq!(
            classify("query.is_baby ? texture.baby : texture.default"),
            SlotRef::Expression
        );
        assert_eq!(classify("array.a[0] + array.b[1]"), SlotRef::Expression);
        assert_eq!(classify("array.a[0]]"), SlotRef::Expression);
        assert_eq!(classify("texture."), SlotRef::Expression);
    }

    #[test]
    fn materials_are_deduplicated_and_marked_per_bone() {
        let rc = crate::json::parse(
            r#"{ "materials": [ {"*": "material.default"}, {"blood": "material.blood"},
                                {"blood2": "Material.blood"}, {"eye": "material.blood"} ] }"#,
        )
        .unwrap();
        let def = parse_render_controller(&rc);
        assert_eq!(def.uses.len(), 2);
        assert!(!def.uses[0].per_bone);
        assert!(def.uses[1].per_bone);
    }
}
