//! What a mod declares, from whichever loader's file it has: the one reader
//! of `fabric.mod.json`, `quilt.mod.json` and `mods.toml`.
//!
//! [`read`] is the jar's own metadata, and says so even when it is malformed:
//! the file makes it a mod. [`meta_of`] is what the loader would accept, with
//! the mods bundled inside it.

use crate::source::Source;
use crate::version::{fabric_any, maven_matches};
use kebitx_core::json::{self, Json};
use std::collections::BTreeMap;

/// A version requirement, in the loader's own syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Req {
    /// Fabric / Quilt predicates, any of them.
    Fabric(Vec<String>),
    /// A Maven range (Forge, NeoForge).
    Maven(String),
}

impl Req {
    pub fn matches(&self, version: &str) -> Option<bool> {
        match self {
            Req::Fabric(p) => fabric_any(p, version),
            Req::Maven(r) => maven_matches(r, version),
        }
    }
}

impl std::fmt::Display for Req {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Req::Fabric(p) => write!(f, "{}", p.join(" || ")),
            Req::Maven(r) => write!(f, "{r}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub id: String,
    pub req: Req,
}

/// What a mod's metadata declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModMeta {
    pub loader: &'static str,
    /// Its id; empty when the metadata is malformed.
    pub id: String,
    /// Every mod the jar declares (several `[[mods]]`): its own namespaces.
    pub ids: Vec<String>,
    /// The name it shows, when it gives one.
    pub name: Option<String>,
    pub version: String,
    /// The other ids it answers to (the rest of `ids` among them).
    pub provides: Vec<String>,
    pub depends: Vec<Dep>,
    pub breaks: Vec<Dep>,
    /// Every dependency it names, in order, required or not.
    pub named: Vec<String>,
    /// The Minecraft versions it declares it is for, as written (`[1.21.1]`,
    /// `>=1.21 <1.22`).
    pub minecraft: Option<String>,
    /// `(id, version)` of the mods bundled inside it ([`meta_of`] only).
    pub bundled: Vec<(String, String)>,
}

impl ModMeta {
    /// The name to show: its own, else its id, else what it is.
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .filter(|n| !n.is_empty())
            .or_else(|| Some(self.id.clone()).filter(|i| !i.is_empty()))
            .unwrap_or_else(|| format!("{} mod", self.loader))
    }
}

fn str_of(j: &Json, key: &str) -> Option<String> {
    j.get(key).and_then(Json::as_str).map(str::to_string)
}

