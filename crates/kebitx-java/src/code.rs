//! A mod's code against the game: every class, field and method of the game
//! it names, and every Mixin target, must exist in the version it runs on.
//! "This mod calls a method that does not exist in 26.3" is a crash waiting
//! for the line to run (`NoSuchMethodError`), or, for a Mixin target, a crash
//! at start-up.
//!
//! Since 26.x the game's `.jar` is not obfuscated, so the names a mod is
//! compiled against are the names in the `.jar`. A mod compiled for an
//! obfuscated version names Fabric's intermediary classes
//! (`net/minecraft/class_1234`): those cannot be compared, and the report
//! says so instead of reporting thousands of differences.
//!
//! What is proven:
//!   - a class is missing only under one of the `.jar`'s own package roots
//!     (`net/minecraft`, `com/mojang/blaze3d`, ...): the game's libraries
//!     (`com/mojang/serialization`, ...) are separate `.jar`s;
//!   - a member is looked up as the JVM does — the class, then its
//!     superclasses and interfaces. It is missing only when every class on
//!     the way is known: the game's own, or `java/lang/Object`, whose
//!     methods never change. A chain through any other JDK or library class
//!     is unchecked, never reported;
//!   - a constructor (`<init>`) belongs to its own class only.

use crate::class::{self, ClassFile, MemberRef, RefKind, Value};
use crate::game::JavaJar;
use crate::source::Source;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// What the game declares of one class.
#[derive(Debug, Default, Clone)]
pub struct GameClass {
    pub super_name: Option<String>,
    pub interfaces: Vec<String>,
    pub fields: HashSet<(String, String)>,
    pub methods: HashSet<(String, String)>,
}

/// Every class of one game version.
#[derive(Debug, Default)]
pub struct GameIndex {
    pub classes: HashMap<String, GameClass>,
    /// Package roots of the `.jar`'s classes (`net/minecraft/`, ...).
    pub roots: BTreeSet<String>,
    /// Class files of the `.jar` that could not be read.
    pub unreadable: usize,
}

/// `net/minecraft/` for the game's own package, else the first three
/// segments (`com/mojang/blaze3d/`).
fn root_of(class: &str) -> Option<String> {
    let parts: Vec<&str> = class.split('/').collect();
    let n = if parts.first() == Some(&"net") && parts.get(1) == Some(&"minecraft") {
        2
    } else {
        3
    };
    (parts.len() > n).then(|| format!("{}/", parts[..n].join("/")))
}

/// The methods of `java/lang/Object` (JLS 4.3.2; unchanged since Java 1.0
/// apart from `finalize`, still present).
fn object_method(name: &str, descriptor: &str) -> bool {
    matches!(
        (name, descriptor),
        ("getClass", "()Ljava/lang/Class;")
            | ("hashCode", "()I")
            | ("equals", "(Ljava/lang/Object;)Z")
            | ("clone", "()Ljava/lang/Object;")
            | ("toString", "()Ljava/lang/String;")
            | ("notify", "()V")
            | ("notifyAll", "()V")
            | ("wait", "()V")
            | ("wait", "(J)V")
            | ("wait", "(JI)V")
            | ("finalize", "()V")
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    Yes,
    No,
    /// The lookup went through a class nothing here describes.
    Unknown,
}

impl GameIndex {
    /// Every class of a game version's `.jar`, read on every core.
    pub fn from_jar(jar: &JavaJar) -> Self {
        let names: Vec<&String> = jar.zip.names().filter(|n| n.ends_with(".class")).collect();
        let parsed = kebitx_core::par::par_map(&names, |n| {
            jar.zip
                .read(n, 16 * 1024 * 1024)
                .ok()
                .and_then(|b| class::parse(&b).ok())
        });
        let mut index = Self::default();
        for c in parsed {
            match c {
                Some(c) => index.add(&c),
                None => index.unreadable += 1,
            }
        }
        index
    }

    pub fn add(&mut self, c: &ClassFile) {
        if let Some(r) = root_of(&c.name) {
            self.roots.insert(r);
        }
        let pairs = |m: &[class::Member]| {
            m.iter()
                .map(|m| (m.name.clone(), m.descriptor.clone()))
                .collect()
        };
        self.classes.insert(
            c.name.clone(),
            GameClass {
                super_name: c.super_name.clone(),
                interfaces: c.interfaces.clone(),
                fields: pairs(&c.fields),
                methods: pairs(&c.methods),
            },
        );
    }

    /// Is this class in one of the game's own packages?
    pub fn is_game(&self, class: &str) -> bool {
        root_of(class).is_some_and(|r| self.roots.contains(&r))
    }

    /// Does the member exist in the game alone, looked up as the JVM does?
    pub fn resolve(&self, r: &MemberRef) -> Found {
        Env {
            game: self,
            extra: HashMap::new(),
            added: HashMap::new(),
        }
        .resolve(r)
    }
}

/// The game as a mod sees it at run time: the game's classes, the classes the
/// mod and the jars it bundles bring, and what their Mixins add to the game's
/// classes (a Mixin's interfaces and its own members are merged into its
/// target: that is how Fabric API gives `BlockStateModel` its `emitQuads`).
struct Env<'a> {
    game: &'a GameIndex,
    /// the mod's and its bundled jars' own classes
    extra: HashMap<String, GameClass>,
    /// game class -> what Mixins add to it
    added: HashMap<String, GameClass>,
}

