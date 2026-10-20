//! What a Mixin class needs of the class it targets, read from its
//! annotations — each one fails in its own way when the game changes:
//!
//!   - `@Shadow` and `@Overwrite` members must exist in the target: Mixin
//!     refuses the class when the game starts ("Shadow method ... was not
//!     located in the target class");
//!   - `@Accessor("x")` and `@Invoker("y")` name a field or method of it;
//!   - an injector (`@Inject`, `@Redirect`, `@ModifyArg`, MixinExtras'
//!     `@WrapOperation`, ...) names the method it goes into, and its `@At`
//!     the call or field it hooks. A target that is gone matches nothing:
//!     with `require` of 1 or more (on the annotation, else the config's
//!     `injectors.defaultRequire`) the game does not start; with 0 the
//!     injection silently does nothing.
//!
//! Only what can be read exactly is checked: a selector with a wildcard or a
//! quantifier, an accessor whose name is inferred, a constructor invoker,
//! are left alone.

use crate::class::{Annotation, ClassFile, Member, MemberRef, RefKind, Value};
use crate::source::Source;
use std::collections::{BTreeSet, HashMap};

const SHADOW: &str = "Lorg/spongepowered/asm/mixin/Shadow;";
const OVERWRITE: &str = "Lorg/spongepowered/asm/mixin/Overwrite;";
const ACCESSOR: &str = "Lorg/spongepowered/asm/mixin/gen/Accessor;";
const INVOKER: &str = "Lorg/spongepowered/asm/mixin/gen/Invoker;";
const AT: &str = "Lorg/spongepowered/asm/mixin/injection/At;";

/// A method selector of an injector: `tick`, `tick()V`, `Lowner;tick()V`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    pub name: String,
    pub descriptor: Option<String>,
}

/// `None` for what cannot be matched exactly: wildcards (`*`, `tick*`),
/// quantifiers (`{2}`), regex selectors (`/.../`), target selectors (`@`).
pub fn selector(s: &str) -> Option<Selector> {
    let s = s.trim();
    if s.is_empty() || s.starts_with(['/', '@']) || s.contains(['*', '{']) {
        return None;
    }
    // an owner prefix: `Lnet/minecraft/X;tick()V`
    let s = match (s.starts_with('L'), s.find(';')) {
        (true, Some(i)) if !s[..i].contains('(') => &s[i + 1..],
        _ => s,
    };
    let (name, descriptor) = match s.find(['(', ':']) {
        Some(i) => {
            let d = s[i..].trim_start_matches(':');
            (&s[..i], (!d.is_empty()).then(|| d.to_string()))
        }
        None => (s, None),
    };
    (!name.is_empty() && !name.contains(['/', ';', '.'])).then(|| Selector {
        name: name.to_string(),
        descriptor,
    })
}

/// An `@At(target = ...)`: `Lowner;name(desc)ret` (a method) or
/// `Lowner;name:Ldesc;` (a field).
pub fn at_target(s: &str) -> Option<MemberRef> {
    let s = s.trim();
    if s.contains(['*', '{']) {
        return None;
    }
    let rest = s.strip_prefix('L')?;
    let (owner, member) = rest.split_once(';')?;
    if let Some(i) = member.find('(') {
        let (name, descriptor) = member.split_at(i);
        return (!name.is_empty()).then(|| MemberRef {
            kind: RefKind::Method,
            owner: owner.to_string(),
            name: name.to_string(),
            descriptor: descriptor.to_string(),
        });
    }
    let (name, descriptor) = member.split_once(':')?;
    (!name.is_empty() && !descriptor.is_empty()).then(|| MemberRef {
        kind: RefKind::Field,
        owner: owner.to_string(),
        name: name.to_string(),
        descriptor: descriptor.to_string(),
    })
}

/// Mixin class (internal name) -> its config's `injectors.defaultRequire`,
/// from the Mixin configs at the root of the mod (`*.mixins.json`: a
/// `package` and lists `mixins`, `client`, `server`).
pub fn default_requires(src: &Source, files: &BTreeSet<String>) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for f in files
        .iter()
        .filter(|f| !f.contains('/') && f.ends_with(".json"))
    {
        let Some(j) = src
            .read(f)
            .and_then(|b| kebitx_core::json::parse(&String::from_utf8_lossy(&b)).ok())
        else {
            continue;
        };
        let Some(package) = j.get("package").and_then(kebitx_core::json::Json::as_str) else {
            continue;
        };
        let require = match j.get("injectors").and_then(|i| i.get("defaultRequire")) {
            Some(kebitx_core::json::Json::Num(n)) => *n as i64,
            _ => 0,
        };
        for list in ["mixins", "client", "server"] {
            for m in j
                .get(list)
                .and_then(kebitx_core::json::Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(kebitx_core::json::Json::as_str)
            {
                out.insert(format!("{package}.{m}").replace('.', "/"), require);
            }
        }
    }
    out
}

