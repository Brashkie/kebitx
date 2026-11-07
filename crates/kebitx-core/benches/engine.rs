//! Benchmarks of the engine's hot paths, with a harness of a few lines: each
//! one runs for its time budget and reports its fastest and median run.
//!
//! ```text
//! cargo bench -p kebitx-core                       # fixtures + synthetic input
//! KEBITX_BENCH_WORLD=<world folder> cargo bench -p kebitx-core   # + a real world
//! ```
//!
//! Without `KEBITX_BENCH_WORLD` the world benchmarks run on the fixtures, which
//! are tiny; the numbers that matter come from a real world. The game catalog
//! benchmark runs only when an install is found.

use kebitx_core::analyze::analyze_world_with;
use kebitx_core::compat::check_world;
use kebitx_core::graphics;
use kebitx_core::json;
use kebitx_core::manifest::packs_in;
use kebitx_core::passes::{duplicate_resource, unused_asset, unused_definition};
use kebitx_core::vanilla::VanillaCatalog;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Runs `f` for `budget` (at least 10 times) and prints its times; with `bytes`,
/// the speed at the fastest run too.
fn bench<T>(name: &str, budget: Duration, bytes: Option<usize>, mut f: impl FnMut() -> T) {
    black_box(f()); // warm up
    let start = Instant::now();
    let mut runs = Vec::new();
    while runs.len() < 10 || start.elapsed() < budget {
        let t = Instant::now();
        black_box(f());
        runs.push(t.elapsed());
    }
    runs.sort();
    let speed = bytes
        .map(|b| {
            format!(
                "  {:.0} MB/s",
                b as f64 / 1e6 / runs[0].as_secs_f64().max(1e-9)
            )
        })
        .unwrap_or_default();
    println!(
        "{name:<34} fastest {:>12?}  median {:>12?}  ({} runs){speed}",
        runs[0],
        runs[runs.len() / 2],
        runs.len()
    );
}

fn world() -> PathBuf {
    std::env::var_os("KEBITX_BENCH_WORLD")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/world_assets")
        })
}

/// A model file like the big ones in real packs: thousands of cubes with
/// per-face UVs, comments, escapes, non-ASCII names.
fn synthetic_model(cubes: usize) -> String {
    let mut s = String::from(
        "{\n  // generated\n  \"format_version\": \"1.12.0\",\n  \"minecraft:geometry\": [{\n    \"description\": {\"identifier\": \"geometry.bench\", \"texture_width\": 128, \"texture_height\": 128},\n    \"bones\": [{\"name\": \"cuerpo_ñ\", \"pivot\": [0, 24, 0], \"cubes\": [\n",
    );
    for i in 0..cubes {
        if i > 0 {
            s.push_str(",\n");
        }
        s.push_str(&format!(
            "      {{\"origin\": [{}.5, -{}.25, 3.125], \"size\": [4, 12.5, 4], \"inflate\": 0.25, \"uv\": {{\
             \"north\": {{\"uv\": [{}, 16], \"uv_size\": [4, 12]}}, \"south\": {{\"uv\": [8, 16], \"uv_size\": [-4, 12]}}, \
             \"up\": {{\"uv\": [4, 16], \"uv_size\": [4, -4]}}, \"down\": {{\"uv\": [8, 20], \"uv_size\": [4, 4]}}}}, \
             \"name\": \"c\\u00e9\\\"{i}\"}}",
            i % 64,
            i % 32,
            i % 120
        ));
    }
    s.push_str("\n    ]}]\n  }]\n}\n");
    s
}

fn main() {
    const WORLD: Duration = Duration::from_secs(5);
    let src = synthetic_model(5_000);
    bench(
        "json: parse model 5000 cubes",
        Duration::from_secs(3),
        Some(src.len()),
        || json::parse(black_box(&src)).unwrap(),
    );

    if let Some(install) = kebitx_core::vanilla::install_dir() {
        bench("catalog: build from install", WORLD, None, || {
            VanillaCatalog::from_install(black_box(&install))
        });
    }

    let w = world();
    let catalog = VanillaCatalog::empty();
    let packs = packs_in(&w);
    bench("world: references", WORLD, None, || {
        analyze_world_with(black_box(&w), &catalog)
    });
    bench("world: compatibility", WORLD, None, || {
        check_world(black_box(&w), None)
    });
    bench("world: graphics", WORLD, None, || {
        graphics::check(black_box(&packs), &catalog)
    });
    bench("world: unused assets", WORLD, None, || {
        unused_asset::run(black_box(&packs), &catalog)
    });
    bench("world: unused definitions", WORLD, None, || {
        unused_definition::run(black_box(&packs), &catalog)
    });
    bench("world: identical files", WORLD, None, || {
        duplicate_resource::run(black_box(&packs))
    });
}
