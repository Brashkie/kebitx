//! Java language files (`lang/es_es.json`) → Bedrock ones (`texts/es_ES.lang`).
//!
//! The two editions key their strings differently (`block.minecraft.stone`
//! is `tile.stone.name`). The proof is the English text: a Java key and a
//! Bedrock key whose `en_us` / `en_US` strings are the same — and that string
//! appears once on each side — are the same string. Identical English also
//! means identical placeholders (`%s`, `%1$s`), so a translation carries over
//! as it is. A Java language maps to the Bedrock language of the same code
//! (case aside) in the game's `languages.json`; others stay to be adapted.

use kebitx_core::json::Json;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// The Bedrock game's English strings and its languages.
#[derive(Default)]
pub struct BedrockLang {
    pub en: HashMap<String, String>,
    pub languages: Vec<String>,
    /// The game's own `.lang` text of every language, by lowercased code.
    raw: HashMap<String, String>,
}

impl BedrockLang {
    pub fn from_install(install: &Path) -> Self {
        let mut files = kebitx_core::vanilla::files::map_files(
            install,
            |p| p.starts_with("texts/") && (p.ends_with(".lang") || p == "texts/languages.json"),
            |_, b| Some(String::from_utf8_lossy(b).to_string()),
        );
        let en = files
            .get("texts/en_us.lang")
            .map(|t| parse_lang(t))
            .unwrap_or_default();
        let languages = files
            .remove("texts/languages.json")
            .and_then(|t| kebitx_core::json::parse(&t).ok())
            .and_then(|j| {
                j.as_array().map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
            })
            .unwrap_or_default();
        let raw = files
            .into_iter()
            .filter_map(|(p, t)| {
                Some((
                    p.strip_prefix("texts/")?.strip_suffix(".lang")?.to_string(),
                    t,
                ))
            })
            .collect();
        BedrockLang { en, languages, raw }
    }

    /// The Bedrock code of a Java language code (`es_es` -> `es_ES`).
    pub fn language(&self, java_code: &str) -> Option<&str> {
        self.languages
            .iter()
            .find(|l| l.eq_ignore_ascii_case(java_code))
            .map(String::as_str)
    }

    /// The game's own strings in a language.
    pub fn own(&self, code: &str) -> HashMap<String, String> {
        self.raw
            .get(&code.to_ascii_lowercase())
            .map(|t| parse_lang(t))
            .unwrap_or_default()
    }
}

/// `key=value` lines; `##` starts a comment, and so does a tab before `#`.
pub fn parse_lang(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}');
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.split("\t#").next().unwrap_or(v);
        let v = v.split("##").next().unwrap_or(v);
        out.insert(k.trim().to_string(), v.trim_end().to_string());
    }
    out
}

/// Java key -> Bedrock key, one to one, proven by identical English.
pub fn key_map(java_en: &Json, bedrock: &BedrockLang) -> HashMap<String, String> {
    let mut java_by_text: HashMap<&str, Vec<&String>> = HashMap::new();
    for (k, v) in java_en.as_object().into_iter().flat_map(|o| o.iter()) {
        if let Some(t) = v.as_str().filter(|t| !t.trim().is_empty()) {
            java_by_text.entry(t).or_default().push(k);
        }
    }
    let mut bedrock_by_text: HashMap<&str, Vec<&String>> = HashMap::new();
    for (k, v) in &bedrock.en {
        if !v.trim().is_empty() {
            bedrock_by_text.entry(v.as_str()).or_default().push(k);
        }
    }
    let mut map = HashMap::new();
    for (text, java) in java_by_text {
        let bedrock_keys = bedrock_by_text.get(text).map(Vec::as_slice).unwrap_or(&[]);
        if let ([j], [b]) = (java.as_slice(), bedrock_keys) {
            map.insert(j.to_string(), b.to_string());
            continue;
        }
        // A text both sides use more than once ("Apple" is also a join-code
        // icon): still one to one within one family of keys (a block is a
        // tile, an item an item), or nothing.
        for (java_family, bedrock_family) in FAMILIES {
            let j: Vec<&&String> = java.iter().filter(|k| k.starts_with(java_family)).collect();
            let b: Vec<&&String> = bedrock_keys
                .iter()
                .filter(|k| k.starts_with(bedrock_family))
                .collect();
            if let ([j], [b]) = (j.as_slice(), b.as_slice()) {
                map.insert(j.to_string(), b.to_string());
            }
        }
    }
    map
}

