//! Benchmarks of the engine's hot paths, with criterion.
//!
//! ```text
//! cargo bench -p kebitx-core                       # fixtures + synthetic input
//! KEBITX_BENCH_WORLD=<world folder> cargo bench -p kebitx-core   # + a real world
//! ```
//!
//! Without `KEBITX_BENCH_WORLD` the world benchmarks run on the fixtures, which
//! are tiny; the numbers that matter come from a real world. The game catalog
//! benchmark runs only when an install is found.

use criterion::{criterion_group, criterion_main, Criterion};
use kebitx_core::analyze::analyze_world_with;
use kebitx_core::compat::check_world;
use kebitx_core::graphics;
use kebitx_core::json;
use kebitx_core::manifest::packs_in;
use kebitx_core::passes::{duplicate_resource, unused_asset, unused_definition};
use kebitx_core::vanilla::VanillaCatalog;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Duration;

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

fn bench_json(c: &mut Criterion) {
    let src = synthetic_model(5_000);
    let mut g = c.benchmark_group("json");
    g.throughput(criterion::Throughput::Bytes(src.len() as u64));
    g.bench_function("parse model 5000 cubes", |b| {
        b.iter(|| json::parse(black_box(&src)).unwrap())
    });
    g.finish();
}

fn bench_catalog(c: &mut Criterion) {
    let Some(install) = kebitx_core::vanilla::install_dir() else {
        return;
    };
    let mut g = c.benchmark_group("catalog");
    g.sample_size(10).measurement_time(Duration::from_secs(30));
    g.bench_function("build from install", |b| {
        b.iter(|| VanillaCatalog::from_install(black_box(&install)))
    });
    g.finish();
}

fn bench_world(c: &mut Criterion) {
    let w = world();
    let catalog = VanillaCatalog::empty();
    let packs = packs_in(&w);
    let mut g = c.benchmark_group("world");
    g.sample_size(10).measurement_time(Duration::from_secs(20));
    g.bench_function("references", |b| {
        b.iter(|| analyze_world_with(black_box(&w), &catalog))
    });
    g.bench_function("compatibility", |b| {
        b.iter(|| check_world(black_box(&w), None))
    });
    g.bench_function("graphics", |b| {
        b.iter(|| graphics::check(black_box(&packs), &catalog))
    });
    g.bench_function("unused assets", |b| {
        b.iter(|| unused_asset::run(black_box(&packs), &catalog))
    });
    g.bench_function("unused definitions", |b| {
        b.iter(|| unused_definition::run(black_box(&packs), &catalog))
    });
    g.bench_function("identical files", |b| {
        b.iter(|| duplicate_resource::run(black_box(&packs)))
    });
    g.finish();
}

criterion_group!(benches, bench_json, bench_catalog, bench_world);
criterion_main!(benches);
