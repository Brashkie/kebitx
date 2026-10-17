//! Java data-pack recipes → Bedrock behavior-pack recipes.
//!
//! A recipe is compared by what it MEANS, not how it is written: the grid
//! of ingredients (or the bag of them, without a shape), the result and its
//! count; for furnaces, input, output and stations. Both games ship their
//! own recipes, so the converter is checked against them: a Java vanilla
//! recipe, converted, must mean what the Bedrock vanilla recipe for the
//! same result means (`recipes_real` test).
//!
//! Names: `minecraft:` is implied when Bedrock leaves it out. An item or
//! tag either game names differently is only renamed by a pair the games'
//! own recipes prove (`learn_names`); anything else keeps its name, and a
//! name Bedrock's data never uses is reported, not guessed.

use kebitx_core::json::{Json, JsonMap};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// One ingredient: any of these items or tags.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Alt {
    Item(String),
    Tag(String),
}

pub type Ingredient = BTreeSet<Alt>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Station {
    Furnace,
    Blast,
    Smoker,
    Campfire,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Recipe {
    /// Rows of cells, trimmed to the pattern; `None` is an empty cell.
    Shaped {
        rows: Vec<Vec<Option<Ingredient>>>,
        result: (String, u32),
    },
    /// Ingredients sorted.
    Shapeless {
        ingredients: Vec<Ingredient>,
        result: (String, u32),
    },
    Cooking {
        input: Ingredient,
        result: String,
        stations: BTreeSet<Station>,
    },
}

impl Recipe {
    pub fn result(&self) -> &str {
        match self {
            Recipe::Shaped { result, .. } | Recipe::Shapeless { result, .. } => &result.0,
            Recipe::Cooking { result, .. } => result,
        }
    }

    /// Every item and tag the recipe names.
    pub fn names(&self) -> Vec<Alt> {
        let mut out: Vec<Alt> = Vec::new();
        let mut add = |i: &Ingredient| out.extend(i.iter().cloned());
        match self {
            Recipe::Shaped { rows, .. } => rows.iter().flatten().flatten().for_each(&mut add),
            Recipe::Shapeless { ingredients, .. } => ingredients.iter().for_each(&mut add),
            Recipe::Cooking { input, .. } => add(input),
        }
        out.push(Alt::Item(self.result().to_string()));
        out
    }

    /// The same recipe with names replaced.
    pub fn renamed(&self, f: &dyn Fn(&Alt) -> Alt) -> Recipe {
        let ing = |i: &Ingredient| i.iter().map(f).collect::<Ingredient>();
        let item = |s: &str| match f(&Alt::Item(s.to_string())) {
            Alt::Item(x) | Alt::Tag(x) => x,
        };
        match self {
            Recipe::Shaped { rows, result } => Recipe::Shaped {
                rows: rows
                    .iter()
                    .map(|r| r.iter().map(|c| c.as_ref().map(ing)).collect())
                    .collect(),
                result: (item(&result.0), result.1),
            },
            Recipe::Shapeless {
                ingredients,
                result,
            } => {
                let mut v: Vec<Ingredient> = ingredients.iter().map(ing).collect();
                v.sort();
                Recipe::Shapeless {
                    ingredients: v,
                    result: (item(&result.0), result.1),
                }
            }
            Recipe::Cooking {
                input,
                result,
                stations,
            } => Recipe::Cooking {
                input: ing(input),
                result: item(result),
                stations: stations.clone(),
            },
        }
    }
}

/// `stick` -> `minecraft:stick`.
pub fn full(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

fn java_ingredient(v: &Json) -> Option<Ingredient> {
    let alt = |s: &str| match s.strip_prefix('#') {
        Some(t) => Alt::Tag(full(t)),
        None => Alt::Item(full(s)),
    };
    let one = |v: &Json| -> Option<Alt> {
        match v {
            Json::Str(s) => Some(alt(s)),
            // the older object forms
            Json::Obj(_) => match (
                v.get("item").and_then(Json::as_str),
                v.get("tag").and_then(Json::as_str),
            ) {
                (Some(i), _) => Some(Alt::Item(full(i))),
                (None, Some(t)) => Some(Alt::Tag(full(t))),
                _ => None,
            },
            _ => None,
        }
    };
    let set: Ingredient = match v {
        Json::Arr(a) => a.iter().map(one).collect::<Option<_>>()?,
        _ => [one(v)?].into(),
    };
    (!set.is_empty()).then_some(set)
}

fn count_of(v: Option<&Json>) -> u32 {
    match v {
        Some(Json::Num(n)) if *n >= 1.0 => *n as u32,
        _ => 1,
    }
}

/// Rows of a pattern trimmed of empty edges (Java and Bedrock both allow
/// padding spaces), cells looked up in `key`.
fn grid(
    pattern: &[Json],
    key: &dyn Fn(char) -> Option<Option<Ingredient>>,
) -> Option<Vec<Vec<Option<Ingredient>>>> {
    let rows: Vec<Vec<char>> = pattern
        .iter()
        .map(|r| r.as_str().map(|s| s.chars().collect()))
        .collect::<Option<_>>()?;
    let width = rows.iter().map(Vec::len).max()?;
    let mut cells: Vec<Vec<Option<Ingredient>>> = Vec::new();
    for r in &rows {
        let mut row = Vec::new();
        for i in 0..width {
            let c = r.get(i).copied().unwrap_or(' ');
            row.push(if c == ' ' { None } else { key(c)? });
        }
        cells.push(row);
    }
    // trim empty rows and columns
    while cells.first().is_some_and(|r| r.iter().all(Option::is_none)) {
        cells.remove(0);
    }
    while cells.last().is_some_and(|r| r.iter().all(Option::is_none)) {
        cells.pop();
    }
    let used: Vec<usize> = (0..width)
        .filter(|&i| cells.iter().any(|r| r[i].is_some()))
        .collect();
    let (lo, hi) = (*used.first()?, *used.last()?);
    Some(cells.into_iter().map(|r| r[lo..=hi].to_vec()).collect())
}

/// A Java recipe file. `None` for types with no meaning to compare here
/// (smithing, special crafting, ...).
pub fn parse_java(v: &Json) -> Option<Recipe> {
    let kind = v
        .get("type")
        .and_then(Json::as_str)?
        .trim_start_matches("minecraft:");
    let result = |v: &Json| -> Option<(String, u32)> {
        let r = v.get("result")?;
        match r {
            Json::Str(s) => Some((full(s), 1)),
            _ => Some((
                full(
                    r.get("id")
                        .or_else(|| r.get("item"))
                        .and_then(Json::as_str)?,
                ),
                count_of(r.get("count")),
            )),
        }
    };
    match kind {
        "crafting_shaped" => {
            let key = v.get("key")?;
            let lookup = |c: char| -> Option<Option<Ingredient>> {
                Some(Some(java_ingredient(key.get(&c.to_string())?)?))
            };
            Some(Recipe::Shaped {
                rows: grid(v.get("pattern")?.as_array()?, &lookup)?,
                result: result(v)?,
            })
        }
        "crafting_shapeless" => {
            let mut ingredients: Vec<Ingredient> = v
                .get("ingredients")?
                .as_array()?
                .iter()
                .map(java_ingredient)
                .collect::<Option<_>>()?;
            ingredients.sort();
            Some(Recipe::Shapeless {
                ingredients,
                result: result(v)?,
            })
        }
        "smelting" | "blasting" | "smoking" | "campfire_cooking" => {
            let station = match kind {
                "smelting" => Station::Furnace,
                "blasting" => Station::Blast,
                "smoking" => Station::Smoker,
                _ => Station::Campfire,
            };
            Some(Recipe::Cooking {
                input: java_ingredient(v.get("ingredient")?)?,
                result: result(v)?.0,
                stations: [station].into(),
            })
        }
        _ => None,
    }
}

fn bedrock_ingredient(v: &Json) -> Option<Ingredient> {
    let one = |v: &Json| -> Option<Alt> {
        match v {
            Json::Str(s) => {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() == 3 && parts[2] != "0" {
                    return None; // a data value: another item
                }
                Some(Alt::Item(full(&parts[..parts.len().min(2)].join(":"))))
            }
            Json::Obj(_) => {
                // a data value or extra state makes it another item: not compared
                if v.get("data")
                    .is_some_and(|d| !matches!(d, Json::Num(n) if *n == 0.0))
                {
                    return None;
                }
                match (
                    v.get("item").and_then(Json::as_str),
                    v.get("tag").and_then(Json::as_str),
                ) {
                    (Some(i), _) => Some(Alt::Item(full(i))),
                    (None, Some(t)) => Some(Alt::Tag(full(t))),
                    _ => None,
                }
            }
            _ => None,
        }
    };
    let set: Ingredient = match v {
        Json::Arr(a) => a.iter().map(one).collect::<Option<_>>()?,
        _ => [one(v)?].into(),
    };
    (!set.is_empty()).then_some(set)
}

/// A Bedrock recipe file.
pub fn parse_bedrock(v: &Json) -> Option<Recipe> {
    let result = |r: &Json| -> Option<(String, u32)> {
        let r = match r {
            Json::Arr(a) if a.len() == 1 => &a[0],
            Json::Arr(_) => return None,
            other => other,
        };
        if r.get("data")
            .is_some_and(|d| !matches!(d, Json::Num(n) if *n == 0.0))
        {
            return None;
        }
        match r {
            Json::Str(s) => Some((full(s), 1)),
            _ => Some((
                full(r.get("item").and_then(Json::as_str)?),
                count_of(r.get("count")),
            )),
        }
    };
    if let Some(s) = v.get("minecraft:recipe_shaped") {
        if !s
            .get("tags")?
            .as_array()?
            .iter()
            .any(|t| t.as_str() == Some("crafting_table"))
        {
            return None;
        }
        let key = s.get("key")?;
        let lookup = |c: char| -> Option<Option<Ingredient>> {
            Some(Some(bedrock_ingredient(key.get(&c.to_string())?)?))
        };
        return Some(Recipe::Shaped {
            rows: grid(s.get("pattern")?.as_array()?, &lookup)?,
            result: result(s.get("result")?)?,
        });
    }
    if let Some(s) = v.get("minecraft:recipe_shapeless") {
        if !s
            .get("tags")?
            .as_array()?
            .iter()
            .any(|t| t.as_str() == Some("crafting_table"))
        {
            return None;
        }
        let mut ingredients = Vec::new();
        for i in s.get("ingredients")?.as_array()? {
            let ing = bedrock_ingredient(i)?;
            for _ in 0..count_of(i.get("count")) {
                ingredients.push(ing.clone());
            }
        }
        ingredients.sort();
        return Some(Recipe::Shapeless {
            ingredients,
            result: result(s.get("result")?)?,
        });
    }
    if let Some(s) = v.get("minecraft:recipe_furnace") {
        let mut stations = BTreeSet::new();
        for t in s.get("tags")?.as_array()? {
            match t.as_str()? {
                "furnace" => stations.insert(Station::Furnace),
                "blast_furnace" => stations.insert(Station::Blast),
                "smoker" => stations.insert(Station::Smoker),
                "campfire" | "soul_campfire" => stations.insert(Station::Campfire),
                _ => false,
            };
        }
        return Some(Recipe::Cooking {
            input: bedrock_ingredient(s.get("input")?)?,
            result: result(s.get("output")?)?.0,
            stations,
        });
    }
    None
}

/// Java cooking recipes for the same input and output, one per station,
/// merged into one: Bedrock writes them as one recipe with several tags.
pub fn merge_cooking(recipes: Vec<Recipe>) -> Vec<Recipe> {
    let mut out = Vec::new();
    let mut cooking: BTreeMap<(String, Vec<Alt>), BTreeSet<Station>> = BTreeMap::new();
    for r in recipes {
        match r {
            Recipe::Cooking {
                input,
                result,
                stations,
            } => {
                cooking
                    .entry((result, input.into_iter().collect()))
                    .or_default()
                    .extend(stations);
            }
            other => out.push(other),
        }
    }
    for ((result, input), stations) in cooking {
        out.push(Recipe::Cooking {
            input: input.into_iter().collect(),
            result,
            stations,
        });
    }
    out
}

/// Java name -> Bedrock name, proven by the games' own recipes: in a pair
/// of recipes with the same result that differ only in names, cell by cell,
/// the names that stand where the other game's names stand. A Java name is
/// renamed only when every pair agrees on the same Bedrock name, and no
/// other Java name claims it.
pub fn learn_names(java: &[Recipe], bedrock: &[Recipe]) -> HashMap<Alt, Alt> {
    let mut by_result: HashMap<&str, Vec<&Recipe>> = HashMap::new();
    for b in bedrock {
        by_result.entry(b.result()).or_default().push(b);
    }
    let mut votes: HashMap<Alt, HashSet<Alt>> = HashMap::new();
    for j in java {
        let Some(cands) = by_result.get(j.result()) else {
            continue;
        };
        // the one Bedrock recipe of that result with the same shape
        let aligned: Vec<Vec<(Alt, Alt)>> = cands.iter().filter_map(|b| align(j, b)).collect();
        if let [pairs] = aligned.as_slice() {
            for (a, b) in pairs {
                votes.entry(a.clone()).or_default().insert(b.clone());
            }
        }
    }
    let mut map = HashMap::new();
    let mut claimed: HashMap<Alt, usize> = HashMap::new();
    for (j, bs) in &votes {
        if let Some(b) = bs.iter().next().filter(|_| bs.len() == 1) {
            if b != j {
                *claimed.entry(b.clone()).or_insert(0) += 1;
                map.insert(j.clone(), b.clone());
            }
        }
    }
    map.retain(|_, b| claimed.get(b) == Some(&1));
    map
}

/// Two recipes of the same shape: the (Java, Bedrock) names standing in
/// the same places, each ingredient a single name. `None` if the shapes or
/// counts differ.
fn align(j: &Recipe, b: &Recipe) -> Option<Vec<(Alt, Alt)>> {
    let single = |i: &Ingredient| -> Option<Alt> {
        (i.len() == 1).then(|| i.iter().next().cloned()).flatten()
    };
    let mut pairs = Vec::new();
    match (j, b) {
        (
            Recipe::Shaped {
                rows: jr,
                result: (_, jc),
            },
            Recipe::Shaped {
                rows: br,
                result: (_, bc),
            },
        ) => {
            if jc != bc
                || jr.len() != br.len()
                || jr.iter().zip(br).any(|(a, b)| a.len() != b.len())
            {
                return None;
            }
            for (a, b) in jr.iter().flatten().zip(br.iter().flatten()) {
                match (a, b) {
                    (None, None) => {}
                    (Some(a), Some(b)) => pairs.push((single(a)?, single(b)?)),
                    _ => return None,
                }
            }
        }
        (
            Recipe::Cooking {
                input: ji,
                stations: js,
                ..
            },
            Recipe::Cooking {
                input: bi,
                stations: bs,
                ..
            },
        ) => {
            if js != bs {
                return None;
            }
            pairs.push((single(ji)?, single(bi)?));
        }
        _ => return None,
    }
    Some(pairs)
}

/// A recipe as a Bedrock recipe file. `id` is its identifier.
pub fn to_bedrock(id: &str, r: &Recipe) -> Json {
    let s = |v: &str| Json::Str(v.to_string());
    let obj = |e: Vec<(&str, Json)>| {
        Json::Obj(JsonMap::from_entries(
            e.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        ))
    };
    let alt = |a: &Alt| match a {
        Alt::Item(i) => obj(vec![("item", s(i))]),
        Alt::Tag(t) => obj(vec![("tag", s(t))]),
    };
    let ing = |i: &Ingredient| -> Json {
        let v: Vec<Json> = i.iter().map(alt).collect();
        if v.len() == 1 {
            v.into_iter().next().unwrap_or(Json::Null)
        } else {
            Json::Arr(v)
        }
    };
    let result = |(item, count): &(String, u32)| {
        obj(vec![("item", s(item)), ("count", Json::Num(*count as f64))])
    };
    let description = obj(vec![("identifier", s(id))]);
    let (key, body) = match r {
        Recipe::Shaped { rows, result: res } => {
            // one symbol per distinct ingredient
            let mut symbols: Vec<&Ingredient> = Vec::new();
            let mut pattern = Vec::new();
            for row in rows {
                let mut line = String::new();
                for cell in row {
                    match cell {
                        None => line.push(' '),
                        Some(i) => {
                            let n = symbols.iter().position(|x| *x == i).unwrap_or_else(|| {
                                symbols.push(i);
                                symbols.len() - 1
                            });
                            line.push((b'A' + n as u8) as char);
                        }
                    }
                }
                pattern.push(Json::Str(line));
            }
            let key: Vec<(String, Json)> = symbols
                .iter()
                .enumerate()
                .map(|(n, i)| (((b'A' + n as u8) as char).to_string(), ing(i)))
                .collect();
            (
                "minecraft:recipe_shaped",
                obj(vec![
                    ("description", description),
                    ("tags", Json::Arr(vec![s("crafting_table")])),
                    ("pattern", Json::Arr(pattern)),
                    ("key", Json::Obj(JsonMap::from_entries(key))),
                    ("result", result(res)),
                ]),
            )
        }
        Recipe::Shapeless {
            ingredients,
            result: res,
        } => (
            "minecraft:recipe_shapeless",
            obj(vec![
                ("description", description),
                ("tags", Json::Arr(vec![s("crafting_table")])),
                (
                    "ingredients",
                    Json::Arr(ingredients.iter().map(ing).collect()),
                ),
                ("result", result(res)),
            ]),
        ),
        Recipe::Cooking {
            input,
            result: out,
            stations,
        } => {
            let mut tags = Vec::new();
            for st in stations {
                match st {
                    Station::Furnace => tags.push(s("furnace")),
                    Station::Blast => tags.push(s("blast_furnace")),
                    Station::Smoker => tags.push(s("smoker")),
                    Station::Campfire => {
                        tags.push(s("campfire"));
                        tags.push(s("soul_campfire"));
                    }
                }
            }
            (
                "minecraft:recipe_furnace",
                obj(vec![
                    ("description", description),
                    ("tags", Json::Arr(tags)),
                    ("input", ing(input)),
                    ("output", s(out)),
                ]),
            )
        }
    };
    obj(vec![("format_version", s("1.20.10")), (key, body)])
}

/// What the games' own recipes prove.
#[derive(Default)]
pub struct RecipeRefs {
    /// Bedrock's recipes, by meaning -> identifier.
    pub bedrock: HashMap<Recipe, String>,
    /// Every item and tag Bedrock's recipes name.
    pub known: HashSet<Alt>,
    /// Java name -> Bedrock name, learned from the two games' recipes.
    pub names: HashMap<Alt, Alt>,
    /// Java vanilla recipe file -> the Bedrock identifier of the same recipe.
    pub same_as: HashMap<String, String>,
}

impl RecipeRefs {
    /// From the installed Bedrock game's behavior packs and a Java `.jar`.
    pub fn build(install: &std::path::Path, jar: &crate::java::JavaJar) -> Self {
        use kebitx_core::vanilla::files::{map_files_in, PackKind};
        let parse = |b: &[u8]| kebitx_core::json::parse(&String::from_utf8_lossy(b)).ok();
        let files = map_files_in(
            install,
            PackKind::Behavior,
            |p| p.starts_with("recipes/"),
            |_, b| {
                let j = parse(b)?;
                let r = parse_bedrock(&j)?;
                let id = [
                    "minecraft:recipe_shaped",
                    "minecraft:recipe_shapeless",
                    "minecraft:recipe_furnace",
                ]
                .iter()
                .find_map(|k| j.get(k))
                .and_then(|s| s.get("description"))
                .and_then(|d| d.get("identifier"))
                .and_then(Json::as_str)?
                .to_string();
                Some((r, id))
            },
        );
        let mut refs = RecipeRefs::default();
        for (_, (r, id)) in files {
            refs.known.extend(r.names());
            refs.bedrock.insert(r, id);
        }
        // the creative catalog names every item the game has; it is stored
        // as binary JSON, but each id is plain text: `minecraft:oak_planks`
        let catalogs = map_files_in(
            install,
            PackKind::Behavior,
            |p| p.starts_with("item_catalog/"),
            |_, b| Some(catalog_ids(b)),
        );
        for ids in catalogs.into_values() {
            refs.known.extend(ids.into_iter().map(Alt::Item));
        }
        let java_files: Vec<String> = jar
            .zip
            .names()
            .filter(|n| n.starts_with("data/minecraft/recipe/"))
            .cloned()
            .collect();
        let java: Vec<(String, Recipe)> = java_files
            .iter()
            .filter_map(|n| Some((n.clone(), parse_java(&parse(&jar.read(n)?)?)?)))
            .collect();
        let bedrock: Vec<Recipe> = refs.bedrock.keys().cloned().collect();
        let merged = merge_cooking(java.iter().map(|(_, r)| r.clone()).collect());
        refs.names = learn_names(&merged, &bedrock);
        for (file, r) in &java {
            if let Some(id) = refs.bedrock.get(&refs.to_bedrock_names(r)) {
                refs.same_as.insert(file.clone(), id.clone());
            }
        }
        refs
    }

    pub fn to_bedrock_names(&self, r: &Recipe) -> Recipe {
        r.renamed(&|a| self.names.get(a).cloned().unwrap_or_else(|| a.clone()))
    }

    /// Names in `r` (already renamed) that Bedrock's data never uses.
    pub fn unknown(&self, r: &Recipe) -> Vec<Alt> {
        r.names()
            .into_iter()
            .filter(|a| !self.known.contains(a))
            .collect()
    }
}

/// The items of a Java item tag (`minecraft:planks`), following nested
/// tags, from the pack first, then the Java game. `None` if a tag in the
/// chain cannot be read.
pub fn tag_items(
    tag: &str,
    pack: &std::path::Path,
    jar: Option<&crate::java::JavaJar>,
) -> Option<BTreeSet<String>> {
    fn walk(
        tag: &str,
        pack: &std::path::Path,
        jar: Option<&crate::java::JavaJar>,
        depth: usize,
        out: &mut BTreeSet<String>,
    ) -> Option<()> {
        if depth > 16 {
            return None;
        }
        let (ns, name) = tag.split_once(':').unwrap_or(("minecraft", tag));
        let mut text = None;
        for folder in ["item", "items"] {
            let p = format!("data/{ns}/tags/{folder}/{name}.json");
            if let Ok(b) = std::fs::read(pack.join(&p)) {
                text = Some(b);
                break;
            }
            if let Some(b) = jar.and_then(|j| j.read(&p)) {
                text = Some(b);
                break;
            }
        }
        let v = kebitx_core::json::parse(&String::from_utf8_lossy(&text?)).ok()?;
        for value in v.get("values")?.as_array()? {
            let id = match value {
                Json::Str(s) => s.as_str(),
                Json::Obj(_) => value.get("id")?.as_str()?,
                _ => return None,
            };
            match id.strip_prefix('#') {
                Some(nested) => walk(nested, pack, jar, depth + 1, out)?,
                None => {
                    out.insert(full(id));
                }
            }
        }
        Some(())
    }
    let mut out = BTreeSet::new();
    walk(tag, pack, jar, 0, &mut out)?;
    (!out.is_empty()).then_some(out)
}

/// Tags Bedrock does not have, replaced by their Java items.
pub fn expand_unknown_tags(
    r: &Recipe,
    known: &HashSet<Alt>,
    items_of: &dyn Fn(&str) -> Option<BTreeSet<String>>,
) -> Option<Recipe> {
    let mut failed = false;
    let ing = |i: &Ingredient, failed: &mut bool| -> Ingredient {
        let mut out = Ingredient::new();
        for a in i {
            match a {
                Alt::Tag(t) if !known.contains(a) => match items_of(t) {
                    Some(items) => out.extend(items.into_iter().map(Alt::Item)),
                    None => *failed = true,
                },
                other => {
                    out.insert(other.clone());
                }
            }
        }
        out
    };
    let r = match r {
        Recipe::Shaped { rows, result } => Recipe::Shaped {
            rows: rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|c| c.as_ref().map(|i| ing(i, &mut failed)))
                        .collect()
                })
                .collect(),
            result: result.clone(),
        },
        Recipe::Shapeless {
            ingredients,
            result,
        } => {
            let mut v: Vec<Ingredient> = ingredients.iter().map(|i| ing(i, &mut failed)).collect();
            v.sort();
            Recipe::Shapeless {
                ingredients: v,
                result: result.clone(),
            }
        }
        Recipe::Cooking {
            input,
            result,
            stations,
        } => Recipe::Cooking {
            input: ing(input, &mut failed),
            result: result.clone(),
            stations: stations.clone(),
        },
    };
    (!failed).then_some(r)
}

