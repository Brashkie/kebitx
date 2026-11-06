//! kebitx-native: the native parts of kebitx, built with Zig.
//!
//!   zig build                               # libkebitx_native (static): Zig kernels + libdeflate (C)
//!   zig build test                          # unit tests (each kernel against its plain version)
//!   zig build bench-unfilter --release=fast -- <unfilter_corpus.bin>
//!   zig build bench-inflate --release=fast -- <corpus.bin>   # Zig std on the same input as every decoder
//!
//! Cargo builds the library through `crates/kebitx-native/build.rs` when
//! kebitx is built with `--features native`, for the target Cargo builds for.
//! Every kernel here is measured against kebitx's Rust on the same input and
//! must give the same bytes; it is kept only where it is faster.
const std = @import("std");

/// libdeflate's decompression side (vendored, unmodified: native/c/third_party/libdeflate).
const libdeflate_sources = [_][]const u8{
    "lib/deflate_decompress.c",
    "lib/zlib_decompress.c",
    "lib/adler32.c",
    "lib/utils.c",
    "lib/x86/cpu_features.c",
    "lib/arm/cpu_features.c",
};

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{ .preferred_optimize_mode = .ReleaseFast });

    const lib = b.addStaticLibrary(.{
        .name = "kebitx_native",
        .root_source_file = b.path("src/kebitx_native.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    lib.bundle_compiler_rt = true;
    // Rust links static-pie on musl, so every object must be position independent
    lib.root_module.pic = true;
    const libdeflate = b.path("../../c/third_party/libdeflate");
    lib.addIncludePath(libdeflate);
    var flags = std.ArrayList([]const u8).init(b.allocator);
    flags.appendSlice(&.{ "-std=c99", "-DNDEBUG", "-fno-sanitize=undefined" }) catch @panic("OOM");
    // libdeflate's AVX-512 Adler-32 does not compile with the clang Zig ships
    // ("requires target feature evex512"); libdeflate's own switch leaves it
    // out, and the AVX2 path, picked at run time, stays
    flags.append("-DLIBDEFLATE_ASSEMBLER_DOES_NOT_SUPPORT_AVX512VNNI") catch @panic("OOM");
    // libdeflate's ARM CPU detection on Linux uses O_CLOEXEC, which glibc's headers hide
    // under a strict -std=c99
    flags.append("-D_GNU_SOURCE") catch @panic("OOM");
    lib.addCSourceFiles(.{ .root = libdeflate, .files = &libdeflate_sources, .flags = flags.items });
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

    const bench_inflate = b.addExecutable(.{
        .name = "bench-inflate",
        .root_source_file = b.path("src/bench_inflate.zig"),
        .target = target,
        .optimize = optimize,
    });
    const run_inflate = b.addRunArtifact(bench_inflate);
    if (b.args) |args| run_inflate.addArgs(args);
    b.step("bench-inflate", "Inflate a corpus with Zig's decoder and time it").dependOn(&run_inflate.step);

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
