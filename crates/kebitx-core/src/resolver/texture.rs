use crate::graph::TextureId;
use crate::registry::texture::TextureRegistry;
use crate::vanilla::VanillaCatalog;

#[derive(Debug, PartialEq, Eq)]
pub enum TextureResolution {
    Resolved(TextureId),
    /// Exists only under a different capitalisation. Windows resolves it, Android
    /// and iOS do not. NOT "broken" in general — broken on case-sensitive
    /// platforms, which is a separate, precise claim. Carries the on-disk spelling.
    CaseMismatch(String),
    /// The GAME provides it. Not a finding: the invariant says only KnownMissing
    /// may say "broken", and a vanilla texture is simply not ours.
    External,
    KnownMissing,
}

/// The texture resolver knows nothing about variants, Molang, arrays or queries.
/// Its only job: name-WITHOUT-extension -> probe extensions -> a resolution.
///
/// Order matters:
///   1. exact match in the packs          -> Resolved
///   2. match ignoring case in the packs  -> CaseMismatch
///   3. provided by the game              -> External
///   4. otherwise                         -> KnownMissing
///
/// Step 2 goes before 3 on purpose: if the pack HAS the file under another
/// spelling, that is the author's intent, and the fix is to correct the spelling.
pub struct TextureResolver<'a> {
    registry: &'a TextureRegistry,
    catalog: &'a VanillaCatalog,
    next_id: u32,
}

impl<'a> TextureResolver<'a> {
    /// Probe order confirmed by the horse: `horse_armor_leather.tga` lives next to `.png` files.
    const EXTENSIONS: [&'static str; 2] = ["png", "tga"];

    pub fn new(registry: &'a TextureRegistry, catalog: &'a VanillaCatalog) -> Self {
        Self {
            registry,
            catalog,
            next_id: 0,
        }
    }

    pub fn resolve(&mut self, name: &str) -> TextureResolution {
        for ext in Self::EXTENSIONS {
            if self.registry.contains(&format!("{name}.{ext}")) {
                let id = TextureId(self.next_id);
                self.next_id += 1;
                return TextureResolution::Resolved(id);
            }
        }
        for ext in Self::EXTENSIONS {
            if let Some(on_disk) = self.registry.find_ignoring_case(&format!("{name}.{ext}")) {
                return TextureResolution::CaseMismatch(on_disk.to_string());
            }
        }
        // Before calling it missing, ask whether the game supplies it. Skipping
        // this step is what turned 105 references into "broken" when 2 were real.
        for ext in Self::EXTENSIONS {
            if self.catalog.has_texture(&format!("{name}.{ext}")) {
                return TextureResolution::External;
            }
        }
        TextureResolution::KnownMissing
    }
}
