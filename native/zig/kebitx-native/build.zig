//! kebitx-native: the native parts of kebitx, built with Zig.
//!
//!   zig build                               # libkebitx_native (static)
//!   zig build test                          # unit tests (each kernel against its plain version)
//!   zig build bench-unfilter --release=fast -- <unfilter_corpus.bin>
//!
//! Cargo builds the library through `crates/kebitx-native/build.rs` when
//! kebitx is built with `--features native`, for the target Cargo builds for.
//! Every kernel here is measured against kebitx's Rust on the same input and
//! must give the same bytes; it is kept only where it is faster.
const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{ .preferred_optimize_mode = .ReleaseFast });

    const lib = b.addStaticLibrary(.{
        .name = "kebitx_native",
        .root_source_file = b.path("src/kebitx_native.zig"),
        .target = target,
        .optimize = optimize,
    });
    lib.bundle_compiler_rt = true;
    // Rust links static-pie on musl, so every object must be position independent
    lib.root_module.pic = true;
    b.installArtifact(lib);

    const tests = b.addTest(.{
        .root_source_file = b.path("src/kebitx_native.zig"),
        .target = target,
        .optimize = optimize,
    });
    b.step("test", "Run the unit tests").dependOn(&b.addRunArtifact(tests).step);

    const bench = b.addExecutable(.{
        .name = "bench-unfilter",
        .root_source_file = b.path("src/bench_unfilter.zig"),
        .target = target,
        .optimize = optimize,
    });
    const run = b.addRunArtifact(bench);
    if (b.args) |args| run.addArgs(args);
    b.step("bench-unfilter", "Unfilter a corpus and time it").dependOn(&run.step);

    const gen = b.addExecutable(.{
        .name = "gen-streams",
        .root_source_file = b.path("src/gen_streams.zig"),
        .target = b.graph.host,
        .optimize = .ReleaseSafe,
    });
    const run_gen = b.addRunArtifact(gen);
    if (b.args) |args| run_gen.addArgs(args);
    b.step("gen-streams", "Write the DEFLATE/zlib fixture for the differential tests").dependOn(&run_gen.step);
}
