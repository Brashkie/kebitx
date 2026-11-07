//! The words people read, in their language.
//!
//! `lang/en.txt` is the list: one text per line, as the program says it in English,
//! with `{0}`, `{1}` where values go. Every other file has its translation on the
//! same line. A language is added with one more file and one more row in
//! `LANGUAGES`, and tests refuse a file with a line missing or a value lost.
//!
//! English is what nothing is translated into: `tr` gives back its argument, so the
//! CLI, which never chooses a language, says what it always said.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

const ENGLISH: &str = include_str!("../lang/en.txt");

/// Code, the language's name for itself, and its file.
pub const LANGUAGES: [(&str, &str, &str); 7] = [
    ("en", "English", ENGLISH),
    ("es", "Español", include_str!("../lang/es.txt")),
    ("pt", "Português (Brasil)", include_str!("../lang/pt.txt")),
    ("fr", "Français", include_str!("../lang/fr.txt")),
    ("de", "Deutsch", include_str!("../lang/de.txt")),
    ("ru", "Русский", include_str!("../lang/ru.txt")),
    ("zh", "简体中文", include_str!("../lang/zh.txt")),
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn tables() -> &'static Vec<HashMap<&'static str, &'static str>> {
    static TABLES: OnceLock<Vec<HashMap<&'static str, &'static str>>> = OnceLock::new();
    TABLES.get_or_init(|| {
        LANGUAGES
            .iter()
            .map(|(_, _, file)| ENGLISH.lines().zip(file.lines()).collect())
            .collect()
    })
}

/// Chooses the language by its code: `es`, or as systems write it, `pt-BR`, `de_AT`.
/// False, with nothing changed, when there is no such language.
pub fn set_language(code: &str) -> bool {
    let wanted = code
        .split(['-', '_', '.'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match LANGUAGES.iter().position(|(c, _, _)| *c == wanted) {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// The code of the language in use.
pub fn language() -> &'static str {
    LANGUAGES[CURRENT.load(Ordering::Relaxed)].0
}

/// A text in the language in use; itself, when the list does not have it.
pub fn tr(text: &str) -> &str {
    let i = CURRENT.load(Ordering::Relaxed);
    if i == 0 {
        return text;
    }
    tables()[i].get(text).copied().unwrap_or(text)
}

/// A text with its values in place of `{0}`, `{1}`, ...
pub fn tf(template: &str, values: &[&dyn Display]) -> String {
    let text = tr(template);
    let mut out = String::with_capacity(text.len() + values.len() * 16);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let index: usize = after[..close].parse().ok()?;
            Some((values.get(index)?, close))
        });
        match value {
            Some((v, close)) => {
                out.push_str(&v.to_string());
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn values_in(line: &str) -> BTreeSet<&str> {
        line.split('{')
            .skip(1)
            .filter_map(|p| p.split_once('}').map(|(n, _)| n))
            .filter(|n| n.bytes().all(|b| b.is_ascii_digit()) && !n.is_empty())
            .collect()
    }

    /// A line lost shifts every line after it, and values show where.
    #[test]
    fn every_language_has_every_line_with_the_same_values() {
        let english: Vec<&str> = ENGLISH.lines().collect();
        let unique: BTreeSet<&&str> = english.iter().collect();
        assert_eq!(unique.len(), english.len(), "a text listed twice");
        for (code, _, file) in LANGUAGES {
            let lines: Vec<&str> = file.lines().collect();
            assert_eq!(lines.len(), english.len(), "{code}: number of lines");
            for (n, (en, there)) in english.iter().zip(&lines).enumerate() {
                assert!(!there.trim().is_empty(), "{code}:{}: empty", n + 1);
                assert_eq!(
                    values_in(en),
                    values_in(there),
                    "{code}:{}: {en} / {there}",
                    n + 1
                );
            }
        }
    }

    /// Every text the code asks for by name is in the list, or it stays in English
    /// with nobody noticing.
    #[test]
    fn every_text_the_code_says_is_in_the_list() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let listed: BTreeSet<&str> = ENGLISH.lines().collect();
        let sources = [
            ("crates/kebitx-core/src/explain.rs", "tf(\""),
            ("crates/kebitx-core/src/explain.rs", "tr(\""),
            ("crates/kebitx-java/src/describe.rs", "tf(\""),
            ("crates/kebitx-java/src/describe.rs", "tr(\""),
            ("crates/kebitx-ffi/src/lib.rs", "tf(\""),
            ("crates/kebitx-ffi/src/lib.rs", "tr(\""),
            ("crates/kebitx-ffi/src/repair.rs", "tf(\""),
            ("crates/kebitx-ffi/src/repair.rs", "tr(\""),
            ("crates/kebitx-ffi/src/world.rs", "tf(\""),
            ("crates/kebitx-ffi/src/world.rs", "tr(\""),
            ("crates/kebitx-ffi/src/package.rs", "tf(\""),
            ("crates/kebitx-ffi/src/package.rs", "tr(\""),
            ("apps/desktop/src/win32.c", "T(\""),
        ];
        let mut seen = 0;
        for (file, marker) in sources {
            let text = std::fs::read_to_string(root.join(file)).expect(file);
            for piece in text.split(marker).skip(1) {
                // up to the closing quote; an escaped one does not end it
                let mut end = 0;
                let bytes = piece.as_bytes();
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                let said = piece[..end.min(piece.len())].replace("\\\"", "\"");
                assert!(listed.contains(said.as_str()), "{file}: {said}");
                seen += 1;
            }
        }
        assert!(
            seen > 100,
            "only {seen}: did the code change its way of asking?"
        );
    }

    #[test]
    fn values_go_in_place() {
        // English, whatever another test chose: this reads the list directly
        assert_eq!(tf("{1} and {0}", &[&"a", &2]), "2 and a");
        assert_eq!(tf("no values", &[]), "no values");
        // what is not a value stays
        assert_eq!(tf("{x} {0} {9}", &[&1]), "{x} 1 {9}");
    }

    #[test]
    fn codes_as_systems_write_them() {
        assert!(!set_language("xx"));
        assert_eq!(language(), "en");
        assert!(tables()[1].contains_key("Home"));
        assert_eq!(tables()[1].get("Home"), Some(&"Inicio"));
        for code in ["es", "ES", "es-MX", "es_ES.UTF-8"] {
            let wanted = code
                .split(['-', '_', '.'])
                .next()
                .unwrap()
                .to_ascii_lowercase();
            assert_eq!(wanted, "es");
        }
    }
}
