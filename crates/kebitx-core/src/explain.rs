//! What each finding means for the player, how sure kebitx is, and what
//! would fix it — one place, so every tool explains a finding the same way.
//!
//! Confidence is about the CONSEQUENCE, not the detection (detection is
//! always proved):
//! - **High**: the files alone decide what the game does.
//! - **Medium**: the defect is certain, its visible effect depends on how the
//!   world is played (pack order, which entity variant spawns, ...).
//! - **Low**: may well be intended; reported to explain a look, not to fix it.

use crate::compat::CompatIssue;
use crate::graphics::GraphicsIssue;
use crate::i18n::{tf, tr};
use crate::optimize::OptIssue;
use crate::report::Issue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub fn name(self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    pub confidence: Confidence,
    /// What the player sees, and a fix.
    pub fix: String,
}

fn ex(confidence: Confidence, fix: impl Into<String>) -> Explanation {
    Explanation {
        confidence,
        fix: fix.into(),
    }
}

pub fn issue(i: &Issue) -> Explanation {
    use Confidence::*;
    match i {
        Issue::Missing { kind, reference, .. } => match *kind {
            "texture" => ex(High, tf("shows the missing texture; add {0}.png (or .tga), or point the entity at an existing texture", &[reference])),
            "geometry" => ex(High, tf("the entity is invisible; add a model that defines {0}, or correct the identifier", &[reference])),
            "animation" | "animation_controller" => ex(
                High,
                tf("it never plays; define {0} in a RESOURCE pack's animations (a behavior-pack definition does not count), or remove it from the entity", &[reference]),
            ),
            "render" => ex(High, tf("the entity does not render; define {0} in render_controllers/, or use one of the game's", &[reference])),
            "material" => ex(High, tf("the entity may not render; use a game material (entity_alphatest, entity, ...) or define {0} in materials/", &[reference])),
            "atlas" => ex(High, tr("the block or item shows the missing texture; add the image or correct the path in the atlas")),
            "texture slot" | "geometry slot" | "material slot" => ex(
                High,
                tr("the render controller asks the entity for a name it does not declare; declare it in the entity, or change the controller"),
            ),
            "function" => ex(High, tf("the command fails, and the game may refuse the function that calls it; add {0}, or correct the name", &[reference])),
            "script file" => ex(High, tf("the pack's scripts do not load; add {0}, or correct the path", &[reference])),
            "script module" => ex(High, tf("the script does not load; add {0} to the dependencies in manifest.json", &[reference])),
            "animation name" => ex(High, tf("it never runs; declare {0} in description.animations, or remove it from scripts.animate", &[reference])),
            "behavior animation" | "behavior controller" => ex(
                High,
                tf("it never runs; define {0} in this BEHAVIOR pack (a resource-pack definition does not count here), or remove it from the entity", &[reference]),
            ),
            "recipe key" => ex(High, tf("the recipe is not loaded; add '{0}' to its key, or remove it from the pattern", &[reference])),
            _ => ex(High, tr("add what is referenced, or remove the reference")),
        },
        Issue::Dead { kind, reference, .. } => match *kind {
            "loot table" | "trade table" => ex(Medium, tf("nothing comes from it; add {0}, or correct the path, or remove it if nothing should", &[reference])),
            "component group" => ex(Medium, tf("that part of the event does nothing; define {0} in component_groups, or remove it from the event", &[reference])),
            "event" => ex(Medium, tf("nothing happens at that moment; define {0} in events, or correct the name", &[reference])),
            _ => ex(Medium, tf("the packs here define others in its namespace, but not this one: it does nothing unless a pack that is not here defines it; define {0}, or correct the name", &[reference])),
        },
        Issue::CaseMismatch { reference, on_disk, .. } => ex(
            High,
            tf("works on Windows, missing texture on Android/iOS; write the reference as the file is named ({0}), or rename the file to {1}", &[on_disk, reference]),
        ),
        Issue::EmptyTextureSet { shortname, .. } => {
            ex(High, tf("always the missing texture; give {0} a texture path, or remove the entry", &[shortname]))
        }
        Issue::Unreadable { why, .. } => {
            ex(Medium, tf("nothing inside was checked, and the game may reject it too; fix the syntax ({0})", &[why]))
        }
    }
}

