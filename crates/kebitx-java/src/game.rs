//! The installed Java Edition, as a reference: its own `sounds.json` says which
//! files each Java sound event plays, and that is what proves which Bedrock
//! event is the same one.
//!
//! Java keeps its assets in the launcher's object store: an index
//! (`assets/indexes/<id>.json`) maps each name to a SHA-1, and the file lives
//! at `assets/objects/<first two hex digits>/<sha1>`. The newest numeric index
//! is used. Nothing is downloaded: without an installed Java there is no
//! reference, and the report says so.

use kebitx_core::codec::zip::ZipArchive;
use kebitx_core::json::{self, Json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Asset indexes are a few hundred KB; this is far above any real one.
const INDEX_MAX_BYTES: u64 = 64 * 1024 * 1024;

pub struct JavaAssets {
    pub dir: PathBuf,
    /// The asset index used (`34`).
    pub index: String,
    objects: HashMap<String, String>,
}

impl JavaAssets {
    /// `KEBITX_JAVA_DIR`, else the launcher's usual folder for this system.
    pub fn discover() -> Option<Self> {
        Self::launcher_dirs()
            .into_iter()
            .find_map(|d| Self::open(&d))
    }

    /// Where the launcher may keep the game: `KEBITX_JAVA_DIR`, or else the usual
    /// folders for each system.
    pub fn launcher_dirs() -> Vec<PathBuf> {
        if let Some(d) = std::env::var_os("KEBITX_JAVA_DIR") {
            return vec![PathBuf::from(d)];
        }
        let home = |v: &str| std::env::var_os(v).map(PathBuf::from);
        [
            home("APPDATA").map(|a| a.join(".minecraft")),
            home("HOME").map(|h| h.join(".minecraft")),
            home("HOME").map(|h| h.join("Library/Application Support/minecraft")),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// A launcher folder (the one holding `assets/`).
    pub fn open(dir: &Path) -> Option<Self> {
        let indexes = dir.join("assets").join("indexes");
        let (n, path) = std::fs::read_dir(&indexes)
            .ok()?
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let n: u32 = p.file_stem()?.to_str()?.parse().ok()?;
                (p.extension()? == "json").then_some((n, p))
            })
            .max_by_key(|(n, _)| *n)?;
        let text =
            String::from_utf8(kebitx_core::filesystem::read_limited(&path, INDEX_MAX_BYTES).ok()?)
                .ok()?;
        let root = json::parse(&text).ok()?;
        let mut objects = HashMap::new();
        for (name, o) in root.get("objects")?.as_object()?.iter() {
            if let Some(h) = o.get("hash").and_then(Json::as_str) {
                // a hash is 40 hex digits and nothing else: it becomes a path
                if h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
                    objects.insert(name.clone(), h.to_ascii_lowercase());
                }
            }
        }
        Some(Self {
            dir: dir.to_path_buf(),
            index: n.to_string(),
            objects,
        })
    }

    /// Where an asset (`minecraft/sounds.json`) is stored, if it is there.
    pub fn object(&self, name: &str) -> Option<PathBuf> {
        let h = self.objects.get(name)?;
        let p = self
            .dir
            .join("assets")
            .join("objects")
            .join(&h[..2])
            .join(h);
        p.is_file().then_some(p)
    }

    /// Every asset name of the index.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.objects.keys()
    }

    /// The game's own `sounds.json`.
    pub fn sounds_json(&self) -> Option<Json> {
        let p = self.object("minecraft/sounds.json")?;
        json::parse(&kebitx_core::filesystem::read_text(&p).ok()?).ok()
    }

    /// The installed game version (`versions/<id>/<id>.jar`) that matches a
    /// pack: the one whose `version.json` declares that resource pack format;
    /// with none, the newest stable one (`exact` says which happened).
    pub fn jar_for(&self, pack_format: Option<u32>) -> Option<JavaJar> {
        self.jar_in(pack_format.map(|f| (f, f)))
    }

    /// As `jar_for`, for a pack that declares a range of formats
    /// (`min_format` .. `max_format`): the newest installed version inside it.
    pub fn jar_in(&self, formats: Option<(u32, u32)>) -> Option<JavaJar> {
        let mut found: Vec<Installed> = self
            .every_version()
            .into_iter()
            .filter(|v| v.format.is_some())
            .collect();
        let newest = |found: &[Installed], keep: &dyn Fn(&Installed) -> bool| {
            found
                .iter()
                .enumerate()
                .filter(|(_, v)| keep(v))
                .max_by(|a, b| a.1.built.cmp(&b.1.built))
                .map(|(i, _)| i)
        };
        let inside = formats.and_then(|(min, max)| {
            newest(&found, &|v| {
                v.format.is_some_and(|f| (min..=max).contains(&f))
            })
        });
        let (pick, exact) = match inside {
            Some(i) => (found.swap_remove(i), true),
            None => (found.swap_remove(newest(&found, &|v| v.stable)?), false),
        };
        Some(JavaJar {
            id: pick.id,
            format: pick.format.unwrap_or(0),
            exact,
            zip: pick.zip,
        })
    }

    /// Every installed version with a `.jar`: `(id, stable)`, newest first
    /// (by the build time its `version.json` declares).
    pub fn installed(&self) -> Vec<(String, bool)> {
        let mut all = self.every_version();
        all.sort_by(|a, b| b.built.cmp(&a.built));
        all.into_iter().map(|v| (v.id, v.stable)).collect()
    }

    /// Every version folder with a `.jar` that says what it is.
    fn every_version(&self) -> Vec<Installed> {
        let Ok(entries) = std::fs::read_dir(self.dir.join("versions")) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().to_string();
                let zip = ZipArchive::open(&e.path().join(format!("{id}.jar"))).ok()?;
                let bytes = zip.read("version.json", 1024 * 1024).ok()?;
                let v = json::parse(&String::from_utf8_lossy(&bytes)).ok()?;
                let pv = v.get("pack_version");
                let format = pv
                    .and_then(|p| p.get("resource_major").or_else(|| p.get("resource")))
                    .or(pv.filter(|p| matches!(p, Json::Num(_))));
                Some(Installed {
                    format: match format {
                        Some(Json::Num(f)) => Some(*f as u32),
                        _ => None,
                    },
                    stable: matches!(v.get("stable"), Some(Json::Bool(true))),
                    built: v
                        .get("build_time")
                        .and_then(Json::as_str)
                        .unwrap_or("")
                        .to_string(),
                    id,
                    zip,
                })
            })
            .collect()
    }

    /// One installed version by its id (`26.2`), if its `.jar` is there.
    pub fn jar_by_id(&self, id: &str, exact: bool) -> Option<JavaJar> {
        if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") {
            return None;
        }
        let zip =
            ZipArchive::open(&self.dir.join("versions").join(id).join(format!("{id}.jar"))).ok()?;
        Some(JavaJar {
            id: id.to_string(),
            format: 0,
            exact,
            zip,
        })
    }

    /// The installed version a mod's Minecraft requirement names, when it
    /// names exactly one: `26.2`, `~26.2`, `=26.2`, `26.2.x`, `[1.21.1]`.
    /// A range (`>=26.2 <26.4`, `[1.21,1.22)`) is not resolved here.
    pub fn jar_for_requirement(&self, requirement: &str) -> Option<JavaJar> {
        let r = requirement.trim();
        let bare = r
            .trim_start_matches(['~', '='])
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim_end_matches(".x")
            .trim();
        let simple = !bare.is_empty()
            && bare.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            && (r.starts_with('[') == r.ends_with(']'));
        if !simple {
            return None;
        }
        self.jar_by_id(bare, true)
    }
}

/// What a version's `version.json` says, with its open `.jar`.
struct Installed {
    id: String,
    /// The resource pack format it declares.
    format: Option<u32>,
    stable: bool,
    built: String,
    zip: ZipArchive,
}

/// One installed Java version's client `.jar`: the game's own textures and
/// `en_us.json` live there, not in the asset index.
pub struct JavaJar {
    pub id: String,
    /// The resource pack format it declares.
    pub format: u32,
    /// It declares the pack's own format (false: the newest stable one stood in).
    pub exact: bool,
    pub zip: ZipArchive,
}

impl JavaJar {
    /// An asset's bytes (`assets/minecraft/lang/en_us.json`), up to 64 MiB.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.zip.read(name, 64 * 1024 * 1024).ok()
    }
}