/// A version requirement: a string, or a list of alternatives.
fn predicates(v: &Json) -> Vec<String> {
    match v {
        Json::Str(s) => vec![s.clone()],
        Json::Arr(a) => a
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Predicates as the mod wrote them, when it wrote any.
fn written(p: &Req) -> Option<String> {
    Some(p.to_string()).filter(|s| !s.is_empty())
}

fn minecraft_in(deps: &[Dep]) -> Option<String> {
    deps.iter()
        .filter(|d| d.id == "minecraft")
        .find_map(|d| written(&d.req))
}

fn fabric_deps(j: &Json, key: &str) -> Vec<Dep> {
    j.get(key)
        .and_then(Json::as_object)
        .map(|o| {
            o.iter()
                .map(|(id, v)| Dep {
                    id: id.clone(),
                    req: Req::Fabric(predicates(v)),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `fabric.mod.json`: id, name, version, provides, depends, breaks.
pub(crate) fn fabric_meta(j: &Json) -> Option<ModMeta> {
    let id = str_of(j, "id")?;
    let depends = fabric_deps(j, "depends");
    Some(ModMeta {
        loader: "Fabric",
        ids: vec![id.clone()],
        id,
        name: str_of(j, "name"),
        version: str_of(j, "version").unwrap_or_default(),
        provides: j
            .get("provides")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect(),
        breaks: fabric_deps(j, "breaks"),
        named: depends.iter().map(|d| d.id.clone()).collect(),
        minecraft: minecraft_in(&depends),
        depends,
        bundled: Vec::new(),
    })
}

/// `quilt.mod.json`: the same, under `quilt_loader`, and dependencies as a
/// list of ids or of `{id, versions}`.
fn quilt_meta(j: &Json) -> Option<ModMeta> {
    let q = j.get("quilt_loader")?;
    let deps = |key: &str| -> Vec<Dep> {
        q.get(key)
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|d| match d {
                Json::Str(id) => Some(Dep {
                    id: id.clone(),
                    req: Req::Fabric(vec!["*".into()]),
                }),
                o => Some(Dep {
                    id: str_of(o, "id")?,
                    req: Req::Fabric(
                        o.get("versions")
                            .map(predicates)
                            .unwrap_or_else(|| vec!["*".into()]),
                    ),
                }),
            })
            .collect()
    };
    let id = str_of(q, "id")?;
    let depends = deps("depends");
    Some(ModMeta {
        loader: "Quilt",
        ids: vec![id.clone()],
        id,
        name: q.get("metadata").and_then(|m| str_of(m, "name")),
        version: str_of(q, "version").unwrap_or_default(),
        provides: q
            .get("provides")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_str().map(str::to_string).or_else(|| str_of(p, "id")))
            .collect(),
        breaks: deps("breaks"),
        named: depends.iter().map(|d| d.id.clone()).collect(),
        minecraft: minecraft_in(&depends),
        depends,
        bundled: Vec::new(),
    })
}

/// The tables of a `mods.toml`: `(header, key -> value)`, values unquoted.
/// A multi-line string (a `description`) is skipped over: its lines are text,
/// even the ones that look like a table or a key.
fn toml_tables(t: &str) -> Vec<(String, BTreeMap<String, String>)> {
    let mut out: Vec<(String, BTreeMap<String, String>)> = vec![(String::new(), BTreeMap::new())];
    let mut until = None;
    for l in t.lines().map(str::trim) {
        if let Some(close) = until {
            if l.contains(close) {
                until = None;
            }
            continue;
        }
        if l.starts_with('#') {
            continue;
        }
        if l.starts_with('[') {
            let head = l.split('#').next().unwrap_or("").trim();
            out.push((
                head.trim_start_matches('[')
                    .trim_end_matches(']')
                    .trim()
                    .to_string(),
                BTreeMap::new(),
            ));
            continue;
        }
        let Some((k, v)) = l.split_once('=') else {
            continue;
        };
        let v = v.trim();
        let value = if let Some(q) = ["'''", "\"\"\""].into_iter().find(|q| v.starts_with(q)) {
            let rest = &v[q.len()..];
            if !rest.contains(q) {
                until = Some(q);
            }
            rest.split(q).next().unwrap_or("")
        } else if let Some(q) = v.chars().next().filter(|c| matches!(c, '"' | '\'')) {
            // a quoted value ends at its quote
            v[1..].split(q).next().unwrap_or("")
        } else {
            v.split('#').next().unwrap_or("").trim()
        };
        if let Some(last) = out.last_mut() {
            last.1.insert(k.trim().to_string(), value.to_string());
        }
    }
    out
}

/// `META-INF/(neoforge.)mods.toml`: its `[[mods]]`, the first one naming the
/// jar, and their `[[dependencies.<id>]]` (`type = "required"` /
/// `"incompatible"` on NeoForge, `mandatory = true` on Forge).
fn toml_meta(t: &str, loader: &'static str, manifest_version: Option<String>) -> Option<ModMeta> {
    let tables = toml_tables(t);
    let mut mods = tables.iter().filter(|(h, _)| h == "mods");
    let (_, m) = mods.next()?;
    let mut version = m.get("version").cloned().unwrap_or_default();
    if version.contains("${") {
        version = manifest_version.unwrap_or_default();
    }
    let id = m.get("modId").filter(|i| !i.is_empty())?.clone();
    let provides: Vec<String> = mods
        .filter_map(|(_, m)| m.get("modId"))
        .filter(|i| !i.is_empty())
        .cloned()
        .collect();
    let mut meta = ModMeta {
        loader,
        ids: std::iter::once(id.clone())
            .chain(provides.iter().cloned())
            .collect(),
        id,
        name: m.get("displayName").cloned(),
        version,
        provides,
        ..Default::default()
    };
    for (h, kv) in &tables {
        if !h.starts_with("dependencies") {
            continue;
        }
        let (Some(id), range) = (kv.get("modId"), kv.get("versionRange")) else {
            continue;
        };
        if !id.is_empty() && !meta.named.contains(id) {
            meta.named.push(id.clone());
        }
        if id == "minecraft" && meta.minecraft.is_none() {
            meta.minecraft = range.cloned();
        }
        let dep = Dep {
            id: id.clone(),
            req: Req::Maven(range.cloned().unwrap_or_default()),
        };
        let kind = kv.get("type").map(|s| s.to_ascii_lowercase());
        let required = match kind.as_deref() {
            Some("required") => true,
            Some(_) => false,
            None => kv.get("mandatory").is_some_and(|v| v == "true"),
        };
        if required {
            meta.depends.push(dep);
        } else if kind.as_deref() == Some("incompatible") {
            meta.breaks.push(dep);
        }
    }
    Some(meta)
}

/// `Implementation-Version` of a jar's manifest (what `${file.jarVersion}`
/// stands for).
fn manifest_version(src: &Source) -> Option<String> {
    let m = String::from_utf8_lossy(&src.read("META-INF/MANIFEST.MF")?).to_string();
    m.lines()
        .find_map(|l| l.strip_prefix("Implementation-Version:"))
        .map(|v| v.trim().to_string())
}

/// The metadata files, in the order they are looked at. One jar can carry
/// several, for several loaders.
const FILES: [(&str, &str); 4] = [
    ("fabric.mod.json", "Fabric"),
    ("quilt.mod.json", "Quilt"),
    ("META-INF/neoforge.mods.toml", "NeoForge"),
    ("META-INF/mods.toml", "Forge"),
];

/// A source's own metadata: `None` when it has no such file (not a mod). The
/// first file that can be understood wins; when none can, the first one still
/// makes it a mod, with an empty `id`.
pub fn read(src: &Source) -> Option<ModMeta> {
    let mut malformed = None;
    for (file, loader) in FILES {
        let Some(bytes) = src.read(file) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let parsed = match loader {
            "Fabric" => json::parse(&text).ok().as_ref().and_then(fabric_meta),
            "Quilt" => json::parse(&text).ok().as_ref().and_then(quilt_meta),
            _ => toml_meta(&text, loader, manifest_version(src)),
        };
        match parsed {
            Some(m) => return Some(m),
            None => {
                malformed = malformed.or(Some(ModMeta {
                    loader,
                    ..Default::default()
                }))
            }
        }
    }
    malformed
}

/// A mod's metadata as the loader takes it: `None` when it has none it could
/// load, and with what it bundles.
pub fn meta_of(src: &Source) -> Option<ModMeta> {
    let mut meta = read(src).filter(|m| !m.id.is_empty())?;
    src.each_bundled(&src.files(), &mut |inner, _| {
        if let Some(m) = bundled_fabric(inner) {
            meta.bundled.push((m.id.clone(), m.version.clone()));
            meta.bundled
                .extend(m.provides.iter().map(|p| (p.clone(), m.version.clone())));
        }
    });
    Some(meta)
}

/// The metadata of a bundled jar: only Fabric's nest this way.
pub(crate) fn bundled_fabric(inner: &Source) -> Option<ModMeta> {
    inner
        .read("fabric.mod.json")
        .and_then(|b| json::parse(&String::from_utf8_lossy(&b)).ok())
        .as_ref()
        .and_then(fabric_meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_metadata_with_its_dependency_kinds() {
        let t = r#"
modLoader="javafml"
[[mods]] # the mod
modId="farmersdelight"
displayName="Farmer's Delight" # shown
version="${file.jarVersion}"
[[dependencies.farmersdelight]]
    modId="neoforge"
    type="required"
    versionRange="[21.1.219,)"
[[dependencies.farmersdelight]]
    modId = "minecraft"
    type = "required"
    versionRange = "[1.21.1]" # exact
[[dependencies.farmersdelight]]
    modId="crafttweaker"
    type="optional"
    versionRange="[14.0.6,)"
[[dependencies.farmersdelight]]
    modId="badmod"
    type="incompatible"
    versionRange="[1.0,)"
"#;
        let m = toml_meta(t, "NeoForge", Some("1.21.1-1.3.4".into())).unwrap();
        assert_eq!(
            (m.id.as_str(), m.version.as_str()),
            ("farmersdelight", "1.21.1-1.3.4")
        );
        assert_eq!(m.ids, ["farmersdelight"]);
        assert_eq!(m.display_name(), "Farmer's Delight");
        assert_eq!(m.minecraft.as_deref(), Some("[1.21.1]"));
        let ids: Vec<&str> = m.depends.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["neoforge", "minecraft"]);
        assert_eq!(
            m.named,
            ["neoforge", "minecraft", "crafttweaker", "badmod"],
            "the dependency tables' modId names what it needs, not the mod"
        );
        assert_eq!(m.breaks[0].id, "badmod");
        assert_eq!(m.depends[1].req.matches("1.21.1"), Some(true));
        assert_eq!(m.depends[1].req.matches("26.3"), Some(false));
    }

    #[test]
    fn toml_strings_in_every_form() {
        let t = "
[[mods]]
modId='single'
version=\"1\"
description='''
[not a table]
modId=\"not_the_id\"
'''
displayName = '''Inline'''
[[mods]]
modId=\"second\"
[[dependencies.single]]
modId='minecraft'
mandatory=true
versionRange='[1.21,)'
";
        let m = toml_meta(t, "Forge", None).unwrap();
        assert_eq!(m.id, "single");
        assert_eq!(m.display_name(), "Inline");
        assert_eq!(m.ids, ["single", "second"]);
        assert_eq!(
            m.provides,
            ["second"],
            "a second mod of the jar is provided"
        );
        assert_eq!(m.minecraft.as_deref(), Some("[1.21,)"));
        assert_eq!(m.depends.len(), 1);
    }

    #[test]
    fn a_name_falls_back_to_the_id_then_to_the_loader() {
        let j = json::parse(r#"{"id":"demo"}"#).unwrap();
        assert_eq!(fabric_meta(&j).unwrap().display_name(), "demo");
        let m = ModMeta {
            loader: "Quilt",
            ..Default::default()
        };
        assert_eq!(m.display_name(), "Quilt mod");
    }

    #[test]
    fn fabric_and_quilt_declare_the_same_things() {
        let f = json::parse(
            r#"{"id":"a","name":"A","version":"1","provides":["b"],
                "depends":{"minecraft":[">=26.1","26.3"],"fabric-api":"*"}}"#,
        )
        .unwrap();
        let f = fabric_meta(&f).unwrap();
        assert_eq!(f.minecraft.as_deref(), Some(">=26.1 || 26.3"));
        assert_eq!(f.provides, ["b"]);
        assert!(f.named.iter().any(|n| n == "fabric-api"));

        let q = json::parse(
            r#"{"quilt_loader":{"id":"a","version":"1","metadata":{"name":"A"},
                "provides":["b",{"id":"c"}],
                "depends":["fabric-api",{"id":"minecraft","versions":">=26.1"}]}}"#,
        )
        .unwrap();
        let q = quilt_meta(&q).unwrap();
        assert_eq!(q.display_name(), "A");
        assert_eq!(q.minecraft.as_deref(), Some(">=26.1"));
        assert_eq!(q.provides, ["b", "c"]);
        assert_eq!(q.named, ["fabric-api", "minecraft"]);
    }
}
