//! kebitx-check — analyse a Minecraft Bedrock pack or world through kebitx's C
//! ABI (crates/kebitx-ffi/include/kebitx.h), from Zig.
//!
//! The library is loaded at run time (std.DynLib), so nothing depends on how
//! Rust and Zig would link against each other's runtime: the C ABI is the
//! whole contract. Exit code: 0 clean, 1 problems found, 2 usage or load error.
const std = @import("std");

const VersionFn = *const fn () callconv(.C) [*:0]const u8;
const AnalyzeFn = *const fn ([*:0]const u8) callconv(.C) ?[*:0]u8;
const FreeFn = *const fn (?[*:0]u8) callconv(.C) void;

/// A JSON number as an integer: kebitx writes counts as integers, but a
/// reader should not break if one ever comes as 3.0.
fn count(v: ?std.json.Value) i64 {
    const value = v orelse return 0;
    return switch (value) {
        .integer => |i| i,
        .float => |f| @intFromFloat(f),
        else => 0,
    };
}

fn text(v: ?std.json.Value) []const u8 {
    const value = v orelse return "";
    return switch (value) {
        .string => |s| s,
        else => "",
    };
}

pub fn main() !u8 {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();
    const out = std.io.getStdOut().writer();
    const err = std.io.getStdErr().writer();

    const args = try std.process.argsAlloc(alloc);
    defer std.process.argsFree(alloc, args);
    if (args.len != 3) {
        try err.print("usage: kebitx-check <kebitx_ffi library> <pack or world folder>\n", .{});
        return 2;
    }

    var lib = std.DynLib.open(args[1]) catch |e| {
        try err.print("cannot load {s}: {s}\n", .{ args[1], @errorName(e) });
        return 2;
    };
    defer lib.close();
    const version = lib.lookup(VersionFn, "kebitx_version") orelse return missing(err, "kebitx_version");
    const analyze = lib.lookup(AnalyzeFn, "kebitx_analyze") orelse return missing(err, "kebitx_analyze");
    const free = lib.lookup(FreeFn, "kebitx_string_free") orelse return missing(err, "kebitx_string_free");

    const path = try alloc.dupeZ(u8, args[2]);
    defer alloc.free(path);
    const report = analyze(path) orelse {
        try err.print("kebitx could not analyse {s} (not a folder?)\n", .{args[2]});
        return 2;
    };
    defer free(report);

    const parsed = try std.json.parseFromSlice(std.json.Value, alloc, std.mem.span(report), .{});
    defer parsed.deinit();
    const root = parsed.value.object;

    try out.print("kebitx-check (Zig) — engine {s}, through the C ABI\n", .{std.mem.span(version())});
    try out.print("  {s}: {s}, {d} pack(s), {d} entities\n", .{
        text(root.get("path")), text(root.get("mode")), count(root.get("packs")), count(root.get("entities")),
    });
    const broken = count(root.get("broken"));
    const case = count(root.get("case_mismatches"));
    const compat = count(root.get("compat_errors"));
    try out.print("  broken {d}, case mismatches {d}, compatibility errors {d}, unused assets {d}\n", .{
        broken, case, compat, count(root.get("unused_assets")),
    });

    // the issues the files alone decide, at most ten
    var shown: usize = 0;
    if (root.get("issues")) |issues| {
        for (issues.array.items) |issue| {
            const o = issue.object;
            if (!std.mem.eql(u8, text(o.get("confidence")), "high")) continue;
            if (shown == 10) {
                try out.print("  ...\n", .{});
                break;
            }
            try out.print("  {s:<10} {s} -> {s}\n", .{ text(o.get("kind")), text(o.get("owner")), text(o.get("reference")) });
            shown += 1;
        }
    }
    return if (broken + case + compat > 0) 1 else 0;
}

fn missing(err: anytype, name: []const u8) !u8 {
    try err.print("the library has no {s}: not a kebitx_ffi build?\n", .{name});
    return 2;
}

test "counts read integers and floats, and survive anything else" {
    try std.testing.expectEqual(@as(i64, 4), count(.{ .integer = 4 }));
    try std.testing.expectEqual(@as(i64, 3), count(.{ .float = 3.0 }));
    try std.testing.expectEqual(@as(i64, 0), count(.{ .string = "x" }));
    try std.testing.expectEqual(@as(i64, 0), count(null));
    try std.testing.expectEqualStrings("", text(.{ .integer = 1 }));
}