pub fn compat(i: &CompatIssue) -> Explanation {
    use Confidence::*;
    match i {
        CompatIssue::DuplicateUuid { .. } => ex(High, "the game loads only one of them; give each pack and module its own new UUID"),
        CompatIssue::EngineTooNew { .. } => {
            ex(High, "the game refuses the pack; update Minecraft, or lower min_engine_version if the pack needs nothing newer")
        }
        CompatIssue::MissingDependency { .. } => ex(High, "the pack does not activate; add the pack it depends on, or remove the dependency"),
        CompatIssue::MissingListedPack { .. } => ex(High, "the world asks for a pack it does not have; add the pack, or remove it from the list"),
        CompatIssue::NotActivated { .. } => ex(Medium, "it ships with the world but does nothing; activate it, or remove it to save space"),
        CompatIssue::ConflictingFile { .. } => {
            ex(Medium, "only one of the versions is used, depending on pack order; merge them into one file in one pack")
        }
        CompatIssue::ConflictingDefinition { .. } => ex(
            Medium,
            "one definition silently replaces the other; give one a different identifier (a copy-paste usually reused it), or keep a single definition",
        ),
        CompatIssue::InvalidModuleVersion { .. } => ex(High, "the pack's scripts do not load; write the version as major.minor.patch, e.g. 1.19.0"),
        CompatIssue::ConflictingModuleVersions { .. } => ex(High, "the scripts do not load; require the module once, with one version"),
        CompatIssue::BetaApisOff { .. } => {
            ex(High, "the scripts do not run; turn on Beta APIs in the world's experiments, or use a stable module version")
        }
        CompatIssue::NeedsBetaApis { .. } => ex(Medium, "make sure the world has the Beta APIs experiment on, or the scripts will not run"),
    }
}

pub fn graphics(i: &GraphicsIssue) -> Explanation {
    use Confidence::*;
    match i {
        GraphicsIssue::UvOutside { .. } => ex(
            High,
            "those faces show transparent or wrong pixels; move their UVs inside the canvas, or enlarge texture_width/height and the image",
        ),
        GraphicsIssue::Stretched { declared, .. } => ex(
            High,
            format!("the texture is deformed; resize the image to a {}:{} proportion, or declare its real size in the model", declared[0], declared[1]),
        ),
        GraphicsIssue::AnimationBones { .. } => {
            ex(High, "that part never moves; name the bones in the animation as the model names them, or use an animation made for this model")
        }
        GraphicsIssue::BoneParentMissing { parent, .. } => {
            ex(High, format!("the bone hangs from the root and does not follow the body; add a bone named {parent}, or name an existing one as parent"))
        }
        GraphicsIssue::DuplicateBones { .. } => ex(Medium, "animations and visibility rules reach only one of them; rename the copies"),
        GraphicsIssue::InvertedCubes { .. } => ex(
            Medium,
            "those cubes render inside out: seen from outside they mostly disappear (sometimes done on purpose, e.g. a sight's reticle); if not intended, make every size positive and inflate no smaller than -size/2",
        ),
        GraphicsIssue::EmptyCubes { .. } => ex(Low, "they never render; harmless, but they can be removed"),
        GraphicsIssue::AnimationScale { value, .. } => {
            if *value == [0.0; 3] {
                ex(Low, "the bone vanishes while this animation plays; if that part should stay visible, this is why it does not")
            } else {
                ex(Low, "the bone becomes huge while this animation plays; if it looks wrong, this scale is the cause")
            }
        }
        GraphicsIssue::ShaderNeedsModifiedClient { .. } => ex(
            High,
            "the normal game ignores compiled shaders: the pack changes nothing; use a Vibrant Visuals pack (atmospherics, lighting, ...) instead",
        ),
        GraphicsIssue::VibrantVisuals { .. } => ex(
            Medium,
            "shows only on a device that supports Vibrant Visuals, with it chosen in the video settings; elsewhere the world looks as without it",
        ),
    }
}

pub fn optimize(i: &OptIssue) -> Explanation {
    use Confidence::*;
    match i {
        OptIssue::BigIcon { .. } => ex(High, "the game shows it small; a 256×256 PNG looks the same and weighs a fraction"),
        OptIssue::BigTexture { .. } => ex(
            Low,
            "fine on a PC, a large share of a phone's video memory; if the detail is not needed, halve it (keeping the proportion)",
        ),
        OptIssue::MislabelledImage { .. } => {
            ex(Low, "the content is not what the name says: it has no transparency, and tools that trust the name misread it; convert it to a real PNG")
        }
        OptIssue::HeavyModel { .. } => {
            ex(Low, "every cube is drawn every frame; on a weak phone many of these at once cost frame rate — merge or remove hidden cubes")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_explanation_has_a_fix() {
        let issues = [
            Issue::Missing {
                kind: "texture",
                owner: "o".into(),
                reference: "r".into(),
            },
            Issue::Missing {
                kind: "something new",
                owner: "o".into(),
                reference: "r".into(),
            },
            Issue::CaseMismatch {
                owner: "o".into(),
                reference: "a".into(),
                on_disk: "A".into(),
            },
            Issue::EmptyTextureSet {
                file: "f".into(),
                shortname: "s".into(),
            },
            Issue::Unreadable {
                file: "f".into(),
                why: "w".into(),
            },
        ];
        for i in &issues {
            assert!(!issue(i).fix.is_empty());
        }
        let g = GraphicsIssue::AnimationScale {
            animation: "a".into(),
            file: "f".into(),
            bone: "b".into(),
            value: [0.0; 3],
        };
        assert_eq!(graphics(&g).confidence, Confidence::Low);
        assert!(graphics(&g).fix.contains("vanishes"));
    }
}
