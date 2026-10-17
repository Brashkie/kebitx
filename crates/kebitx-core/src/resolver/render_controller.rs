//! Checks a render controller against ONE entity that uses it.
//!
//! A render controller does not stand alone: `Texture.default` means "the
//! texture this entity calls `default`". The same controller can be fine for
//! one entity and broken for the next, so the check is per (entity, controller).

use crate::parser::render_controller::{classify, RenderControllerDef, Slot, SlotRef};
use crate::parser::ClientEntityDecl;
use std::collections::HashSet;

/// Verdict for one slot value. Maps onto `EdgeState`: `Bounded` is
/// `BoundedOpaque`, `Opaque` is `UnboundedOpaque`. Only `Missing` is broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotCheck {
    Resolved,
    /// Provably absent: a direct name the entity does not declare, an array
    /// none of whose candidates it declares, or an array that does not exist.
    Missing,
    /// An array where some candidates exist and some do not: which one the
    /// runtime picks is unknown.
    Bounded,
    /// A Molang expression: not evaluated, never a finding.
    Opaque,
}

/// The shortnames an entity declares, lowercased (Molang ignores case).
#[derive(Debug, Default)]
pub struct EntitySlots {
    textures: HashSet<String>,
    geometry: HashSet<String>,
    materials: HashSet<String>,
}

impl EntitySlots {
    pub fn of(decl: &ClientEntityDecl) -> Self {
        let lower = |m: &std::collections::BTreeMap<String, String>| {
            m.keys().map(|k| k.to_ascii_lowercase()).collect()
        };
        Self {
            textures: lower(&decl.textures),
            geometry: lower(&decl.geometry),
            materials: lower(&decl.materials),
        }
    }
    fn has(&self, slot: Slot, name: &str) -> bool {
        match slot {
            Slot::Texture => self.textures.contains(name),
            Slot::Geometry => self.geometry.contains(name),
            Slot::Material => self.materials.contains(name),
        }
    }
}

/// Check every slot value of one definition.
pub fn check(def: &RenderControllerDef, slots: &EntitySlots) -> Vec<(Slot, String, SlotCheck)> {
    def.uses
        .iter()
        .map(|u| {
            let mut c = check_value(def, slots, &u.raw);
            // a per-bone material is looked up only if the bone exists and is
            // shown: absence is not provable (see `SlotUse::per_bone`)
            if u.per_bone && c == SlotCheck::Missing {
                c = SlotCheck::Opaque;
            }
            (u.slot, u.raw.clone(), c)
        })
        .collect()
}

/// Check against every definition of the controller. A value stays `Missing`
/// only if it is missing under all of them; if the definitions disagree, which
/// one the game uses depends on pack order, so the verdict becomes `Opaque`.
pub fn check_all(
    defs: &[RenderControllerDef],
    slots: &EntitySlots,
) -> Vec<(Slot, String, SlotCheck)> {
    let Some((first, rest)) = defs.split_first() else {
        return Vec::new();
    };
    let mut out = check(first, slots);
    if rest.is_empty() {
        return out;
    }
    let others: Vec<HashSet<String>> = rest
        .iter()
        .map(|d| {
            check(d, slots)
                .into_iter()
                .filter(|(_, _, c)| *c == SlotCheck::Missing)
                .map(|(_, raw, _)| raw.trim().to_ascii_lowercase())
                .collect()
        })
        .collect();
    for (_, raw, c) in &mut out {
        let key = raw.trim().to_ascii_lowercase();
        if *c == SlotCheck::Missing && !others.iter().all(|m| m.contains(&key)) {
            *c = SlotCheck::Opaque;
        }
    }
    out
}

fn check_value(def: &RenderControllerDef, slots: &EntitySlots, raw: &str) -> SlotCheck {
    match classify(raw) {
        SlotRef::Direct(slot, name) => {
            if slots.has(slot, &name) {
                SlotCheck::Resolved
            } else {
                SlotCheck::Missing
            }
        }
        SlotRef::Array(name) => {
            let Some(elems) = def.arrays.get(&name) else {
                return SlotCheck::Missing;
            };
            let mut present = 0;
            let mut absent = 0;
            for e in elems {
                match classify(e) {
                    SlotRef::Direct(slot, n) if slots.has(slot, &n) => present += 1,
                    SlotRef::Direct(..) => absent += 1,
                    // an element that is itself an expression: nothing provable
                    _ => return SlotCheck::Bounded,
                }
            }
            match (present, absent) {
                (_, 0) if present > 0 => SlotCheck::Resolved,
                (0, a) if a > 0 => SlotCheck::Missing,
                // empty array, or a mix
                _ => SlotCheck::Bounded,
            }
        }
        SlotRef::Expression => SlotCheck::Opaque,
    }
}
