//! kebitx-check: the reference consumer of kebitx's C ABI, in Zig.
//!
//!   zig build                      # ReleaseSafe by default: bounds and overflow checks stay on
//!   zig build run -- <kebitx_ffi.dll> <pack or world folder>
const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{ .preferred_optimize_mode = .ReleaseSafe });

    const exe = b.addExecutable(.{
        .name = "kebitx-check",
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
    });
    b.installArtifact(exe);

    const run = b.addRunArtifact(exe);
    if (b.args) |args| run.addArgs(args);
    b.step("run", "Analyse a pack or world through kebitx's C ABI").dependOn(&run.step);

    const tests = b.addTest(.{ .root_source_file = b.path("src/main.zig"), .target = target, .optimize = optimize });
    b.step("test", "Run the unit tests").dependOn(&b.addRunArtifact(tests).step);
}
