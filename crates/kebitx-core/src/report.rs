use crate::findings::Finding;

/// One concrete, provable problem, with enough context to go and fix it.
///
/// Every variant is born from a resolution the engine could PROVE: a
/// `KnownMissing`, a match that only exists ignoring case, or an atlas entry that
/// yields zero paths. Nothing here comes from a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    /// Referenced, and neither the packs nor the game have it.
    Missing {
        kind: &'static str,
        owner: String,
        reference: String,
    },
    /// Resolves only ignoring case: fine on Windows, pink on Android/iOS.
    CaseMismatch {
        owner: String,
        reference: String,
        on_disk: String,
    },
    /// An atlas entry that declares a shortname and yields no texture at all.
    EmptyTextureSet { file: String, shortname: String },
    /// A file that could not be parsed. NOT counted as broken: nothing about its
    /// references can be proved. But it is never skipped in silence either — a
    /// skipped file hides every defect inside it.
    Unreadable { file: String, why: String },
}

/// Counts are kept PER KIND. Once there is more than one resolver, a bare "resolved" is ambiguous,
/// so textures, geometry and atlases are counted separately.
#[derive(Debug, Default)]
pub struct AnalysisReport {
    pub entities: usize,

    // textures referenced by client entities
    pub textures_declared: usize, // declared shortnames
    pub texture_files: usize,     // unique physical files (logical identity != physical)
    pub textures_resolved: usize,
    pub textures_external: usize, // provided by the game
    pub textures_missing: usize,

    // geometry
    pub geometry_declared: usize,
    pub geometry_resolved: usize,
    pub geometry_external: usize,
    pub geometry_missing: usize,

    // animations named by client entities (animations map + legacy controllers array)
    pub animations_declared: usize,
    pub animations_resolved: usize,
    pub animations_external: usize,
    pub animations_missing: usize,

    // render controllers named by client entities
    pub render_controllers_declared: usize,
    pub render_controllers_resolved: usize,
    pub render_controllers_external: usize,
    pub render_controllers_missing: usize,

    // materials named by client entities (`"default": "entity_alphatest"`).
    // Not checked without the game's catalog: almost every material is the game's.
    pub materials_declared: usize,
    pub materials_resolved: usize, // defined by a pack's materials/*.material
    pub materials_external: usize, // the game's
    pub materials_missing: usize,
    pub materials_unchecked: usize, // no catalog: nothing provable

    // slots a render controller asks of the entity using it (`Texture.default`, ...)
    pub slots_checked: usize,
    pub slots_resolved: usize,
    pub slots_bounded: usize, // array, some candidates present: runtime picks
    pub slots_opaque: usize,  // Molang expression: not evaluated
    pub slots_missing: usize,

    // atlases (terrain_texture.json / item_texture.json)
    pub atlas_entries: usize,
    pub atlas_paths: usize,
    pub atlas_resolved: usize,
    pub atlas_external: usize,
    pub atlas_missing: usize,
    pub atlas_empty: usize,

    // platform: resolves on Windows only
    pub case_mismatches: usize,

    // files the engine could not read: reported, never silently skipped
    pub unreadable: usize,

    pub issues: Vec<Issue>,
    pub findings: Vec<Finding>,
}

impl AnalysisReport {
    /// Broken everywhere: missing references and atlas entries with no texture.
    pub fn broken(&self) -> usize {
        self.textures_missing
            + self.geometry_missing
            + self.animations_missing
            + self.render_controllers_missing
            + self.materials_missing
            + self.slots_missing
            + self.atlas_missing
            + self.atlas_empty
    }

    /// Fold another report's counts and issues into this one.
    pub fn absorb(&mut self, o: &AnalysisReport) {
        self.entities += o.entities;
        self.textures_declared += o.textures_declared;
        self.texture_files += o.texture_files;
        self.textures_resolved += o.textures_resolved;
        self.textures_external += o.textures_external;
        self.textures_missing += o.textures_missing;
        self.geometry_declared += o.geometry_declared;
        self.geometry_resolved += o.geometry_resolved;
        self.geometry_external += o.geometry_external;
        self.geometry_missing += o.geometry_missing;
        self.animations_declared += o.animations_declared;
        self.animations_resolved += o.animations_resolved;
        self.animations_external += o.animations_external;
        self.animations_missing += o.animations_missing;
        self.render_controllers_declared += o.render_controllers_declared;
        self.render_controllers_resolved += o.render_controllers_resolved;
        self.render_controllers_external += o.render_controllers_external;
        self.render_controllers_missing += o.render_controllers_missing;
        self.materials_declared += o.materials_declared;
        self.materials_resolved += o.materials_resolved;
        self.materials_external += o.materials_external;
        self.materials_missing += o.materials_missing;
        self.materials_unchecked += o.materials_unchecked;
        self.slots_checked += o.slots_checked;
        self.slots_resolved += o.slots_resolved;
        self.slots_bounded += o.slots_bounded;
        self.slots_opaque += o.slots_opaque;
        self.slots_missing += o.slots_missing;
        self.atlas_entries += o.atlas_entries;
        self.atlas_paths += o.atlas_paths;
        self.atlas_resolved += o.atlas_resolved;
        self.atlas_external += o.atlas_external;
        self.atlas_missing += o.atlas_missing;
        self.atlas_empty += o.atlas_empty;
        self.case_mismatches += o.case_mismatches;
        self.unreadable += o.unreadable;
        self.issues.extend(o.issues.iter().cloned());
    }
}
