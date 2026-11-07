//! A script's static imports. No parser: a walk over the text that knows where
//! comments, strings and regular expressions are, so what they contain is never
//! taken for code. What it cannot follow it does not report: a `from` it misses
//! is a finding not made, and never a false one.

/// The modules a script imports with `import` or `export ... from`, at its top level.
pub fn imports(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut depth) = (0usize, 0usize);
    // the last thing seen that was not space: it tells a division from a regex
    let mut last = b';';
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'"' | b'\'' | b'`' => {
                i = past_string(b, i);
                last = b'"';
                continue;
            }
            b'/' if b"(,=:[!&|?{};+-*%<>~^".contains(&last) => {
                i = past_regex(b, i);
                last = b'"';
                continue;
            }
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if is_word(c) {
            let start = i;
            while i < b.len() && is_word(b[i]) {
                i += 1;
            }
            let word = &text[start..i];
            if depth == 0 && (word == "import" || word == "export") {
                if let Some((module, end)) = statement_module(text, i) {
                    out.push(module);
                    i = end;
                }
            }
            last = b'a';
            continue;
        }
        last = c;
        i += 1;
    }
    out
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

fn past_string(b: &[u8], start: usize) -> usize {
    let quote = b[start];
    let mut i = start + 1;
    while i < b.len() && b[i] != quote {
        if b[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    i + 1
}

fn past_regex(b: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    let mut class = false;
    while i < b.len() && b[i] != b'\n' {
        match b[i] {
            b'\\' => i += 1,
            b'[' => class = true,
            b']' => class = false,
            b'/' if !class => return i + 1,
            _ => {}
        }
        i += 1;
    }
    i
}

/// After `import` or `export`: the module named, and where the statement ends.
fn statement_module(text: &str, from: usize) -> Option<(String, usize)> {
    let b = text.as_bytes();
    let mut i = from;
    let mut braces = 0usize;
    let mut first = true;
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let c = *b.get(i)?;
        match c {
            b'"' | b'\'' => {
                // `import "x"`, or the string after `from` (see below)
                if !first {
                    return None;
                }
                let end = past_string(b, i);
                return Some((text.get(i + 1..end - 1)?.to_string(), end));
            }
            b'{' => {
                braces += 1;
                i += 1;
            }
            b'}' => {
                braces = braces.checked_sub(1)?;
                i += 1;
            }
            b'*' | b',' => i += 1,
            // `import(...)` is dynamic, and `import.meta` is no import
            b'(' | b'.' | b';' | b'=' if braces == 0 => return None,
            _ if is_word(c) => {
                let start = i;
                while i < b.len() && is_word(b[i]) {
                    i += 1;
                }
                let word = &text[start..i];
                if braces == 0 {
                    if word == "from" {
                        while i < b.len() && b[i].is_ascii_whitespace() {
                            i += 1;
                        }
                        let q = *b.get(i)?;
                        if q != b'"' && q != b'\'' {
                            // a name that happens to be `from`
                            continue;
                        }
                        let end = past_string(b, i);
                        return Some((text.get(i + 1..end - 1)?.to_string(), end));
                    }
                    if matches!(
                        word,
                        "const" | "let" | "var" | "function" | "class" | "default" | "async"
                    ) {
                        return None;
                    }
                }
            }
            _ => i += 1,
        }
        first = false;
    }
}

/// `./x.js` from a script in `dir`, as a path in the pack. `None` when it leaves
/// the pack: that is nothing to look up.
pub fn join(dir: &str, import: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in import.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_form_of_import() {
        let text = r#"
            import { world, system as s } from "@minecraft/server";
            import * as ui from '@minecraft/server-ui';
            import d from "./default.js"
            import d2, { x } from "./mixed.js";
            import "./side_effect.js";
            export { y } from "./re_export.js";
            export * from "../up.js";
            import{a}from"./tight.js";
        "#;
        assert_eq!(
            imports(text),
            [
                "@minecraft/server",
                "@minecraft/server-ui",
                "./default.js",
                "./mixed.js",
                "./side_effect.js",
                "./re_export.js",
                "../up.js",
                "./tight.js",
            ]
        );
    }

    #[test]
    fn what_only_looks_like_an_import() {
        let text = r#"
            // import a from "./comment.js";
            /* import b from "./block.js"; */
            const s = 'import c from "./string.js"';
            const t = `import d from "./template.js"`;
            const r = /import e from "\.\/regex.js"/;
            const q = 10 / 2; const w = 4 / 2;
            function f() { import("./dynamic.js"); }
            const meta = import.meta;
            export const k = 1;
            export default function g() {}
            export function h() { return "./not.js"; }
            if (s) { const from = "./inner.js"; }
        "#;
        assert_eq!(imports(text), Vec::<String>::new());
    }

    #[test]
    fn broken_text_never_panics() {
        for text in [
            "import",
            "import {",
            "import { a } from",
            "import \"",
            "export *",
            "/*",
            "\"",
            "/",
            "import ñ from \"./ñ.js\"",
        ] {
            let _ = imports(text);
        }
    }

    #[test]
    fn paths() {
        assert_eq!(
            join("scripts/a", "./b.js").as_deref(),
            Some("scripts/a/b.js")
        );
        assert_eq!(
            join("scripts/a", "../c.js").as_deref(),
            Some("scripts/c.js")
        );
        assert_eq!(join("scripts", "../../d.js"), None);
    }
}