impl Env<'_> {
    fn declared(&self, c: &str) -> Option<&GameClass> {
        self.game.classes.get(c).or_else(|| self.extra.get(c))
    }

    fn resolve(&self, r: &MemberRef) -> Found {
        let key = (r.name.clone(), r.descriptor.clone());
        let field = r.kind == RefKind::Field;
        if r.name == "<init>" {
            // constructors are not inherited, and Mixins cannot add them
            return match self.declared(&r.owner) {
                Some(c) if c.methods.contains(&key) => Found::Yes,
                Some(_) => Found::No,
                None => Found::Unknown,
            };
        }
        let mut stack = vec![r.owner.as_str()];
        let mut seen = HashSet::new();
        let mut unknown = false;
        while let Some(c) = stack.pop() {
            if !seen.insert(c) {
                continue;
            }
            if c == "java/lang/Object" {
                if !field && object_method(&r.name, &r.descriptor) {
                    return Found::Yes;
                }
                continue;
            }
            let base = self.declared(c);
            let add = self.added.get(c);
            if base.is_none() {
                unknown = true;
            }
            for gc in base.into_iter().chain(add) {
                if (if field { &gc.fields } else { &gc.methods }).contains(&key) {
                    return Found::Yes;
                }
                stack.extend(gc.super_name.as_deref());
                stack.extend(gc.interfaces.iter().map(String::as_str));
            }
        }
        if unknown {
            Found::Unknown
        } else {
            Found::No
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CodeFinding {
    /// A class of the game's own packages that this version does not have.
    MissingClass {
        class: String,
        used_by: String,
        uses: usize,
    },
    /// A field or method the game class (and its ancestors) does not have.
    MissingMember {
        owner: String,
        name: String,
        descriptor: String,
        field: bool,
        used_by: String,
        uses: usize,
    },
    /// A Mixin applied to a class this version does not have.
    MissingMixinTarget { mixin: String, target: String },
    /// A class file of the mod that could not be read.
    Unreadable { file: String, why: String },
    /// Information: not in the game's `.jar`, but something the mod runs with
    /// may add it — the loader's own patches (Forge, NeoForge), or a mod it
    /// depends on and does not bundle. Not claimed either way.
    NotInGame {
        what: String,
        used_by: String,
        may_come_from: String,
    },
    /// A member a Mixin needs of its target (`@Shadow`, `@Overwrite`,
    /// `@Accessor`, `@Invoker`) that is not there: Mixin refuses the class
    /// when the game starts.
    MixinMember {
        mixin: String,
        target: String,
        how: String,
        member: String,
    },
    /// An injector whose method or `@At` matches nothing in this version.
    /// `required` (a `require` of 1 or more): the game does not start;
    /// otherwise the injection silently does nothing (information).
    MixinInjection {
        mixin: String,
        target: String,
        handler: String,
        missing: String,
        required: bool,
    },
}

impl CodeFinding {
    pub fn is_info(&self) -> bool {
        matches!(
            self,
            Self::NotInGame { .. }
                | Self::MixinInjection {
                    required: false,
                    ..
                }
        )
    }
}

/// What one Mixin class needs of one of its targets (a game class that
/// exists), and is not there.
fn mixin_needs(
    c: &ClassFile,
    target: &str,
    env: &Env,
    requires: &HashMap<String, i64>,
    report: &mut CodeReport,
) -> Vec<CodeFinding> {
    use crate::mixin::Need;
    let mixin = c.name.replace('/', ".");
    let dotted = target.replace('/', ".");
    let mut out = Vec::new();
    // the methods an injector may go into: the target's own, and those
    // other Mixins add to it (not inherited ones)
    let mut methods: Vec<&(String, String)> = Vec::new();
    for gc in env
        .game
        .classes
        .get(target)
        .into_iter()
        .chain(env.added.get(target))
    {
        methods.extend(gc.methods.iter());
    }
    for need in crate::mixin::needs(c) {
        match need {
            Need::Member {
                how,
                handler: _,
                names,
                field,
                descriptor,
            } => {
                let found: Vec<Found> = names
                    .iter()
                    .map(|n| {
                        env.resolve(&MemberRef {
                            kind: if field {
                                RefKind::Field
                            } else {
                                RefKind::Method
                            },
                            owner: target.to_string(),
                            name: n.clone(),
                            descriptor: descriptor.clone(),
                        })
                    })
                    .collect();
                if found.contains(&Found::Yes) {
                    report.checked += 1;
                } else if found.contains(&Found::Unknown) {
                    report.unchecked += 1;
                } else {
                    out.push(CodeFinding::MixinMember {
                        mixin: mixin.clone(),
                        target: dotted.clone(),
                        how: how.to_string(),
                        member: format!(
                            "{} {} {descriptor}",
                            if field { "field" } else { "method" },
                            names.join(" | ")
                        ),
                    });
                }
            }
            Need::Injection {
                handler,
                kind,
                methods: selectors,
                at,
                require,
            } => {
                let required = require
                    .or_else(|| requires.get(&c.name).copied())
                    .unwrap_or(0)
                    > 0;
                let mut missing = |what: String| {
                    out.push(CodeFinding::MixinInjection {
                        mixin: mixin.clone(),
                        target: dotted.clone(),
                        handler: format!("{kind} {handler}"),
                        missing: what,
                        required,
                    })
                };
                for s in selectors {
                    let hit = methods.iter().any(|(n, d)| {
                        *n == s.name && s.descriptor.as_ref().is_none_or(|sd| sd == d)
                    });
                    if hit {
                        report.checked += 1;
                    } else {
                        missing(format!(
                            "method {}{} in {dotted}",
                            s.name,
                            s.descriptor.as_deref().unwrap_or("")
                        ));
                    }
                }
                for r in at {
                    if !env.game.is_game(&r.owner) {
                        continue;
                    }
                    let what = format!(
                        "@At {} {}.{} {}",
                        if r.kind == RefKind::Field {
                            "field"
                        } else {
                            "method"
                        },
                        r.owner.replace('/', "."),
                        r.name,
                        r.descriptor
                    );
                    if !env.game.classes.contains_key(&r.owner) {
                        missing(what);
                        continue;
                    }
                    match env.resolve(&r) {
                        Found::Yes => report.checked += 1,
                        Found::Unknown => report.unchecked += 1,
                        Found::No => missing(what),
                    }
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeReport {
    pub classes: usize,
    pub mixins: usize,
    /// Jars bundled inside the mod (`META-INF/jars/`), read for what they
    /// add to the game, and their classes.
    pub nested_jars: usize,
    pub bundled_classes: usize,
    /// Dependencies neither the game, the loader nor the bundled jars
    /// provide: what they add to the game cannot be seen from here.
    pub external: Vec<String>,
    /// The mod names intermediary classes (`net/minecraft/class_1234`): it
    /// was compiled for an obfuscated version and is not compared.
    pub intermediary: bool,
    pub checked: usize,
    pub unchecked: usize,
    pub findings: Vec<CodeFinding>,
}

impl CodeReport {
    pub fn broken(&self) -> usize {
        self.findings.iter().filter(|f| !f.is_info()).count()
    }
}

/// `net/minecraft/class_1234`, Fabric's intermediary naming.
fn is_intermediary(class: &str) -> bool {
    class
        .rsplit('/')
        .next()
        .and_then(|n| n.strip_prefix("class_"))
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
        && class.starts_with("net/minecraft/")
}

/// The classes a `@Mixin` applies to: `value` class literals and `targets`
/// names (`net.minecraft.X$Inner`).
pub fn mixin_targets(c: &ClassFile) -> Option<Vec<String>> {
    let m = c
        .annotations
        .iter()
        .find(|a| a.descriptor == "Lorg/spongepowered/asm/mixin/Mixin;")?;
    let mut out = Vec::new();
    let mut take = |v: &Value| match v {
        Value::Class(d) => {
            if let Some(n) = d.strip_prefix('L').and_then(|d| d.strip_suffix(';')) {
                out.push(n.to_string());
            }
        }
        Value::Str(s) => out.push(s.replace('.', "/")),
        _ => {}
    };
    for key in ["value", "targets"] {
        match m.get(key) {
            Some(Value::Array(a)) => a.iter().for_each(&mut take),
            Some(v) => take(v),
            None => {}
        }
    }
    Some(out)
}

/// Dependencies that add nothing to the game's classes: the game, Java, the
/// Fabric and Quilt loaders (which do not patch the game), and MixinExtras
/// (a library of Mixin injectors).
const ADDS_NOTHING: [&str; 6] = [
    "minecraft",
    "java",
    "fabricloader",
    "fabric-loader",
    "quilt_loader",
    "mixinextras",
];

/// Bundled jars nest (a jar in a jar in a jar): this deep is plenty.
const MAX_NESTING: usize = 3;

/// Every class of the jars bundled in `src`, and the mod ids they provide.
fn bundled(src: &Source, files: &BTreeSet<String>, depth: usize, out: &mut Bundled) {
    if depth >= MAX_NESTING {
        return;
    }
    for jar in files
        .iter()
        .filter(|f| f.starts_with("META-INF/jars/") && f.ends_with(".jar"))
    {
        let Some(inner) = src
            .read(jar)
            .and_then(|b| kebitx_core::codec::zip::ZipArchive::from_bytes(b).ok())
            .map(|z| Source::Zip(Box::new(z)))
        else {
            continue;
        };
        out.jars += 1;
        let inner_files = inner.files();
        if let Some(meta) = inner
            .read("fabric.mod.json")
            .and_then(|b| kebitx_core::json::parse(&String::from_utf8_lossy(&b)).ok())
        {
            let id = meta.get("id").and_then(kebitx_core::json::Json::as_str);
            let provides = meta
                .get("provides")
                .and_then(kebitx_core::json::Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(kebitx_core::json::Json::as_str);
            out.ids
                .extend(id.into_iter().chain(provides).map(str::to_string));
        }
        for f in inner_files
            .iter()
            .filter(|f| f.ends_with(".class") && !f.starts_with("META-INF/"))
        {
            if let Some(c) = inner.read(f).and_then(|b| class::parse(&b).ok()) {
                out.classes.push(c);
            }
        }
        bundled(&inner, &inner_files, depth + 1, out);
    }
}

#[derive(Default)]
struct Bundled {
    jars: usize,
    ids: BTreeSet<String>,
    classes: Vec<ClassFile>,
}

/// What the other mods of a set bring when they load together: their
/// classes (bundled jars included) and the mod ids they provide. Empty for a
/// mod analysed alone.
#[derive(Default)]
pub struct Context {
    pub classes: Vec<ClassFile>,
    pub ids: BTreeSet<String>,
}

impl Context {
    /// Adds one mod: its classes, its bundled jars, and `ids` (its own id
    /// and what it provides).
    pub fn add_mod(&mut self, src: &Source, files: &BTreeSet<String>, ids: &[String]) {
        for f in files
            .iter()
            .filter(|f| f.ends_with(".class") && !f.starts_with("META-INF/"))
        {
            if let Some(c) = src.read(f).and_then(|b| class::parse(&b).ok()) {
                self.classes.push(c);
            }
        }
        let mut b = Bundled::default();
        bundled(src, files, 0, &mut b);
        self.classes.extend(b.classes);
        self.ids.extend(b.ids);
        self.ids.extend(ids.iter().cloned());
    }
}

/// What a Mixin class merges into its targets.
fn merge_mixin(c: &ClassFile, targets: &[String], added: &mut HashMap<String, GameClass>) {
    let shadow = |m: &class::Member| {
        m.annotations
            .iter()
            .any(|a| a.descriptor == "Lorg/spongepowered/asm/mixin/Shadow;")
    };
    for t in targets {
        let a = added.entry(t.clone()).or_default();
        a.interfaces.extend(c.interfaces.iter().cloned());
        a.methods.extend(
            c.methods
                .iter()
                .filter(|m| !shadow(m) && !m.name.starts_with('<'))
                .map(|m| (m.name.clone(), m.descriptor.clone())),
        );
        a.fields.extend(
            c.fields
                .iter()
                .filter(|m| !shadow(m))
                .map(|m| (m.name.clone(), m.descriptor.clone())),
        );
    }
}

/// The mod's classes against the game. `None` when the source has no code.
/// `loader` and `depends` come from the mod's metadata.
pub fn check(
    src: &Source,
    files: &BTreeSet<String>,
    game: &GameIndex,
    loader: Option<&str>,
    depends: &[String],
    ctx: &Context,
) -> Option<CodeReport> {
    // multi-release copies (`META-INF/versions/<n>/`) repeat classes for
    // other JDKs: the base ones are what the game loads
    let class_files: Vec<&String> = files
        .iter()
        .filter(|f| f.ends_with(".class") && !f.starts_with("META-INF/"))
        .collect();
    if class_files.is_empty() {
        return None;
    }
    let mut findings = BTreeSet::new();
    let mut own = Vec::new();
    for f in &class_files {
        match src
            .read(f)
            .ok_or_else(|| "cannot be read".to_string())
            .and_then(|b| class::parse(&b))
        {
            Ok(c) => own.push(c),
            Err(why) => {
                findings.insert(CodeFinding::Unreadable {
                    file: (*f).clone(),
                    why,
                });
            }
        }
    }
    let requires = crate::mixin::default_requires(src, files);
    let mut b = Bundled::default();
    bundled(src, files, 0, &mut b);

    let patches_game = matches!(loader, Some("Forge" | "NeoForge"));
    let external: Vec<String> = depends
        .iter()
        .filter(|d| {
            !ADDS_NOTHING.contains(&d.as_str()) && !b.ids.contains(*d) && !ctx.ids.contains(*d)
        })
        .filter(|d| !(patches_game && matches!(d.as_str(), "forge" | "neoforge")))
        .cloned()
        .collect();
    let mut report = CodeReport {
        classes: class_files.len(),
        nested_jars: b.jars,
        bundled_classes: b.classes.len(),
        external: external.clone(),
        ..Default::default()
    };

    if own
        .iter()
        .any(|c| c.classes.iter().any(|n| is_intermediary(n)))
    {
        report.intermediary = true;
        report.findings = findings.into_iter().collect();
        return Some(report);
    }

    let mut env = Env {
        game,
        extra: HashMap::new(),
        added: HashMap::new(),
    };
    for c in own.iter().chain(&b.classes).chain(&ctx.classes) {
        let pairs = |m: &[class::Member]| {
            m.iter()
                .map(|m| (m.name.clone(), m.descriptor.clone()))
                .collect()
        };
        env.extra.insert(
            c.name.clone(),
            GameClass {
                super_name: c.super_name.clone(),
                interfaces: c.interfaces.clone(),
                fields: pairs(&c.fields),
                methods: pairs(&c.methods),
            },
        );
        if let Some(t) = mixin_targets(c) {
            merge_mixin(c, &t, &mut env.added);
        }
    }

    // what is not in the game's .jar is claimed only when nothing the mod
    // runs with can add it
    let may_come_from = if patches_game {
        Some(format!(
            "{} patches the game",
            loader.unwrap_or("the loader")
        ))
    } else if !external.is_empty() {
        Some(external.join(", "))
    } else {
        None
    };

    // aggregated by what is missing: (first user, number of users)
    let mut missing_classes: BTreeMap<String, (String, usize)> = BTreeMap::new();
    let mut missing_members: BTreeMap<MemberRef, (String, usize)> = BTreeMap::new();
    // member lookups are the cost: every class on every core, results in order
    let resolved: Vec<Vec<(MemberRef, Found)>> = kebitx_core::par::par_map(&own, |c| {
        c.refs
            .iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            // a missing owner is already a missing class
            .filter(|r| {
                !r.owner.starts_with('[')
                    && game.is_game(&r.owner)
                    && game.classes.contains_key(&r.owner)
            })
            .map(|r| (r.clone(), env.resolve(r)))
            .collect()
    });
    for (c, refs) in own.iter().zip(&resolved) {
        let user = c.name.replace('/', ".");
        for name in c.classes.iter().collect::<BTreeSet<_>>() {
            if !game.is_game(name) {
                continue;
            }
            if game.classes.contains_key(name.as_str()) {
                report.checked += 1;
            } else {
                missing_classes
                    .entry(name.clone())
                    .or_insert_with(|| (user.clone(), 0))
                    .1 += 1;
            }
        }
        for (r, found) in refs {
            match found {
                Found::Yes => report.checked += 1,
                Found::Unknown => report.unchecked += 1,
                Found::No => {
                    missing_members
                        .entry(r.clone())
                        .or_insert_with(|| (user.clone(), 0))
                        .1 += 1
                }
            }
        }
        if let Some(targets) = mixin_targets(c) {
            report.mixins += 1;
            for t in targets {
                if !game.is_game(&t) {
                    continue;
                }
                if game.classes.contains_key(&t) {
                    report.checked += 1;
                    for f in mixin_needs(c, &t, &env, &requires, &mut report) {
                        findings.insert(match &may_come_from {
                            Some(from) => CodeFinding::NotInGame {
                                what: match &f {
                                    CodeFinding::MixinMember { how, member, .. } => {
                                        format!("{how} {member}")
                                    }
                                    CodeFinding::MixinInjection { missing, .. } => missing.clone(),
                                    other => format!("{other:?}"),
                                },
                                used_by: user.clone(),
                                may_come_from: from.clone(),
                            },
                            None => f,
                        });
                    }
                } else if let Some(from) = &may_come_from {
                    findings.insert(CodeFinding::NotInGame {
                        what: format!("Mixin target {}", t.replace('/', ".")),
                        used_by: user.clone(),
                        may_come_from: from.clone(),
                    });
                } else {
                    findings.insert(CodeFinding::MissingMixinTarget {
                        mixin: user.clone(),
                        target: t.replace('/', "."),
                    });
                }
            }
        }
    }
    for (class, (used_by, uses)) in missing_classes {
        let class = class.replace('/', ".");
        findings.insert(match &may_come_from {
            Some(from) => CodeFinding::NotInGame {
                what: format!("class {class}"),
                used_by,
                may_come_from: from.clone(),
            },
            None => CodeFinding::MissingClass {
                class,
                used_by,
                uses,
            },
        });
    }
    for (r, (used_by, uses)) in missing_members {
        let field = r.kind == RefKind::Field;
        let owner = r.owner.replace('/', ".");
        findings.insert(match &may_come_from {
            Some(from) => CodeFinding::NotInGame {
                what: format!(
                    "{} {owner}.{} {}",
                    if field { "field" } else { "method" },
                    r.name,
                    r.descriptor
                ),
                used_by,
                may_come_from: from.clone(),
            },
            None => CodeFinding::MissingMember {
                owner,
                name: r.name,
                descriptor: r.descriptor,
                field,
                used_by,
                uses,
            },
        });
    }
    report.findings = findings.into_iter().collect();
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class::tests::Builder;

    fn game() -> GameIndex {
        let mut g = GameIndex::default();
        let mut entity = Builder::default();
        entity.methods.push(("tick".into(), "()V".into()));
        entity
            .fields
            .push(("level".into(), "Lnet/minecraft/world/level/Level;".into()));
        g.add(
            &class::parse(&entity.build("net/minecraft/world/entity/Entity", "java/lang/Object"))
                .unwrap(),
        );
        let mut mob = Builder::default();
        mob.methods.push(("<init>".into(), "()V".into()));
        g.add(
            &class::parse(&mob.build(
                "net/minecraft/world/entity/Mob",
                "net/minecraft/world/entity/Entity",
            ))
            .unwrap(),
        );
        let list = Builder::default();
        g.add(
            &class::parse(&list.build("net/minecraft/util/Things", "java/util/AbstractList"))
                .unwrap(),
        );
        g
    }

    fn r(kind: RefKind, owner: &str, name: &str, desc: &str) -> MemberRef {
        MemberRef {
            kind,
            owner: owner.into(),
            name: name.into(),
            descriptor: desc.into(),
        }
    }

    #[test]
    fn members_are_found_through_ancestors_as_the_jvm_finds_them() {
        let g = game();
        let mob = "net/minecraft/world/entity/Mob";
        // inherited from Entity, and from Object
        assert_eq!(
            g.resolve(&r(RefKind::Method, mob, "tick", "()V")),
            Found::Yes
        );
        assert_eq!(
            g.resolve(&r(
                RefKind::Field,
                mob,
                "level",
                "Lnet/minecraft/world/level/Level;"
            )),
            Found::Yes
        );
        assert_eq!(
            g.resolve(&r(RefKind::Method, mob, "hashCode", "()I")),
            Found::Yes
        );
        // gone: the whole chain is known
        assert_eq!(
            g.resolve(&r(RefKind::Method, mob, "tick", "(Z)V")),
            Found::No
        );
        assert_eq!(
            g.resolve(&r(RefKind::Field, mob, "hashCode", "I")),
            Found::No
        );
        // constructors are not inherited
        assert_eq!(
            g.resolve(&r(RefKind::Method, mob, "<init>", "()V")),
            Found::Yes
        );
        assert_eq!(
            g.resolve(&r(
                RefKind::Method,
                "net/minecraft/world/entity/Entity",
                "<init>",
                "()V"
            )),
            Found::No
        );
        // through a JDK class nothing here describes: never claimed
        assert_eq!(
            g.resolve(&r(
                RefKind::Method,
                "net/minecraft/util/Things",
                "size",
                "()I"
            )),
            Found::Unknown
        );
    }

    #[test]
    fn a_mods_code_is_checked_and_aggregated() {
        let g = game();
        let mut m = Builder::default();
        m.member_ref(10, "net/minecraft/world/entity/Mob", "tick", "()V");
        m.member_ref(10, "net/minecraft/world/entity/Mob", "oldTick", "()V");
        m.member_ref(10, "net/minecraft/world/entity/Gone", "x", "()V");
        m.member_ref(10, "com/mojang/serialization/Codec", "unit", "()V");
        m.mixin_targets(&[
            "net.minecraft.world.entity.Mob",
            "net.minecraft.client.Removed",
        ]);
        let dir = std::env::temp_dir().join(format!("kebitx-code-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("demo")).unwrap();
        std::fs::write(
            dir.join("demo/MobMixin.class"),
            m.build("demo/MobMixin", "java/lang/Object"),
        )
        .unwrap();
        std::fs::write(dir.join("demo/Broken.class"), b"\xCA\xFE\xBA\xBE\x00").unwrap();
        let src = Source::open(&dir).unwrap();
        let rep = check(
            &src,
            &src.files(),
            &g,
            Some("Fabric"),
            &[],
            &Context::default(),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!((rep.classes, rep.mixins, rep.intermediary), (2, 1, false));
        assert_eq!(
            rep.findings,
            vec![
                CodeFinding::MissingClass {
                    class: "net.minecraft.world.entity.Gone".into(),
                    used_by: "demo.MobMixin".into(),
                    uses: 1,
                },
                CodeFinding::MissingMember {
                    owner: "net.minecraft.world.entity.Mob".into(),
                    name: "oldTick".into(),
                    descriptor: "()V".into(),
                    field: false,
                    used_by: "demo.MobMixin".into(),
                    uses: 1,
                },
                CodeFinding::MissingMixinTarget {
                    mixin: "demo.MobMixin".into(),
                    target: "net.minecraft.client.Removed".into(),
                },
                CodeFinding::Unreadable {
                    file: "demo/Broken.class".into(),
                    why: "truncated class file".into(),
                },
            ]
        );
    }

    #[test]
    fn intermediary_names_are_not_compared() {
        assert!(is_intermediary("net/minecraft/class_1297"));
        assert!(!is_intermediary("net/minecraft/world/entity/Entity"));
        assert!(!is_intermediary("demo/class_1"));
    }

    /// What a Mixin needs of `Mob`: shadows (one inherited, one gone, one
    /// with the `shadow$` prefix), injections into a method that is there,
    /// one that is gone (required, and not), and an `@At` whose call is gone.
    #[test]
    fn mixin_shadows_injections_and_at_targets() {
        use crate::class::tests::Elem;
        let g = game();
        let mut m = Builder::default();
        m.fields
            .push(("level".into(), "Lnet/minecraft/world/level/Level;".into()));
        m.fields.push(("gone".into(), "I".into()));
        for name in ["shadow$tick", "onInit", "onOld", "onMaybe", "atGone"] {
            m.methods.push((name.into(), "()V".into()));
        }
        let shadow = m.annotation("Lorg/spongepowered/asm/mixin/Shadow;", vec![]);
        m.annotate(false, 0, shadow.clone());
        m.annotate(false, 1, shadow.clone());
        m.annotate(true, 0, shadow);
        let inject = "Lorg/spongepowered/asm/mixin/injection/Inject;";
        let at_ok = m.annotation(
            "Lorg/spongepowered/asm/mixin/injection/At;",
            vec![
                ("value", Elem::Str("INVOKE")),
                (
                    "target",
                    Elem::Str("Lnet/minecraft/world/entity/Entity;tick()V"),
                ),
            ],
        );
        let a = m.annotation(
            inject,
            vec![
                ("method", Elem::Strs(vec!["<init>"])),
                ("at", Elem::Ann(at_ok)),
            ],
        );
        m.annotate(true, 1, a);
        let a = m.annotation(
            inject,
            vec![
                ("method", Elem::Strs(vec!["oldTick()V"])),
                ("require", Elem::Int(1)),
            ],
        );
        m.annotate(true, 2, a);
        let a = m.annotation(inject, vec![("method", Elem::Str("maybe"))]);
        m.annotate(true, 3, a);
        let at_gone = m.annotation(
            "Lorg/spongepowered/asm/mixin/injection/At;",
            vec![(
                "target",
                Elem::Str("Lnet/minecraft/world/entity/Mob;vanished()V"),
            )],
        );
        let a = m.annotation(
            "Lorg/spongepowered/asm/mixin/injection/Redirect;",
            vec![
                ("method", Elem::Str("<init>")),
                ("at", Elem::Ann(at_gone)),
                ("require", Elem::Int(1)),
            ],
        );
        m.annotate(true, 4, a);
        m.mixin_targets(&["net.minecraft.world.entity.Mob"]);

        let dir = std::env::temp_dir().join(format!("kebitx-needs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("demo")).unwrap();
        std::fs::write(
            dir.join("demo/MobMixin.class"),
            m.build("demo/MobMixin", "java/lang/Object"),
        )
        .unwrap();
        let src = Source::open(&dir).unwrap();
        let rep = check(
            &src,
            &src.files(),
            &g,
            Some("Fabric"),
            &[],
            &Context::default(),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        let missing: std::collections::BTreeSet<(bool, String)> = rep
            .findings
            .iter()
            .map(|f| match f {
                CodeFinding::MixinMember { how, member, .. } => (true, format!("{how} {member}")),
                CodeFinding::MixinInjection {
                    missing, required, ..
                } => (*required, missing.clone()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            missing,
            std::collections::BTreeSet::from([
                (true, "@Shadow field gone I".to_string()),
                (
                    true,
                    "@At method net.minecraft.world.entity.Mob.vanished ()V".to_string()
                ),
                (
                    true,
                    "method oldTick()V in net.minecraft.world.entity.Mob".to_string()
                ),
                (
                    false,
                    "method maybe in net.minecraft.world.entity.Mob".to_string()
                ),
            ]),
            "{:#?}",
            rep.findings
        );
        assert_eq!(rep.broken(), 3);
    }

    /// A mod whose Mixin gives `Mob` an interface with a default method
    /// (Fabric API's way), and that calls it: found. A call to something no
    /// one adds: missing — unless the mod runs with something that may add it.
    #[test]
    fn what_mixins_add_is_there_and_what_others_may_add_is_not_claimed() {
        let g = game();
        let dir = std::env::temp_dir().join(format!("kebitx-mixin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("demo")).unwrap();
        let mut api = Builder::default();
        api.methods.push(("extra".into(), "()V".into()));
        std::fs::write(
            dir.join("demo/Api.class"),
            api.build("demo/Api", "java/lang/Object"),
        )
        .unwrap();
        let mut mixin = Builder::default();
        mixin.interfaces.push("demo/Api".into());
        mixin.methods.push(("helper".into(), "()I".into()));
        mixin.mixin_targets(&["net.minecraft.world.entity.Mob"]);
        std::fs::write(
            dir.join("demo/MobMixin.class"),
            mixin.build("demo/MobMixin", "java/lang/Object"),
        )
        .unwrap();
        let mut user = Builder::default();
        user.member_ref(10, "net/minecraft/world/entity/Mob", "extra", "()V");
        user.member_ref(10, "net/minecraft/world/entity/Mob", "helper", "()I");
        user.member_ref(10, "net/minecraft/world/entity/Mob", "ghost", "()V");
        std::fs::write(
            dir.join("demo/User.class"),
            user.build("demo/User", "java/lang/Object"),
        )
        .unwrap();
        let src = Source::open(&dir).unwrap();
        let files = src.files();

        let fabric = check(
            &src,
            &files,
            &g,
            Some("Fabric"),
            &["fabricloader".into()],
            &Context::default(),
        )
        .unwrap();
        assert_eq!(fabric.mixins, 1);
        assert_eq!(fabric.broken(), 1, "{:#?}", fabric.findings);
        assert!(matches!(&fabric.findings[0],
            CodeFinding::MissingMember { name, .. } if name == "ghost"));

        // a dependency it does not bundle may add `ghost`: not claimed
        let other = check(
            &src,
            &files,
            &g,
            Some("Fabric"),
            &["othermod".into()],
            &Context::default(),
        )
        .unwrap();
        assert_eq!(other.broken(), 0);
        assert_eq!(other.external, ["othermod"]);
        assert!(matches!(&other.findings[0],
            CodeFinding::NotInGame { may_come_from, .. } if may_come_from == "othermod"));

        // NeoForge patches the game's classes: nothing missing is claimed
        let neo = check(
            &src,
            &files,
            &g,
            Some("NeoForge"),
            &["neoforge".into()],
            &Context::default(),
        )
        .unwrap();
        assert_eq!(neo.broken(), 0);
        assert!(neo.external.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
