//! What a command names: functions to run, items to give, entities to summon.
//! Anything not recognised names nothing: an unknown command is never a finding.

#[derive(Debug, PartialEq, Eq)]
pub enum Ref {
    Function(String),
    Item(String),
    Entity(String),
}

/// Words of a command. A selector's `[...]` and a quoted text are one word, spaces
/// and all.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut depth) = (false, 0usize);
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if quoted => {
                word.push(c);
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            '"' => {
                quoted = !quoted;
                word.push(c);
            }
            '[' if !quoted => {
                depth += 1;
                word.push(c);
            }
            ']' if !quoted => {
                depth = depth.saturating_sub(1);
                word.push(c);
            }
            c if c.is_whitespace() && !quoted && depth == 0 => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// What one line refers to.
pub fn references(line: &str) -> Vec<Ref> {
    let line = line.trim();
    let line = line.strip_prefix('/').unwrap_or(line);
    if line.is_empty() || line.starts_with('#') {
        return Vec::new();
    }
    let words = words(line);
    let unquote = |w: &String| w.trim_matches('"').to_string();
    let mut i = 0;
    // `execute`, in both of its grammars, ends in the command it runs
    while words.get(i).map(String::as_str) == Some("execute") {
        if let Some(run) = words[i..].iter().position(|w| w == "run") {
            i += run + 1;
        } else if words.get(i + 5).map(String::as_str) == Some("detect") {
            i += 11; // execute <who> <x y z> detect <x y z> <block> <data> <command>
        } else {
            i += 5; // execute <who> <x y z> <command>
        }
    }
    let name = words.get(i).map(|w| w.trim_start_matches('/'));
    match name {
        Some("function") => words
            .get(i + 1)
            .map(|w| vec![Ref::Function(unquote(w))])
            .unwrap_or_default(),
        Some("give") => words
            .get(i + 2)
            .map(|w| vec![Ref::Item(unquote(w))])
            .unwrap_or_default(),
        Some("summon") => words
            .get(i + 1)
            .map(|w| vec![Ref::Entity(unquote(w))])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(s: &str) -> Vec<Ref> {
        vec![Ref::Function(s.to_string())]
    }

    #[test]
    fn functions_every_way_they_are_written() {
        assert_eq!(references("function a/b"), f("a/b"));
        assert_eq!(references("  /function a/b  "), f("a/b"));
        assert_eq!(references(r#"function "a b/c""#), f("a b/c"));
        assert_eq!(
            references("execute as @a[tag=x, name=\"a b\"] at @s run function z"),
            f("z")
        );
        assert_eq!(references("execute @a ~ ~1 ~ function z"), f("z"));
        assert_eq!(
            references("execute @a ~ ~ ~ detect ~ ~-1 ~ stone 0 function z"),
            f("z")
        );
        assert_eq!(
            references("execute if entity @a run execute as @s run function z"),
            f("z")
        );
    }

    #[test]
    fn items_and_entities() {
        assert_eq!(
            references("give @s[name=\"a b\"] my:thing 1"),
            vec![Ref::Item("my:thing".into())]
        );
        assert_eq!(
            references("summon my:mob ~ ~ ~"),
            vec![Ref::Entity("my:mob".into())]
        );
    }

    #[test]
    fn nothing_else_names_anything() {
        for line in [
            "",
            "# function a",
            "say function a",
            "tellraw @a {\"rawtext\":[{\"text\":\"function a\"}]}",
            "function",
            "give @s",
            "execute",
            "execute as @a",
            "scoreboard players add @a function 1",
        ] {
            assert_eq!(references(line), Vec::new(), "{line}");
        }
    }
}
