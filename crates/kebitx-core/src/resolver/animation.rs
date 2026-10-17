//! Resolution in IDENTIFIER space for animations, animation controllers and
//! render controllers.

use crate::registry::animation::AnimationRegistry;
use crate::registry::controller::ControllerRegistry;
use crate::vanilla::VanillaCatalog;

#[derive(Debug, PartialEq, Eq)]
pub enum IdResolution {
    Resolved,
    /// Provided by the game. Never a finding.
    External,
    KnownMissing,
}

/// A value from a client entity's `animations` map. It may name an animation OR
/// an animation controller: the entity file does not say which, and the
/// `controller.` prefix is a convention, not a rule. Precision first: it is
/// resolved if EITHER index knows it, so a naming quirk can never become a
/// false "broken".
pub fn resolve_animation_ref(
    reg: &AnimationRegistry,
    catalog: &VanillaCatalog,
    id: &str,
) -> IdResolution {
    if reg.resolve_animation(id).is_some() || reg.resolve_controller(id).is_some() {
        IdResolution::Resolved
    } else if catalog.has_animation(id) || catalog.has_animation_controller(id) {
        IdResolution::External
    } else {
        IdResolution::KnownMissing
    }
}

/// An entry of the legacy `animation_controllers` array of a client entity.
pub fn resolve_animation_controller(
    reg: &AnimationRegistry,
    catalog: &VanillaCatalog,
    id: &str,
) -> IdResolution {
    if reg.resolve_controller(id).is_some() {
        IdResolution::Resolved
    } else if catalog.has_animation_controller(id) {
        IdResolution::External
    } else {
        IdResolution::KnownMissing
    }
}

/// A render controller named by a client entity.
pub fn resolve_render_controller(
    reg: &ControllerRegistry,
    catalog: &VanillaCatalog,
    id: &str,
) -> IdResolution {
    if reg.resolve(id).is_some() {
        IdResolution::Resolved
    } else if catalog.has_render_controller(id) {
        IdResolution::External
    } else {
        IdResolution::KnownMissing
    }
}