/// The recipe as recipes with one item or tag per ingredient: every
/// combination of the alternatives (a cell that takes "red sandstone, cut or
/// chiseled" is three recipes). `None` above `limit` recipes.
pub fn singles(r: &Recipe, limit: usize) -> Option<Vec<Recipe>> {
    // the distinct ingredients with alternatives, each chosen once
    let mut choices: Vec<Ingredient> = Vec::new();
    let mut note = |i: &Ingredient| {
        if i.len() > 1 && !choices.contains(i) {
            choices.push(i.clone());
        }
    };
    match r {
        Recipe::Shaped { rows, .. } => rows.iter().flatten().flatten().for_each(&mut note),
        Recipe::Shapeless { ingredients, .. } => ingredients.iter().for_each(&mut note),
        Recipe::Cooking { input, .. } => note(input),
    }
    let total = choices
        .iter()
        .try_fold(1usize, |n, c| n.checked_mul(c.len()))?;
    if total > limit {
        return None;
    }
    let mut out = Vec::with_capacity(total);
    for k in 0..total {
        let mut rest = k;
        let picked: Vec<(Ingredient, Ingredient)> = choices
            .iter()
            .map(|c| {
                let alt = c
                    .iter()
                    .nth(rest % c.len())
                    .cloned()
                    .map(|a| [a].into())
                    .unwrap_or_default();
                rest /= c.len();
                (c.clone(), alt)
            })
            .collect();
        let pick = |i: &Ingredient| {
            picked
                .iter()
                .find(|(c, _)| c == i)
                .map(|(_, a)| a.clone())
                .unwrap_or_else(|| i.clone())
        };
        out.push(match r {
            Recipe::Shaped { rows, result } => Recipe::Shaped {
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(|c| c.as_ref().map(pick)).collect())
                    .collect(),
                result: result.clone(),
            },
            Recipe::Shapeless {
                ingredients,
                result,
            } => {
                let mut v: Vec<Ingredient> = ingredients.iter().map(pick).collect();
                v.sort();
                Recipe::Shapeless {
                    ingredients: v,
                    result: result.clone(),
                }
            }
            Recipe::Cooking {
                input,
                result,
                stations,
            } => Recipe::Cooking {
                input: pick(input),
                result: result.clone(),
                stations: stations.clone(),
            },
        });
    }
    Some(out)
}

