//! Java data-pack loot tables → Bedrock behavior-pack loot tables.
//!
//! As for recipes, a table is compared by what it MEANS: its pools (rolls,
//! conditions) and their entries (what drops, weight, how many, looting
//! bonus, smelting, conditions). Both games ship their own tables, and a
//! test holds the converter to them. Anything the canonical form does not
//! know is kept as `Other`, so it can only match itself — and a table with
//! an `Other` is not converted: it is reported for a person.

use kebitx_core::json::{Json, JsonMap};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Count {
    Exact(f64),
    Range(f64, f64),
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Cond {
    KilledByPlayer,
    Chance(f64),
    /// chance, extra chance per looting level
    ChanceWithLooting(f64, f64),
    /// the entity was on fire (Java also counts a weapon that smelts loot,
    /// which in Bedrock sets the entity on fire: the same)
    OnFire,
    /// the block survived the explosion that broke it (Java only)
    SurvivesExplosion,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Func {
    SetCount(Count),
    Looting(Count),
    Smelt(Vec<Cond>),
    ExplosionDecay,
    SetDamage(Count),
    EnchantRandomly,
    EnchantWithLevels(Count, bool),
    SetPotion(String),
    Other(String),
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum What {
    Item(String),
    Empty,
    Table(String),
    Other(String),
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct LootEntry {
    pub what: What,
    pub weight: f64,
    pub functions: Vec<Func>,
    pub conditions: Vec<Cond>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Pool {
    pub rolls: Count,
    pub entries: Vec<LootEntry>,
    pub conditions: Vec<Cond>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub pools: Vec<Pool>,
}

impl Table {
    /// Anything not understood anywhere in the table.
    pub fn others(&self) -> Vec<String> {
        let mut out = Vec::new();
        let conds = |c: &[Cond], out: &mut Vec<String>| {
            for x in c {
                if let Cond::Other(s) = x {
                    out.push(s.clone());
                }
            }
        };
        for p in &self.pools {
            conds(&p.conditions, &mut out);
            for e in &p.entries {
                conds(&e.conditions, &mut out);
                if let What::Other(s) = &e.what {
                    out.push(s.clone());
                }
                for f in &e.functions {
                    match f {
                        Func::Other(s) => out.push(s.clone()),
                        Func::Smelt(c) => conds(c, &mut out),
                        _ => {}
                    }
                }
            }
        }
        out
    }

    /// Every item name.
    pub fn items(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for p in &self.pools {
            for e in &p.entries {
                if let What::Item(i) = &e.what {
                    out.insert(i.clone());
                }
            }
        }
        out
    }

    pub fn renamed(&self, f: &dyn Fn(&str) -> String) -> Table {
        let mut t = self.clone();
        for p in &mut t.pools {
            for e in &mut p.entries {
                if let What::Item(i) = &mut e.what {
                    *i = f(i);
                }
            }
        }
        t
    }
}

fn num(v: &Json) -> Option<f64> {
    match v {
        Json::Num(n) => Some(*n),
        _ => None,
    }
}

fn count(v: &Json) -> Option<Count> {
    match v {
        Json::Num(n) => Some(Count::Exact(*n)),
        Json::Obj(_) => {
            let kind = v
                .get("type")
                .and_then(Json::as_str)
                .map(|t| t.trim_start_matches("minecraft:"));
            match kind {
                None | Some("uniform") => {
                    let (lo, hi) = (num(v.get("min")?)?, num(v.get("max")?)?);
                    Some(if lo == hi {
                        Count::Exact(lo)
                    } else {
                        Count::Range(lo, hi)
                    })
                }
                Some("constant") => Some(Count::Exact(num(v.get("value")?)?)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn key_name(v: &Json) -> String {
    v.get("type")
        .or_else(|| v.get("function"))
        .or_else(|| v.get("condition"))
        .and_then(Json::as_str)
        .unwrap_or("")
        .trim_start_matches("minecraft:")
        .to_string()
}

/// A stable text for something not understood: compared only to itself.
fn other(v: &Json) -> String {
    kebitx_core::json::to_string(v)
}

fn java_cond(v: &Json) -> Cond {
    match key_name(v).as_str() {
        "killed_by_player" => Cond::KilledByPlayer,
        "random_chance" => match v.get("chance").and_then(num) {
            Some(c) => Cond::Chance(c),
            None => Cond::Other(other(v)),
        },
        "random_chance_with_enchanted_bonus" => {
            let base = v.get("unenchanted_chance").and_then(num);
            let per = v.get("enchanted_chance").and_then(|e| match e {
                Json::Obj(_)
                    if e.get("type")
                        .and_then(Json::as_str)
                        .map(|t| t.trim_start_matches("minecraft:"))
                        == Some("linear") =>
                {
                    let (b, p) = (
                        e.get("base").and_then(num)?,
                        e.get("per_level_above_first").and_then(num)?,
                    );
                    // Bedrock's form: chance + level * multiplier
                    (base.is_some_and(|base| (b - p - base).abs() < 1e-9) || base.is_none())
                        .then_some(p)
                }
                _ => None,
            });
            let looting = v
                .get("enchantment")
                .and_then(Json::as_str)
                .map(|e| e.trim_start_matches("minecraft:"))
                == Some("looting");
            match (base, per, looting) {
                (Some(b), Some(p), true) => Cond::ChanceWithLooting(b, p),
                _ => Cond::Other(other(v)),
            }
        }
        "entity_properties" if is_on_fire(v) => Cond::OnFire,
        "survives_explosion" => Cond::SurvivesExplosion,
        "any_of" => {
            // Java's smelting test: on fire, or killed by a weapon that
            // smelts loot (fire aspect, which in Bedrock sets it on fire)
            let terms = v
                .get("terms")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default();
            if terms.iter().any(is_on_fire)
                && terms.iter().all(|t| is_on_fire(t) || smelts_loot_weapon(t))
            {
                Cond::OnFire
            } else {
                Cond::Other(other(v))
            }
        }
        _ => Cond::Other(other(v)),
    }
}

fn is_on_fire(v: &Json) -> bool {
    key_name(v) == "entity_properties"
        && v.get("entity").and_then(Json::as_str) == Some("this")
        && v.get("predicate")
            .and_then(|p| p.get("minecraft:flags").or_else(|| p.get("flags")))
            .and_then(|f| f.get("is_on_fire"))
            == Some(&Json::Bool(true))
}

fn smelts_loot_weapon(v: &Json) -> bool {
    key_name(v) == "entity_properties" && other(v).contains("#minecraft:smelts_loot")
}

fn conds_of(v: &Json, java: bool) -> Vec<Cond> {
    let list: Vec<Json> = match (v.get("conditions"), v.get("condition")) {
        (Some(Json::Arr(a)), _) => a.clone(),
        (_, Some(c @ Json::Obj(_))) if java => vec![c.clone()],
        _ => Vec::new(),
    };
    let mut out: Vec<Cond> = list
        .iter()
        .map(|c| if java { java_cond(c) } else { bedrock_cond(c) })
        .collect();
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn java_func(v: &Json) -> Func {
    match key_name(v).as_str() {
        "set_count"
            if !matches!(v.get("add"), Some(Json::Bool(true)))
                && v.get("condition").is_none()
                && v.get("conditions").is_none() =>
        {
            v.get("count")
                .and_then(count)
                .map(Func::SetCount)
                .unwrap_or_else(|| Func::Other(other(v)))
        }
        "enchanted_count_increase"
            if v.get("enchantment")
                .and_then(Json::as_str)
                .map(|e| e.trim_start_matches("minecraft:"))
                == Some("looting")
                && v.get("limit").is_none() =>
        {
            v.get("count")
                .and_then(count)
                .map(Func::Looting)
                .unwrap_or_else(|| Func::Other(other(v)))
        }
        "furnace_smelt" => Func::Smelt(conds_of(v, true)),
        "explosion_decay" => Func::ExplosionDecay,
        "set_damage" if !matches!(v.get("add"), Some(Json::Bool(true))) => v
            .get("damage")
            .and_then(count)
            .map(Func::SetDamage)
            .unwrap_or_else(|| Func::Other(other(v))),
        // the default pool of random enchantments: Bedrock's plain form
        "enchant_randomly"
            if matches!(
                v.get("options").and_then(Json::as_str),
                None | Some("#minecraft:on_random_loot")
            ) =>
        {
            Func::EnchantRandomly
        }
        "enchant_with_levels" => match (
            v.get("levels").and_then(count),
            v.get("options").and_then(Json::as_str),
        ) {
            (Some(l), None | Some("#minecraft:on_random_loot")) => {
                Func::EnchantWithLevels(l, false)
            }
            (Some(l), Some("#minecraft:in_enchanting_table")) => Func::EnchantWithLevels(l, false),
            _ => Func::Other(other(v)),
        },
        "set_potion" => match v.get("id").and_then(Json::as_str) {
            Some(id) => Func::SetPotion(id.trim_start_matches("minecraft:").to_string()),
            None => Func::Other(other(v)),
        },
        _ => Func::Other(other(v)),
    }
}

fn entry_of(e: &Json, java: bool) -> LootEntry {
    let kind = key_name(e);
    let name = e
        .get("name")
        .or_else(|| e.get("value"))
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_string();
    let what = match kind.as_str() {
        "item" => What::Item(crate::recipes::full(&name)),
        "empty" => What::Empty,
        "loot_table" => What::Table(
            name.trim_start_matches("loot_tables/")
                .trim_start_matches("minecraft:")
                .trim_end_matches(".json")
                .to_string(),
        ),
        _ => What::Other(other(e)),
    };
    // Java 26 writes one modifier as an object, several as a list
    let funcs: Vec<Json> = match e
        .get(if java { "modifier" } else { "functions" })
        .or_else(|| e.get("functions"))
    {
        Some(Json::Arr(a)) => a.clone(),
        Some(o @ Json::Obj(_)) => vec![o.clone()],
        _ => Vec::new(),
    };
    LootEntry {
        what,
        weight: e.get("weight").and_then(num).unwrap_or(1.0),
        functions: funcs
            .iter()
            .map(|f| if java { java_func(f) } else { bedrock_func(f) })
            .collect(),
        conditions: conds_of(e, java),
    }
}

fn table_of(v: &Json, java: bool) -> Option<Table> {
    let mut pools = Vec::new();
    for p in v
        .get("pools")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default()
    {
        if p.get("bonus_rolls").and_then(num).is_some_and(|b| b != 0.0) {
            return Some(Table {
                pools: vec![Pool {
                    rolls: Count::Exact(0.0),
                    entries: vec![],
                    conditions: vec![Cond::Other("bonus_rolls".into())],
                }],
            });
        }
        // entries keep their order: both games list them alike, and the
        // order lines them up to learn names
        let entries: Vec<LootEntry> = p
            .get("entries")
            .and_then(Json::as_array)?
            .iter()
            .map(|e| entry_of(e, java))
            .collect();
        let mut pool = Pool {
            rolls: p.get("rolls").and_then(count).unwrap_or(Count::Exact(1.0)),
            entries,
            conditions: conds_of(&p, java),
        };
        // Java's "the block survives the explosion" on a pool of single drops is
        // Bedrock's "each dropped item survives" (explosion_decay): with one
        // item they are the same chance. Bedrock's own tables say so.
        if java
            && pool.conditions.contains(&Cond::SurvivesExplosion)
            && pool.entries.iter().all(|e| {
                !e.functions
                    .iter()
                    .any(|f| matches!(f, Func::SetCount(_) | Func::Looting(_)))
            })
        {
            pool.conditions.retain(|c| *c != Cond::SurvivesExplosion);
            for e in &mut pool.entries {
                e.functions.push(Func::ExplosionDecay);
            }
        }
        pools.push(pool);
    }
    Some(Table { pools })
}

pub fn parse_java(v: &Json) -> Option<Table> {
    table_of(v, true)
}

fn bedrock_cond(v: &Json) -> Cond {
    match key_name(v).as_str() {
        "killed_by_player" | "killed_by_player_or_pets" => Cond::KilledByPlayer,
        "random_chance" => v
            .get("chance")
            .and_then(num)
            .map(Cond::Chance)
            .unwrap_or_else(|| Cond::Other(other(v))),
        "random_chance_with_looting" => match (
            v.get("chance").and_then(num),
            v.get("looting_multiplier").and_then(num),
        ) {
            (Some(c), Some(m)) => Cond::ChanceWithLooting(c, m),
            _ => Cond::Other(other(v)),
        },
        "entity_properties"
            if v.get("entity").and_then(Json::as_str) == Some("this")
                && v.get("properties").and_then(|p| p.get("on_fire"))
                    == Some(&Json::Bool(true)) =>
        {
            Cond::OnFire
        }
        _ => Cond::Other(other(v)),
    }
}

fn bedrock_func(v: &Json) -> Func {
    match key_name(v).as_str() {
        "set_count" if v.get("conditions").is_none() => v
            .get("count")
            .and_then(count)
            .map(Func::SetCount)
            .unwrap_or_else(|| Func::Other(other(v))),
        "looting_enchant" if v.get("limit").is_none() => v
            .get("count")
            .and_then(count)
            .map(Func::Looting)
            .unwrap_or_else(|| Func::Other(other(v))),
        "furnace_smelt" => Func::Smelt(conds_of(v, false)),
        "explosion_decay" => Func::ExplosionDecay,
        "set_damage" => v
            .get("damage")
            .and_then(count)
            .map(Func::SetDamage)
            .unwrap_or_else(|| Func::Other(other(v))),
        "enchant_randomly" if !matches!(v.get("treasure"), Some(Json::Bool(true))) => {
            Func::EnchantRandomly
        }
        "enchant_with_levels" => match v.get("levels").and_then(count) {
            Some(l) => {
                Func::EnchantWithLevels(l, matches!(v.get("treasure"), Some(Json::Bool(true))))
            }
            None => Func::Other(other(v)),
        },
        "set_potion" => match v.get("id").and_then(Json::as_str) {
            Some(id) => Func::SetPotion(id.to_string()),
            None => Func::Other(other(v)),
        },
        _ => Func::Other(other(v)),
    }
}

pub fn parse_bedrock(v: &Json) -> Option<Table> {
    table_of(v, false)
}

/// A table with nothing `Other` as a Bedrock loot table.
pub fn to_bedrock(t: &Table) -> Json {
    let s = |v: &str| Json::Str(v.to_string());
    let obj = |e: Vec<(&str, Json)>| {
        Json::Obj(JsonMap::from_entries(
            e.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        ))
    };
    let count = |c: &Count| match c {
        Count::Exact(n) => Json::Num(*n),
        Count::Range(a, b) => obj(vec![("min", Json::Num(*a)), ("max", Json::Num(*b))]),
    };
    let cond = |c: &Cond| match c {
        Cond::KilledByPlayer => obj(vec![("condition", s("killed_by_player"))]),
        Cond::Chance(p) => obj(vec![
            ("condition", s("random_chance")),
            ("chance", Json::Num(*p)),
        ]),
        Cond::ChanceWithLooting(p, m) => obj(vec![
            ("condition", s("random_chance_with_looting")),
            ("chance", Json::Num(*p)),
            ("looting_multiplier", Json::Num(*m)),
        ]),
        Cond::OnFire => obj(vec![
            ("condition", s("entity_properties")),
            ("entity", s("this")),
            ("properties", obj(vec![("on_fire", Json::Bool(true))])),
        ]),
        Cond::SurvivesExplosion | Cond::Other(_) => Json::Null,
    };
    let conds = |c: &[Cond]| Json::Arr(c.iter().map(cond).collect());
    let mut pools = Vec::new();
    for p in &t.pools {
        let mut entries = Vec::new();
        for e in &p.entries {
            let mut f = Vec::new();
            match &e.what {
                What::Item(i) => {
                    f.push(("type", s("item")));
                    f.push(("name", s(i)));
                }
                What::Empty => f.push(("type", s("empty"))),
                What::Table(n) => {
                    f.push(("type", s("loot_table")));
                    f.push(("name", s(&format!("loot_tables/{n}.json"))));
                }
                What::Other(_) => {}
            }
            f.push(("weight", Json::Num(e.weight)));
            let funcs: Vec<Json> = e
                .functions
                .iter()
                .map(|x| match x {
                    Func::SetCount(c) => {
                        obj(vec![("function", s("set_count")), ("count", count(c))])
                    }
                    Func::Looting(c) => obj(vec![
                        ("function", s("looting_enchant")),
                        ("count", count(c)),
                    ]),
                    Func::Smelt(c) => {
                        let mut v = vec![("function", s("furnace_smelt"))];
                        if !c.is_empty() {
                            v.push(("conditions", conds(c)));
                        }
                        obj(v)
                    }
                    Func::ExplosionDecay => obj(vec![("function", s("explosion_decay"))]),
                    Func::SetDamage(c) => {
                        obj(vec![("function", s("set_damage")), ("damage", count(c))])
                    }
                    Func::EnchantRandomly => obj(vec![("function", s("enchant_randomly"))]),
                    Func::EnchantWithLevels(l, t) => obj(vec![
                        ("function", s("enchant_with_levels")),
                        ("levels", count(l)),
                        ("treasure", Json::Bool(*t)),
                    ]),
                    Func::SetPotion(id) => obj(vec![("function", s("set_potion")), ("id", s(id))]),
                    Func::Other(_) => Json::Null,
                })
                .collect();
            if !funcs.is_empty() {
                f.push(("functions", Json::Arr(funcs)));
            }
            if !e.conditions.is_empty() {
                f.push(("conditions", conds(&e.conditions)));
            }
            entries.push(obj(f));
        }
        let mut pool = vec![("rolls", count(&p.rolls)), ("entries", Json::Arr(entries))];
        if !p.conditions.is_empty() {
            pool.push(("conditions", conds(&p.conditions)));
        }
        pools.push(obj(pool));
    }
    obj(vec![("pools", Json::Arr(pools))])
}

impl Table {
    /// The table without Java's `survives_explosion` condition.
    pub fn without_survives_explosion(&self) -> Table {
        let mut t = self.clone();
        for p in &mut t.pools {
            p.conditions.retain(|c| *c != Cond::SurvivesExplosion);
            for e in &mut p.entries {
                e.conditions.retain(|c| *c != Cond::SurvivesExplosion);
            }
        }
        t
    }

    fn has_survives_explosion(&self) -> bool {
        self.pools.iter().any(|p| {
            p.conditions.contains(&Cond::SurvivesExplosion)
                || p.entries
                    .iter()
                    .any(|e| e.conditions.contains(&Cond::SurvivesExplosion))
        })
    }
}

/// Java item name -> Bedrock item name, from pairs of tables at the same
/// path that are the same but for item names, entry by entry: a Java name
/// is renamed only when every pair agrees and no other Java name claims
/// the same Bedrock name.
pub fn learn_names(pairs: &[(Table, Table)]) -> std::collections::HashMap<String, String> {
    use std::collections::{HashMap, HashSet};
    let mut votes: HashMap<String, HashSet<String>> = HashMap::new();
    for (j, b) in pairs {
        let j = j.without_survives_explosion();
        if j.pools.len() != b.pools.len() {
            continue;
        }
        let mut found = Vec::new();
        let same_but_names = j.pools.iter().zip(&b.pools).all(|(jp, bp)| {
            jp.rolls == bp.rolls
                && jp.conditions == bp.conditions
                && jp.entries.len() == bp.entries.len()
                && jp.entries.iter().zip(&bp.entries).all(|(je, be)| {
                    let names = match (&je.what, &be.what) {
                        (What::Item(a), What::Item(b)) => {
                            found.push((a.clone(), b.clone()));
                            true
                        }
                        (a, b) => a == b,
                    };
                    names
                        && je.weight == be.weight
                        && je.functions == be.functions
                        && je.conditions == be.conditions
                })
        });
        if same_but_names {
            for (a, b) in found {
                votes.entry(a).or_default().insert(b);
            }
        }
    }
    // A name either game also uses as itself is not a rename: Java's
    // `copper_horse_armor` is Bedrock's too, so `iron_horse_armor` lined up
    // with it (the editions list those entries in another order) proves
    // nothing.
    let java_vocab: HashSet<String> = pairs.iter().flat_map(|(j, _)| j.items()).collect();
    let bedrock_vocab: HashSet<String> = pairs.iter().flat_map(|(_, b)| b.items()).collect();
    let mut map: HashMap<String, String> = HashMap::new();
    let mut claims: HashMap<String, usize> = HashMap::new();
    for (j, bs) in &votes {
        if let (1, Some(b)) = (bs.len(), bs.iter().next()) {
            if b != j && !bedrock_vocab.contains(j) && !java_vocab.contains(b) {
                map.insert(j.clone(), b.clone());
                *claims.entry(b.clone()).or_insert(0) += 1;
            }
        }
    }
    map.retain(|_, b| claims.get(b) == Some(&1));
    map
}

/// Does the table, with Java-only `survives_explosion` taken as Bedrock's
/// implicit behaviour, mean `bedrock`? Kept apart so the evidence for that
/// reading can be counted.
pub fn same_as_bedrock(java: &Table, bedrock: &Table) -> bool {
    java == bedrock
        || (java.has_survives_explosion() && &java.without_survives_explosion() == bedrock)
}

/// What the games' own loot tables prove.
#[derive(Default)]
pub struct LootRefs {
    /// Bedrock's tables by path (`entities/cow.json`).
    pub bedrock: std::collections::HashMap<String, Table>,
    /// Java item name -> Bedrock item name.
    pub names: std::collections::HashMap<String, String>,
    /// Every item Bedrock's own tables name.
    pub known: std::collections::HashSet<String>,
    /// Paths where the Java game's table means Bedrock's.
    pub same: std::collections::HashSet<String>,
}

impl LootRefs {
    pub fn build(install: &std::path::Path, jar: &crate::java::JavaJar) -> Self {
        use kebitx_core::vanilla::files::{map_files_in, PackKind};
        let parse = |b: &[u8]| kebitx_core::json::parse(&String::from_utf8_lossy(b)).ok();
        let bedrock: std::collections::HashMap<String, Table> = map_files_in(
            install,
            PackKind::Behavior,
            |p| p.starts_with("loot_tables/"),
            |_, b| parse(b).and_then(|j| parse_bedrock(&j)),
        )
        .into_iter()
        .map(|(p, t)| (p.trim_start_matches("loot_tables/").to_string(), t))
        .collect();
        let mut pairs = Vec::new();
        for n in jar
            .zip
            .names()
            .filter(|n| n.starts_with("data/minecraft/loot_table/") && n.ends_with(".json"))
        {
            let rel = n
                .trim_start_matches("data/minecraft/loot_table/")
                .to_string();
            if let (Some(b), Some(j)) = (
                bedrock.get(&rel),
                jar.read(n)
                    .and_then(|x| parse(&x))
                    .and_then(|j| parse_java(&j)),
            ) {
                pairs.push((rel, j, b.clone()));
            }
        }
        let plain: Vec<(Table, Table)> = pairs
            .iter()
            .map(|(_, j, b)| (j.clone(), b.clone()))
            .collect();
        let names = learn_names(&plain);
        let mut refs = LootRefs {
            known: bedrock.values().flat_map(Table::items).collect(),
            names,
            bedrock,
            ..Default::default()
        };
        for (rel, j, b) in &pairs {
            if same_as_bedrock(&refs.renamed(j), b) {
                refs.same.insert(rel.clone());
            }
        }
        refs
    }

    pub fn renamed(&self, t: &Table) -> Table {
        t.renamed(&|i| self.names.get(i).cloned().unwrap_or_else(|| i.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(s: &str) -> Json {
        kebitx_core::json::parse(s).unwrap()
    }

    #[test]
    fn the_cow_means_the_same_in_both_editions() {
        let java = parse_java(&j(r#"{"type":"minecraft:entity","pools":[{"entries":[{"type":"minecraft:item","modifier":[{"type":"minecraft:set_count","count":{"type":"minecraft:uniform","max":2,"min":0}},{"type":"minecraft:enchanted_count_increase","count":{"type":"minecraft:uniform","max":1.0,"min":0.0},"enchantment":"minecraft:looting"}],"name":"minecraft:leather"}],"rolls":1}]}"#)).unwrap();
        let bedrock = parse_bedrock(&j(r#"{"pools":[{"rolls":1,"entries":[{"type":"item","name":"minecraft:leather","weight":1,"functions":[{"function":"set_count","count":{"min":0,"max":2}},{"function":"looting_enchant","count":{"min":0,"max":1}}]}]}]}"#)).unwrap();
        assert_eq!(java, bedrock);
        assert!(java.others().is_empty());
        // and the conversion means it too
        assert_eq!(parse_bedrock(&to_bedrock(&java)).unwrap(), java);
    }

    #[test]
    fn what_is_not_understood_is_kept_apart() {
        let t = parse_java(&j(r#"{"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:x","modifier":[{"type":"minecraft:set_name","name":"y"}]}]}]}"#)).unwrap();
        assert_eq!(t.others().len(), 1);
    }
}
