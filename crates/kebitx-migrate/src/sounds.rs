//! `sounds.json` → `sound_definitions.json`.
//!
//! The two editions name their sound events differently (`entity.cow.ambient`
//! in Java, `mob.cow.say` in Bedrock), and no name rule is proof. The FILES are:
//! a Java event and a Bedrock event that play exactly the same set of files
//! are the same sound. So a Java event is mapped to a Bedrock event only when
//! the Java game's own `sounds.json` gives it a set of files that exactly one
//! Bedrock event of the installed game has, and no other Java event has. Shared
//! sets (stone steps and stone digging play the same files) map to nothing.
//!
//! A mapped event is then rewritten as Java would play it: with `replace`, the
//! pack's sounds only; without it, Bedrock's sounds plus the pack's additions.
//! Bedrock's category, subtitle and distances are kept. A pack sound is carried
//! over only with what means the same in both editions (`volume` up to 1,
//! `pitch`, `weight`, `stream`; `preload` is a loading hint and is dropped).
//! An event that uses anything else stays for a person to adapt.

use crate::report::Verdict;
use kebitx_core::json::{Json, JsonMap};
use kebitx_core::vanilla::VanillaCatalog;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundEvent {
    pub java: String,
    pub verdict: Verdict,
    pub bedrock: Option<String>,
    pub reason: &'static str,
}

#[derive(Debug, Default)]
pub struct SoundsResult {
    pub events: Vec<SoundEvent>,
    /// Bedrock event -> its new definition.
    pub definitions: BTreeMap<String, Json>,
    /// Pack files (`assets/minecraft/sounds/....ogg`) the definitions play.
    pub used_files: HashSet<String>,
}

type FileSet = BTreeSet<String>;

/// Files of a Java event, as Bedrock stems (`sounds/mob/cow/say1`); `None` if
/// it plays another event or a file of another namespace.
fn java_files(def: &Json) -> Option<FileSet> {
    let mut set = FileSet::new();
    for s in def.get("sounds")?.as_array()? {
        let (name, is_event) = match s {
            Json::Str(n) => (n.as_str(), false),
            Json::Obj(_) => (
                s.get("name")?.as_str()?,
                s.get("type").and_then(Json::as_str) == Some("event"),
            ),
            _ => return None,
        };
        if is_event {
            return None;
        }
        set.insert(format!("sounds/{}", java_path(name)?.to_ascii_lowercase()));
    }
    Some(set)
}

/// `minecraft:mob/cow/say1` or `mob/cow/say1` -> `mob/cow/say1`; another
/// namespace -> `None`.
fn java_path(name: &str) -> Option<&str> {
    match name.split_once(':') {
        Some(("minecraft", p)) => Some(p),
        Some(_) => None,
        None => Some(name),
    }
}

fn bedrock_files(def: &Json) -> FileSet {
    let mut set = FileSet::new();
    for s in def
        .get("sounds")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let name = s.as_str().or_else(|| s.get("name").and_then(Json::as_str));
        if let Some(n) = name {
            set.insert(kebitx_core::filesystem::asset_stem(n));
        }
    }
    set
}

/// Java event -> Bedrock event, one to one, proven by identical file sets.
pub fn event_map(java_vanilla: &Json, catalog: &VanillaCatalog) -> HashMap<String, String> {
    let mut java_by_set: HashMap<FileSet, Vec<&String>> = HashMap::new();
    for (event, def) in java_vanilla.as_object().into_iter().flat_map(|o| o.iter()) {
        if let Some(set) = java_files(def).filter(|s| !s.is_empty()) {
            java_by_set.entry(set).or_default().push(event);
        }
    }
    let mut bedrock_by_set: HashMap<FileSet, Vec<String>> = HashMap::new();
    for (event, raw) in catalog.sound_events() {
        if let Ok(def) = kebitx_core::json::parse(raw) {
            let set = bedrock_files(&def);
            if !set.is_empty() {
                bedrock_by_set.entry(set).or_default().push(event.clone());
            }
        }
    }
    let mut map = HashMap::new();
    for (set, java) in java_by_set {
        if let ([j], Some([b])) = (java.as_slice(), bedrock_by_set.get(&set).map(Vec::as_slice)) {
            map.insert(j.to_string(), b.clone());
        }
    }
    map
}

/// One pack sound as a Bedrock sound entry.
fn bedrock_sound(s: &Json) -> Result<(String, Json), &'static str> {
    let name = match s {
        Json::Str(n) => n.as_str(),
        Json::Obj(_) => s
            .get("name")
            .and_then(Json::as_str)
            .ok_or("a sound without a name")?,
        _ => return Err("a sound that is not a name or an object"),
    };
    let path = java_path(name).ok_or("plays a file of another namespace (a mod)")?;
    let bedrock_name = format!("sounds/{path}");
    let Json::Obj(o) = s else {
        return Ok((path.to_string(), Json::Str(bedrock_name)));
    };
    let mut fields = vec![("name".to_string(), Json::Str(bedrock_name))];
    for (k, v) in o.iter() {
        match (k.as_str(), v) {
            ("name", _) | ("preload", _) => {}
            ("type", Json::Str(t)) if t == "file" => {}
            ("type", _) => return Err("plays another sound event: no Bedrock equivalent"),
            ("volume", Json::Num(n)) if *n <= 1.0 => fields.push((k.clone(), v.clone())),
            ("volume", _) => return Err("volume above 1 (in Java it widens the hearing range)"),
            ("pitch" | "weight", Json::Num(_)) | ("stream", Json::Bool(_)) => {
                fields.push((k.clone(), v.clone()))
            }
            _ => {
                return Err(
                    "a sound setting with no Bedrock equivalent (attenuation_distance, ...)",
                )
            }
        }
    }
    if fields.len() == 1 {
        return Ok((path.to_string(), fields.remove(0).1));
    }
    Ok((path.to_string(), Json::Obj(JsonMap::from_entries(fields))))
}

