//! bench-inflate: Zig's DEFLATE decoder on the same input as kebitx's Rust
//! one, to decide with a measurement whether a native decoder is worth it.
//!
//!   zig build --release=fast
//!   zig-out/bin/bench-inflate <corpus.bin>
//!
//! The corpus comes from `cargo test --release -p kebitx-java --test profile
//! write_inflate_corpus -- --ignored` (every class of the game's `.jar`, raw).
const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{ .preferred_optimize_mode = .ReleaseFast });

    const exe = b.addExecutable(.{
        .name = "bench-inflate",
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
    });
    b.installArtifact(exe);
}