/// Java key families and the Bedrock family of the same things.
const FAMILIES: [(&str, &str); 5] = [
    ("block.", "tile."),
    ("item.", "item."),
    ("entity.", "entity."),
    ("enchantment.", "enchantment."),
    ("biome.", "biome."),
];

/// What one Java language file becomes.
#[derive(Debug, Default)]
pub struct LangResult {
    /// The Bedrock language code, if the game has it.
    pub bedrock: Option<String>,
    /// Bedrock key -> text.
    pub lines: BTreeMap<String, String>,
    pub mapped: usize,
    pub unmapped: usize,
    /// Texts a `.lang` line cannot hold (line breaks).
    pub unwritable: usize,
}

pub fn convert(
    java_code: &str,
    pack_lang: &Json,
    map: &HashMap<String, String>,
    bedrock: &BedrockLang,
) -> LangResult {
    let mut r = LangResult {
        bedrock: bedrock.language(java_code).map(str::to_string),
        ..Default::default()
    };
    let own = r
        .bedrock
        .as_deref()
        .map(|c| bedrock.own(c))
        .unwrap_or_default();
    for (k, v) in pack_lang.as_object().into_iter().flat_map(|o| o.iter()) {
        let Some(text) = v.as_str() else { continue };
        match map.get(k) {
            None => r.unmapped += 1,
            Some(_) if text.contains(['\n', '\r']) => r.unwritable += 1,
            Some(b) => {
                r.mapped += 1;
                // the game already says exactly this: nothing to write
                if own.get(b).map(String::as_str) != Some(text) {
                    r.lines.insert(b.clone(), text.to_string());
                }
            }
        }
    }
    r
}

/// The `.lang` text for these lines.
pub fn lang_file(lines: &BTreeMap<String, String>) -> String {
    let mut s = String::from("## converted from Java by kebitx\n");
    for (k, v) in lines {
        s.push_str(k);
        s.push('=');
        s.push_str(v);
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_lines_and_comments() {
        let m = parse_lang(
            "## header\ntile.stone.name=Stone\t#\nitem.apple.name=Apple ## note\n\nbad line\n",
        );
        assert_eq!(m.get("tile.stone.name").map(String::as_str), Some("Stone"));
        assert_eq!(m.get("item.apple.name").map(String::as_str), Some("Apple"));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn keys_map_only_by_unique_identical_english() {
        let java = kebitx_core::json::parse(
            r#"{"block.minecraft.stone":"Stone","item.minecraft.apple":"Apple","item.minecraft.bread":"Bread","a":"Same","b":"Same","c":"Lonely"}"#,
        )
        .unwrap();
        let mut bedrock = BedrockLang::default();
        bedrock
            .raw
            .insert("es_es".into(), "tile.stone.name=Roca\nx=Sola\n".into());
        for (k, v) in [
            ("tile.stone.name", "Stone"),
            ("item.apple.name", "Apple"),
            ("joincode.icon_text.12", "Apple"),
            ("x", "Same"),
            ("item.bread.name", "Bread"),
            ("item.loaf.name", "Bread"),
        ] {
            bedrock.en.insert(k.into(), v.into());
        }
        bedrock.languages = vec!["en_US".into(), "es_ES".into()];
        let map = key_map(&java, &bedrock);
        assert_eq!(
            map.get("block.minecraft.stone").map(String::as_str),
            Some("tile.stone.name")
        );
        // "Apple" twice in Bedrock, once among items: the family decides
        assert_eq!(
            map.get("item.minecraft.apple").map(String::as_str),
            Some("item.apple.name")
        );
        assert!(
            !map.contains_key("item.minecraft.bread"),
            "two items say Bread: no proof"
        );
        assert!(!map.contains_key("a"), "Same twice in Java");
        assert!(!map.contains_key("c"));

        let es = kebitx_core::json::parse(
            r#"{"block.minecraft.stone":"Piedra","c":"Sola","block.minecraft.x":"a\nb"}"#,
        )
        .unwrap();
        let r = convert("es_es", &es, &map, &bedrock);
        assert_eq!(r.bedrock.as_deref(), Some("es_ES"));
        assert_eq!(
            r.lines.get("tile.stone.name").map(String::as_str),
            Some("Piedra")
        );
        assert_eq!((r.mapped, r.unmapped), (1, 2));
        assert!(lang_file(&r.lines).contains("tile.stone.name=Piedra\n"));
    }
}