/// One thing a Mixin class needs of its target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    /// A member that must be there (`@Shadow`, `@Overwrite`, `@Accessor`,
    /// `@Invoker`): any of `names` (aliases) with this descriptor.
    Member {
        how: &'static str,
        handler: String,
        names: Vec<String>,
        field: bool,
        descriptor: String,
    },
    /// An injector: the methods it goes into, and the calls or fields its
    /// `@At`s hook.
    Injection {
        handler: String,
        kind: String,
        methods: Vec<Selector>,
        at: Vec<MemberRef>,
        require: Option<i64>,
    },
}

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Str(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| match v {
                Value::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Every `@At` inside an annotation (`at`, `slice = @Slice(from = @At ..)`).
fn ats(a: &Annotation, out: &mut Vec<MemberRef>) {
    fn walk(v: &Value, out: &mut Vec<MemberRef>) {
        match v {
            Value::Annotation(a) => ats(a, out),
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    if a.descriptor == AT {
        if let Some(Value::Str(t)) = a.get("target") {
            out.extend(at_target(t));
        }
    }
    for (_, v) in &a.elements {
        walk(v, out);
    }
}

fn is_injector(descriptor: &str) -> bool {
    descriptor.starts_with("Lorg/spongepowered/asm/mixin/injection/")
        || descriptor.starts_with("Lcom/llamalad7/mixinextras/injector/")
}

/// The simple name of an annotation type: `Lorg/.../Inject;` -> `@Inject`.
fn simple(descriptor: &str) -> String {
    let n = descriptor
        .trim_end_matches(';')
        .rsplit('/')
        .next()
        .unwrap_or("");
    format!("@{n}")
}

fn needs_of(m: &Member, field: bool, out: &mut Vec<Need>) {
    for a in &m.annotations {
        let member = |how: &'static str, names: Vec<String>, descriptor: String| Need::Member {
            how,
            handler: m.name.clone(),
            names,
            field,
            descriptor,
        };
        match a.descriptor.as_str() {
            SHADOW => {
                let prefix = match a.get("prefix") {
                    Some(Value::Str(p)) => p.clone(),
                    _ => "shadow$".into(),
                };
                let mut names = vec![m.name.strip_prefix(&prefix).unwrap_or(&m.name).to_string()];
                names.extend(strings(a.get("aliases")));
                out.push(member("@Shadow", names, m.descriptor.clone()));
            }
            OVERWRITE if !field => {
                let mut names = vec![m.name.clone()];
                names.extend(strings(a.get("aliases")));
                out.push(member("@Overwrite", names, m.descriptor.clone()));
            }
            ACCESSOR if !field => {
                // the field's type: a getter returns it, a setter takes it
                let Some(Value::Str(name)) = a.get("value") else {
                    continue;
                };
                let d = &m.descriptor;
                let ty = if let Some(t) = d.strip_prefix("()") {
                    t.to_string()
                } else if let Some(t) = d.strip_prefix('(').and_then(|r| r.strip_suffix(")V")) {
                    t.to_string()
                } else {
                    continue;
                };
                out.push(Need::Member {
                    how: "@Accessor",
                    handler: m.name.clone(),
                    names: vec![name.clone()],
                    field: true,
                    descriptor: ty,
                });
            }
            INVOKER if !field => {
                let Some(Value::Str(name)) = a.get("value") else {
                    continue;
                };
                if name.starts_with('<') {
                    continue;
                }
                out.push(member("@Invoker", vec![name.clone()], m.descriptor.clone()));
            }
            d if !field && is_injector(d) && a.get("method").is_some() => {
                let mut at = Vec::new();
                ats(a, &mut at);
                out.push(Need::Injection {
                    handler: m.name.clone(),
                    kind: simple(d),
                    methods: strings(a.get("method"))
                        .iter()
                        .filter_map(|s| selector(s))
                        .collect(),
                    at,
                    require: match a.get("require") {
                        Some(Value::Int(n)) => Some(*n),
                        _ => None,
                    },
                });
            }
            _ => {}
        }
    }
}

/// Everything a Mixin class needs of its target.
pub fn needs(c: &ClassFile) -> Vec<Need> {
    let mut out = Vec::new();
    for m in &c.fields {
        needs_of(m, true, &mut out);
    }
    for m in &c.methods {
        needs_of(m, false, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_are_read_exactly_or_not_at_all() {
        let s = |x| selector(x);
        assert_eq!(
            s("tick"),
            Some(Selector {
                name: "tick".into(),
                descriptor: None
            })
        );
        assert_eq!(
            s("render(Lnet/minecraft/client/Camera;F)V"),
            Some(Selector {
                name: "render".into(),
                descriptor: Some("(Lnet/minecraft/client/Camera;F)V".into())
            })
        );
        assert_eq!(
            s("Lnet/minecraft/world/entity/Mob;tick()V"),
            Some(Selector {
                name: "tick".into(),
                descriptor: Some("()V".into())
            })
        );
        assert_eq!(s("<init>").map(|s| s.name), Some("<init>".into()));
        for unreadable in ["*", "tick*", "tick{2}", "/tick.*/", "@Desc", ""] {
            assert_eq!(s(unreadable), None, "{unreadable}");
        }
    }

    #[test]
    fn at_targets_are_methods_or_fields() {
        assert_eq!(
            at_target("Lnet/minecraft/world/level/Level;isClientSide()Z"),
            Some(MemberRef {
                kind: RefKind::Method,
                owner: "net/minecraft/world/level/Level".into(),
                name: "isClientSide".into(),
                descriptor: "()Z".into(),
            })
        );
        assert_eq!(
            at_target("Lnet/minecraft/core/BlockPos;ZERO:Lnet/minecraft/core/BlockPos;")
                .map(|r| (r.kind, r.name)),
            Some((RefKind::Field, "ZERO".into()))
        );
        assert_eq!(at_target("Lnet/minecraft/X;*"), None);
        assert_eq!(at_target("tick"), None);
    }
}
