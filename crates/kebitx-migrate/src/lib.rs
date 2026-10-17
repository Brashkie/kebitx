//! Java Edition resource packs to Bedrock: a compatibility report first
//! (`report`), then the conversion of what provably converts (`convert`).
//!
//! Every conversion rests on evidence from the installed games: textures by
//! path, by a confirmed rename (`names`) or by identical pixels (`content`);
//! animations as flipbooks (`flipbook`); sound events and language strings by
//! identical files and identical English (`sounds`, `lang`), checked against
//! the installed Java game (`java`).

pub mod adapters;
pub mod cem;
pub mod content;
pub mod convert;
pub mod flipbook;
pub mod functions;
pub mod input;
pub mod java;
pub mod lang;
pub mod loot;
pub mod models;
pub mod names;
pub mod recipes;
pub mod report;
pub mod sounds;

pub use convert::{convert, Converted};
pub use java::{JavaAssets, JavaJar};
pub use report::{inspect, pack_meta, Entry, MigrationReport, References, Verdict};
pub use sounds::SoundEvent;

use content::{BedrockPixels, JavaPixels};
use kebitx_core::json::Json;
use kebitx_core::vanilla::VanillaCatalog;
use lang::BedrockLang;

/// The references found on this machine, owned; `refs` lends them out.
#[derive(Default)]
pub struct Sources {
    pub java: Option<JavaAssets>,
    pub java_sounds: Option<Json>,
    pub java_jar: Option<JavaJar>,
    pub java_pixels: Option<JavaPixels>,
    pub bedrock_pixels: Option<BedrockPixels>,
    pub bedrock_lang: Option<BedrockLang>,
    pub recipes: Option<recipes::RecipeRefs>,
    pub loot: Option<loot::LootRefs>,
    pub bedrock_geometry: Option<cem::BedrockGeometry>,
    pub bedrock_defs: Option<adapters::BedrockDefs>,
}

impl Sources {
    /// Everything there is: the installed Java game (its `sounds.json`, the
    /// `.jar` of the version of `pack_format`) and the Bedrock game's pixels
    /// and strings.
    pub fn discover(pack_format: Option<u32>) -> Self {
        Self::discover_for(None, pack_format)
    }

    /// Only what the pack at `pack` can use: recipes and loot tables when it
    /// has `data/`, entity geometry when it has OptiFine models (each takes
    /// seconds to read from the games). `None` reads everything.
    pub fn discover_for(pack: Option<&std::path::Path>, pack_format: Option<u32>) -> Self {
        let needs = |rel: &str| pack.is_none_or(|p| p.join(rel).is_dir());
        let java = JavaAssets::discover();
        let java_sounds = java.as_ref().and_then(|j| j.sounds_json());
        let java_jar = java.as_ref().and_then(|j| j.jar_for(pack_format));
        let java_pixels = java_jar.as_ref().map(JavaPixels::from_jar);
        let install = kebitx_core::vanilla::install_dir();
        let bedrock_pixels = install.as_deref().map(BedrockPixels::from_install);
        let bedrock_lang = install.as_deref().map(BedrockLang::from_install);
        let (recipes, loot) = match (&install, &java_jar) {
            (Some(i), Some(j)) if needs("data") => (
                Some(recipes::RecipeRefs::build(i, j)),
                Some(loot::LootRefs::build(i, j)),
            ),
            _ => (None, None),
        };
        let bedrock_geometry = install
            .as_deref()
            .filter(|_| needs("assets/minecraft/optifine/cem"))
            .map(cem::BedrockGeometry::from_install);
        // a namespace of its own under assets/: a mod, or a pack with mod content
        let has_mod = pack.is_none_or(|p| {
            std::fs::read_dir(p.join("assets"))
                .map(|d| {
                    d.flatten()
                        .any(|e| e.path().is_dir() && e.file_name() != "minecraft")
                })
                .unwrap_or(false)
        });
        let bedrock_defs = install
            .as_deref()
            .filter(|_| has_mod)
            .map(adapters::BedrockDefs::from_install);
        Sources {
            bedrock_defs,
            java,
            java_sounds,
            java_jar,
            java_pixels,
            bedrock_pixels,
            bedrock_lang,
            recipes,
            loot,
            bedrock_geometry,
        }
    }

    pub fn refs<'a>(&'a self, catalog: &'a VanillaCatalog) -> References<'a> {
        References {
            catalog,
            java_sounds: self.java_sounds.as_ref(),
            java_jar: self.java_jar.as_ref(),
            java_pixels: self.java_pixels.as_ref(),
            bedrock_pixels: self.bedrock_pixels.as_ref(),
            bedrock_lang: self.bedrock_lang.as_ref(),
            recipes: self.recipes.as_ref(),
            loot: self.loot.as_ref(),
            bedrock_geometry: self.bedrock_geometry.as_ref(),
            bedrock_defs: self.bedrock_defs.as_ref(),
        }
    }
}