/// Convert a pack's `sounds.json`. `has` tells whether the pack has a file.
pub fn convert(
    pack: &Json,
    java_vanilla: Option<&Json>,
    catalog: &VanillaCatalog,
    has: &dyn Fn(&str) -> bool,
) -> SoundsResult {
    let mut out = SoundsResult::default();
    let map = java_vanilla
        .map(|j| event_map(j, catalog))
        .unwrap_or_default();
    for (event, def) in pack.as_object().into_iter().flat_map(|o| o.iter()) {
        let (verdict, bedrock, reason) =
            match one_event(event, def, java_vanilla, &map, catalog, has) {
                Ok(Written {
                    bedrock,
                    definition: Some((d, files)),
                }) => {
                    out.used_files.extend(files);
                    out.definitions.insert(bedrock.clone(), d);
                    (
                        Verdict::Converts,
                        Some(bedrock),
                        "sound event mapped by its files: rewritten in sound_definitions.json",
                    )
                }
                Ok(Written {
                    bedrock,
                    definition: None,
                }) => (
                    Verdict::Converts,
                    Some(bedrock),
                    "the same sounds Bedrock already plays: nothing to write",
                ),
                Err((bedrock, why)) => (Verdict::Adapts, bedrock, why),
            };
        out.events.push(SoundEvent {
            java: event.clone(),
            verdict,
            bedrock,
            reason,
        });
    }
    out
}

struct Written {
    bedrock: String,
    /// The new definition and the pack files it plays; `None`: no change.
    definition: Option<(Json, Vec<String>)>,
}

type Failed = (Option<String>, &'static str);

fn one_event(
    event: &str,
    def: &Json,
    java_vanilla: Option<&Json>,
    map: &HashMap<String, String>,
    catalog: &VanillaCatalog,
    has: &dyn Fn(&str) -> bool,
) -> Result<Written, Failed> {
    let Some(java_vanilla) = java_vanilla else {
        return Err((
            None,
            "the installed Java game's sounds.json is needed to find the Bedrock event",
        ));
    };
    if java_vanilla.get(event).is_none() {
        return Err((
            None,
            "a new sound event: Bedrock needs an event and something that plays it",
        ));
    }
    let b_event = map.get(event).ok_or((
        None,
        "no Bedrock event plays exactly the same files: needs a mapping",
    ))?;
    let fail = |why| (Some(b_event.clone()), why);
    let b_def = catalog
        .sound_definition(b_event)
        .and_then(|r| kebitx_core::json::parse(r).ok())
        .ok_or_else(|| fail("the Bedrock event could not be read"))?;
    let replace = matches!(def.get("replace"), Some(Json::Bool(true)));
    let sounds = def
        .get("sounds")
        .and_then(Json::as_array)
        .ok_or_else(|| fail("an event without a sounds list"))?;
    // What Java ends up playing: the pack's list with `replace`, the game's
    // list plus the pack's without. When that is what Java plays anyway — the
    // same list, or the whole list once more (same odds) — Bedrock's own
    // event already is the same sound: nothing to write.
    let vanilla = java_vanilla
        .get(event)
        .and_then(|d| d.get("sounds"))
        .and_then(Json::as_array);
    let same_as_vanilla = vanilla == Some(sounds);
    if (replace && same_as_vanilla) || (!replace && (sounds.is_empty() || same_as_vanilla)) {
        return Ok(Written {
            bedrock: b_event.clone(),
            definition: None,
        });
    }
    let converted: Vec<(String, Json)> = sounds
        .iter()
        .map(bedrock_sound)
        .collect::<Result<_, _>>()
        .map_err(fail)?;
    let pack_file = |path: &str| format!("assets/minecraft/sounds/{path}.ogg");
    let in_game =
        |path: &str| catalog.has_asset_stem(&format!("sounds/{}", path.to_ascii_lowercase()));
    if converted
        .iter()
        .any(|(path, _)| !has(&pack_file(path)) && !in_game(path))
    {
        return Err(fail("plays a file neither the pack nor the game has"));
    }
    // Bedrock's own sounds stand for Java's (they are the same files), then
    // the pack's are added as Java adds them
    let mut list: Vec<Json> = if replace {
        Vec::new()
    } else {
        b_def
            .get("sounds")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default()
    };
    list.extend(converted.iter().map(|(_, entry)| entry.clone()));
    let files = converted
        .iter()
        .map(|(p, _)| pack_file(p))
        .filter(|f| has(f))
        .collect();
    let mut fields: Vec<(String, Json)> = b_def
        .as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    fields.retain(|(k, _)| k != "sounds");
    fields.push(("sounds".into(), Json::Arr(list)));
    Ok(Written {
        bedrock: b_event.clone(),
        definition: Some((Json::Obj(JsonMap::from_entries(fields)), files)),
    })
}

/// The `sound_definitions.json` text for these definitions.
pub fn definitions_file(defs: &BTreeMap<String, Json>) -> String {
    let body = Json::Obj(JsonMap::from_entries(
        defs.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    ));
    let root = Json::Obj(JsonMap::from_entries(vec![
        ("format_version".into(), Json::Str("1.14.0".into())),
        ("sound_definitions".into(), body),
    ]));
    kebitx_core::json::to_string(&root)
}
