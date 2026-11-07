//! Findings as the lines people read: the same words in the CLI and the apps.

use crate::analyze::Finding;
use crate::code::CodeFinding;
use crate::modset::SetFinding;
use kebitx_core::i18n::tf;

/// One finding of a Java pack or mod, as a report line.
pub fn finding_line(f: &Finding) -> String {
    match f {
        Finding::MissingModel { file, model } => tf("{0}: model {1} does not exist", &[file, model]),
        Finding::MissingTexture { file, texture } => {
            tf("{0}: texture {1} does not exist", &[file, texture])
        }
        Finding::UnboundTexture {
            model,
            variable,
            used_by,
        } => tf(
            "{0}: a face asks for #{1}, which its chain never sets (the missing texture shows; used by {2})",
            &[model, variable, used_by],
        ),
        Finding::BadAnimation {
            texture,
            size,
            frame,
        } => tf(
            "{0}: {1}x{2} does not divide into {3}x{4} frames (the game refuses it)",
            &[texture, &size.0, &size.1, &frame.0, &frame.1],
        ),
        Finding::Unreadable { file, why } => tf("{0}: unreadable ({1})", &[file, why]),
        Finding::NotPng { texture } => {
            tf("{0}: not a PNG (the game checks the signature and refuses it)", &[texture])
        }
        Finding::Proportions {
            texture,
            size,
            game,
        } => tf(
            "{0}: {1}x{2}, the game's is {3}x{4} (other proportions)",
            &[texture, &size.0, &size.1, &game.0, &game.1],
        ),
        Finding::MissingLangKeys { file, missing, of } => {
            tf("{0}: {1} of {2} English keys missing (English shows)", &[file, missing, of])
        }
    }
}

/// One finding about a mod's code, as a report line.
pub fn code_line(f: &CodeFinding) -> String {
    match f {
        CodeFinding::MissingClass {
            class,
            used_by,
            uses,
        } => tf(
            "class {0} does not exist (used by {1}, {2} class(es))",
            &[class, used_by, uses],
        ),
        CodeFinding::MissingMember {
            owner,
            name,
            descriptor,
            field,
            used_by,
            uses,
        } => tf(
            if *field {
                "field {0}.{1} {2} does not exist (used by {3}, {4} class(es))"
            } else {
                "method {0}.{1} {2} does not exist (used by {3}, {4} class(es))"
            },
            &[owner, name, descriptor, used_by, uses],
        ),
        CodeFinding::MissingMixinTarget { mixin, target } => tf(
            "mixin {0} targets {1}, which does not exist",
            &[mixin, target],
        ),
        CodeFinding::Unreadable { file, why } => tf("{0}: unreadable ({1})", &[file, why]),
        CodeFinding::MixinMember {
            mixin,
            target,
            how,
            member,
        } => tf(
            "mixin {0}: {1} {2} is not in {3} (Mixin refuses the class at start-up)",
            &[mixin, how, member, target],
        ),
        CodeFinding::MixinInjection {
            mixin,
            handler,
            missing,
            required,
            ..
        } => tf(
            if *required {
                "mixin {0}: {1} finds no {2} (required: the game does not start)"
            } else {
                "mixin {0}: {1} finds no {2} (not required: it silently does nothing)"
            },
            &[mixin, handler, missing],
        ),
        CodeFinding::NotInGame {
            what,
            used_by,
            may_come_from,
        } => tf(
            "{0} is not in the game's .jar (used by {1}; may come from: {2})",
            &[what, used_by, may_come_from],
        ),
    }
}

/// One finding about a set of mods, as a report line.
pub fn set_line(f: &SetFinding) -> String {
    match f {
        SetFinding::Duplicate { id, jars } => tf(
            "mod {0} is in {1} jars: {2} (the loader refuses to start)",
            &[id, &jars.len(), &jars.join(", ")],
        ),
        SetFinding::Missing {
            jar,
            id,
            dependency,
            wanted,
        } => tf(
            "{0} ({1}) needs {2} {3}, which no mod here provides",
            &[id, jar, dependency, wanted],
        ),
        SetFinding::WrongVersion {
            jar,
            id,
            dependency,
            wanted,
            found,
        } => tf(
            "{0} ({1}) needs {2} {3}; here it is {4}",
            &[id, jar, dependency, wanted, found],
        ),
        SetFinding::Breaks {
            jar,
            id,
            other,
            rule,
            found,
        } => tf(
            "{0} ({1}) declares it breaks {2} {3}; here: {4}",
            &[id, jar, other, rule, found],
        ),
        SetFinding::OverwriteConflict {
            target,
            method,
            mods,
        } => tf(
            "{0} all @Overwrite {1}.{2}: only one change survives",
            &[&mods.join(", "), target, method],
        ),
        SetFinding::MixedLoaders { loaders } => tf(
            "mods for {0} in one folder: each loader loads only its own",
            &[&loaders.join(" and ")],
        ),
        SetFinding::NotAMod { jar } => tf("{0}: no mod metadata (a library, or not a mod)", &[jar]),
    }
}