/// Every `minecraft:<id>` written in `bytes` (ids are lowercase letters,
/// digits and `_`).
pub fn catalog_ids(bytes: &[u8]) -> Vec<String> {
    let needle = b"minecraft:";
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let start = i + needle.len();
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_lowercase()
                    || bytes[end].is_ascii_digit()
                    || bytes[end] == b'_')
            {
                end += 1;
            }
            // `minecraft:` written again right after: the id ends there
            let id = String::from_utf8_lossy(&bytes[start..end]).to_string();
            let id = id.split("minecraft").next().unwrap_or("").to_string();
            if !id.is_empty() {
                out.push(format!("minecraft:{id}"));
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(s: &str) -> Json {
        kebitx_core::json::parse(s).unwrap()
    }

    #[test]
    fn both_editions_mean_the_same_crafting_table() {
        let java = parse_java(&j(r###"{"type":"minecraft:crafting_shaped","key":{"#":"#minecraft:planks"},"pattern":["##","##"],"result":{"id":"minecraft:crafting_table"}}"###)).unwrap();
        let bedrock = parse_bedrock(&j(r#"{"format_version":"1.20.10","minecraft:recipe_shaped":{"description":{"identifier":"minecraft:WorkBench_recipeId"},"tags":["crafting_table"],"pattern":["AA","AA"],"key":{"A":{"tag":"minecraft:planks"}},"result":{"item":"crafting_table"}}}"#)).unwrap();
        assert_eq!(java, bedrock);
        // and the conversion means it too
        assert_eq!(parse_bedrock(&to_bedrock("kebitx:t", &java)).unwrap(), java);
    }

    #[test]
    fn padding_is_trimmed_and_cooking_merges() {
        let a = parse_java(&j(r#"{"type":"minecraft:crafting_shaped","key":{"x":"minecraft:stick"},"pattern":[" x ","   "],"result":{"id":"minecraft:torch","count":4}}"#)).unwrap();
        let b = parse_java(&j(r#"{"type":"minecraft:crafting_shaped","key":{"x":"minecraft:stick"},"pattern":["x"],"result":{"id":"minecraft:torch","count":4}}"#)).unwrap();
        assert_eq!(a, b);
        let s = parse_java(&j(r#"{"type":"minecraft:smelting","ingredient":"minecraft:raw_iron","result":{"id":"minecraft:iron_ingot"}}"#)).unwrap();
        let bl = parse_java(&j(r#"{"type":"minecraft:blasting","ingredient":"minecraft:raw_iron","result":{"id":"minecraft:iron_ingot"}}"#)).unwrap();
        let merged = merge_cooking(vec![s, bl]);
        assert_eq!(merged.len(), 1);
        let b = parse_bedrock(&j(r#"{"minecraft:recipe_furnace":{"tags":["furnace","blast_furnace"],"input":"minecraft:raw_iron","output":"minecraft:iron_ingot"}}"#)).unwrap();
        assert_eq!(merged[0], b);
    }

    #[test]
    fn names_are_learned_only_from_agreeing_pairs() {
        let java = vec![parse_java(&j(r#"{"type":"minecraft:smelting","ingredient":"minecraft:a_java","result":{"id":"minecraft:out"}}"#)).unwrap()];
        let bedrock = vec![parse_bedrock(&j(r#"{"minecraft:recipe_furnace":{"tags":["furnace"],"input":"minecraft:a_bedrock","output":"out"}}"#)).unwrap()];
        let map = learn_names(&java, &bedrock);
        assert_eq!(
            map.get(&Alt::Item("minecraft:a_java".into())),
            Some(&Alt::Item("minecraft:a_bedrock".into()))
        );
    }
}
