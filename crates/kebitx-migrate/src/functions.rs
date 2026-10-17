//! Java functions (`.mcfunction`) → a line-by-line task list.
//!
//! Commands are written differently in the two editions, and no file of
//! either game pairs a Java command line with its Bedrock form: translating
//! them would be guessing. What can be known is whether each command EXISTS
//! in Bedrock — the game's `en_US.lang` names every command it has
//! (`commands.<name>.description`) — and whether a line uses what Bedrock's
//! commands have no form for (NBT `{...}`, item components `[...]` after an
//! id, macros `$`). Each line is marked; nothing is converted.

use std::collections::HashSet;

/// The commands the Bedrock game names.
pub fn bedrock_commands(en: &std::collections::HashMap<String, String>) -> HashSet<String> {
    en.keys()
        .filter_map(|k| k.strip_prefix("commands.")?.strip_suffix(".description"))
        .filter(|c| !c.contains('.'))
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub number: usize,
    pub text: String,
    /// Commands on the line (the first, and each one after `run`).
    pub commands: Vec<String>,
    /// Those Bedrock does not name.
    pub unknown: Vec<String>,
    /// What Bedrock's commands have no form for.
    pub notes: Vec<&'static str>,
}

/// Every command line of a function.
pub fn analyze(text: &str, known: &HashSet<String>) -> Vec<Line> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut notes = Vec::new();
        let body = match line.strip_prefix('$') {
            Some(rest) => {
                notes.push("a macro line ($): Bedrock functions have no macros");
                rest
            }
            None => line,
        };
        let words: Vec<&str> = body.split_whitespace().collect();
        let mut commands = Vec::new();
        if let Some(first) = words.first() {
            commands.push(first.trim_start_matches('/').to_string());
        }
        for (k, w) in words.iter().enumerate() {
            if *w == "run" {
                if let Some(next) = words.get(k + 1) {
                    commands.push(next.trim_start_matches('/').to_string());
                }
            }
        }
        if body.contains('{') {
            notes.push("NBT or a JSON component: Bedrock commands take neither");
        }
        if body
            .split_whitespace()
            .any(|w| w.contains(":") && w.contains('[') && !w.starts_with('@'))
        {
            notes.push("an item or block with [components/states] in Java's form");
        }
        let unknown = commands
            .iter()
            .filter(|c| !known.contains(*c))
            .cloned()
            .collect();
        out.push(Line {
            number: i + 1,
            text: line.to_string(),
            commands,
            unknown,
            notes,
        });
    }
    out
}

/// The task list for one function.
pub fn task_file(file: &str, lines: &[Line]) -> String {
    let mut s = format!(
        "{file}: line by line, for a person to rewrite in Bedrock's command syntax\n\
         (OK = Bedrock has the command; its arguments may still differ)\n\n"
    );
    for l in lines {
        let mark = if l.unknown.is_empty() && l.notes.is_empty() {
            "OK  "
        } else {
            "TODO"
        };
        s.push_str(&format!("{mark} {:4}  {}\n", l.number, l.text));
        for u in &l.unknown {
            s.push_str(&format!("            `{u}`: no such command in Bedrock\n"));
        }
        for n in &l.notes {
            s.push_str(&format!("            {n}\n"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_checked_against_bedrocks_list() {
        let mut en = std::collections::HashMap::new();
        for c in ["say", "execute", "tp", "give"] {
            en.insert(format!("commands.{c}.description"), "x".to_string());
        }
        en.insert("commands.tp.success".into(), "x".into());
        let known = bedrock_commands(&en);
        assert_eq!(known.len(), 4);
        let lines = analyze(
            "# a comment\nsay hi\nexecute as @a at @s run tp ~ ~1 ~\ndata merge entity @s {Invisible:1b}\n$say $(x)\ngive @s minecraft:stick[minecraft:custom_name='x']\n",
            &known,
        );
        assert_eq!(lines.len(), 5);
        assert!(lines[0].unknown.is_empty() && lines[0].notes.is_empty());
        assert_eq!(lines[1].commands, ["execute", "tp"]);
        assert_eq!(lines[2].unknown, ["data"]);
        assert!(!lines[2].notes.is_empty(), "NBT");
        assert!(lines[3].notes[0].contains("macro"));
        assert!(!lines[4].notes.is_empty(), "item components");
        assert!(task_file("f", &lines).contains("TODO    4  data merge"));
    }
}
