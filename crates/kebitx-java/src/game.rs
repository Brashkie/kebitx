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
        if let Some(d) = std::env::var_os("KEBITX_JAVA_DIR") {
            return Self::open(Path::new(&d));
        }
        let home = |v: &str| std::env::var_os(v).map(PathBuf::from);
        let candidates = [
            home("APPDATA").map(|a| a.join(".minecraft")),
            home("HOME").map(|h| h.join(".minecraft")),
            home("HOME").map(|h| h.join("Library/Application Support/minecraft")),
        ];
        candidates
            .into_iter()
            .flatten()
            .find_map(|d| Self::open(&d))
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
        let mut found: Vec<(String, u32, bool, String, ZipArchive)> = Vec::new();
        for e in std::fs::read_dir(self.dir.join("versions")).ok()?.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            let Ok(zip) = ZipArchive::open(&e.path().join(format!("{id}.jar"))) else {
                continue;
            };
            let Some(v) = zip
                .read("version.json", 1024 * 1024)
                .ok()
                .and_then(|b| json::parse(&String::from_utf8_lossy(&b)).ok())
            else {
                continue;
            };
            let pv = v.get("pack_version");
            let format = pv
                .and_then(|p| p.get("resource_major").or_else(|| p.get("resource")))
                .or(pv.filter(|p| matches!(p, Json::Num(_))));
            let Some(Json::Num(format)) = format else {
                continue;
            };
            let stable = matches!(v.get("stable"), Some(Json::Bool(true)));
            let built = v
                .get("build_time")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string();
            found.push((id, *format as u32, stable, built, zip));
        }
        let exact = formats.and_then(|(min, max)| {
            let i = found
                .iter()
                .enumerate()
                .filter(|(_, j)| (min..=max).contains(&j.1))
                .max_by(|a, b| a.1 .3.cmp(&b.1 .3))?
                .0;
            Some(found.swap_remove(i))
        });
        let (pick, is_exact) = match exact {
            Some(j) => (j, true),
            None => {
                let i = found
                    .iter()
                    .enumerate()
                    .filter(|(_, j)| j.2)
                    .max_by(|a, b| a.1 .3.cmp(&b.1 .3))?
                    .0;
                (found.swap_remove(i), false)
            }
        };
        Some(JavaJar {
            id: pick.0,
            format: pick.1,
            exact: is_exact,
            zip: pick.4,
        })
    }

    /// Every installed version with a `.jar`: `(id, stable)`, newest first
    /// (by the build time its `version.json` declares).
    pub fn installed(&self) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool, String)> = Vec::new();
        let Ok(entries) = std::fs::read_dir(self.dir.join("versions")) else {
            return Vec::new();
        };
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            let Ok(zip) = ZipArchive::open(&e.path().join(format!("{id}.jar"))) else {
                continue;
            };
            let Some(v) = zip
                .read("version.json", 1024 * 1024)
                .ok()
                .and_then(|b| json::parse(&String::from_utf8_lossy(&b)).ok())
            else {
                continue;
            };
            let stable = matches!(v.get("stable"), Some(Json::Bool(true)));
            let built = v
                .get("build_time")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string();
            out.push((id, stable, built));
        }
        out.sort_by(|a, b| b.2.cmp(&a.2));
        out.into_iter().map(|(id, s, _)| (id, s)).collect()
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
