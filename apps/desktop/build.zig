//! kebitx's desktop app: C on each system's own APIs, no toolkit, no third-party
//! library. Zig is only the C compiler and the build script here.
//!
//!   zig build                # zig-out/bin/kebitx-app(.exe): Windows or Linux
//!   zig build test           # the report reader, on any system
//!
//! The engine is kebitx_ffi.dll or libkebitx_ffi.so, built by Cargo, and goes next
//! to the executable. On Linux nothing else is linked but libc: the window speaks
//! the X11 protocol itself, so it also builds from any other system:
//!
//!   zig build -Dtarget=x86_64-linux-gnu.2.31
const std = @import("std");

// `= {sizeof(x)}` is how Win32 structures are started: the rest is zero
const flags = [_][]const u8{ "-std=c11", "-Wall", "-Wextra", "-Werror", "-Wno-missing-field-initializers" };

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{ .preferred_optimize_mode = .ReleaseSafe });

    const tests = b.addExecutable(.{
        .name = "report-test",
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });
    tests.addCSourceFiles(.{ .files = &.{ "src/report.c", "src/report_test.c" }, .flags = &flags });
    b.step("test", "Run the report reader's tests").dependOn(&b.addRunArtifact(tests).step);

    if (target.result.os.tag == .linux) {
        const app = b.addExecutable(.{
            .name = "kebitx-app",
            .target = target,
            .optimize = optimize,
            .link_libc = true,
        });
        app.addCSourceFiles(.{ .files = &.{ "src/report.c", "src/x11.c" }, .flags = &flags });
        // inside libc since glibc 2.34; apart before it
        app.linkSystemLibrary("dl");
        app.linkSystemLibrary("pthread");
        b.installArtifact(app);
        return;
    }
    if (target.result.os.tag != .windows) return;

    const exe = b.addExecutable(.{
        .name = "kebitx-app",
        .target = target,
        .optimize = optimize,
        .link_libc = true,
        .win32_manifest = b.path("src/app.manifest"),
    });
    exe.subsystem = .Windows;
    exe.addWin32ResourceFile(.{ .file = b.path("src/app.rc") });
    exe.addCSourceFiles(.{ .files = &.{ "src/report.c", "src/win32.c" }, .flags = &flags });
    for ([_][]const u8{ "user32", "gdi32", "comctl32", "comdlg32", "shell32", "shlwapi", "ole32", "advapi32", "dwmapi", "uxtheme" }) |lib| {
        exe.linkSystemLibrary(lib);
    }
    b.installArtifact(exe);
}
