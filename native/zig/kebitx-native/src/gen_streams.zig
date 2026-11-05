//! Writes `fixtures/deflate/streams.bin`: DEFLATE and zlib streams made by a
//! compressor other than kebitx's (Zig's own, which uses dynamic Huffman
//! codes, as real files do), from data generated here — so the differential
//! tests of `kebitx-core` have real-world-shaped input on machines without
//! the games, CI included. Deterministic: the same file every time.
//!
//!   zig build gen-streams -- ../../../fixtures/deflate/streams.bin
//!
//! Format: `count`, then per stream: `zlib` (u8: 0 raw DEFLATE, 1 zlib),
//! `size`, the CRC-32 of the original data, `packed_len` (u32 little-endian),
//! the packed bytes.
const std = @import("std");

const words = [_][]const u8{
    "minecraft", "block",   "texture", "geometry", "entity", "the",  "of",   "and", "render",
    "controller", "animation", "zombie", "bedrock", "java",   "pack", "world", "a",  "to",
    "\"textures\": {", "}\n", "\"minecraft:", "\": [", "1.21.0", "\n  ",
};

fn data(kind: u8, size: usize, rand: std.Random, out: []u8) void {
    switch (kind) {
        // text: words with repeats, as JSON and .lang files are
        0 => {
            var i: usize = 0;
            while (i < size) {
                const w = words[rand.uintLessThan(usize, words.len)];
                for (w) |c| {
                    if (i >= size) break;
                    out[i] = c;
                    i += 1;
                }
                if (i < size) {
                    out[i] = ' ';
                    i += 1;
                }
            }
        },
        // random bytes: nothing to compress (stored blocks)
        1 => rand.bytes(out[0..size]),
        // image rows: a filter byte, then a gradient with noise
        2 => {
            const stride: usize = 65;
            for (0..size) |i| {
                const x = i % stride;
                out[i] = if (x == 0) @intCast(i / stride % 5) else @truncate(x * 3 + i / stride + rand.uintLessThan(u8, 4));
            }
        },
        // long runs, as empty atlases and transparent areas are
        else => {
            var i: usize = 0;
            while (i < size) {
                const run = 1 + rand.uintLessThan(usize, 300);
                const b = rand.int(u8);
                var k: usize = 0;
                while (k < run and i < size) : (k += 1) {
                    out[i] = b;
                    i += 1;
                }
            }
        },
    }
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();
    const args = try std.process.argsAlloc(alloc);
    defer std.process.argsFree(alloc, args);
    if (args.len < 2) return error.Usage;

    var prng = std.Random.DefaultPrng.init(0x6b65626974780a);
    const rand = prng.random();
    const buf = try alloc.alloc(u8, 400_000);
    defer alloc.free(buf);

    var file = std.ArrayList(u8).init(alloc);
    defer file.deinit();
    try file.appendNTimes(0, 4);
    var count: u32 = 0;
    const sizes = [_]usize{ 0, 1, 7, 100, 1000, 9_000, 100_000 };
    for ([_]u8{ 0, 1, 2, 3 }) |kind| {
        for (sizes) |size| {
            // random data does not compress: keep it small
            if (kind == 1 and size > 9_000) continue;
            for ([_]std.compress.flate.deflate.Level{ .fast, .default, .best }) |level| {
                // the large streams (several blocks) once per kind and container
                if (size > 9_000 and level != .default) continue;
                for ([_]u8{ 0, 1 }) |zlib| {
                    data(kind, size, rand, buf);
                    var packed_bytes = std.ArrayList(u8).init(alloc);
                    defer packed_bytes.deinit();
                    var in = std.io.fixedBufferStream(buf[0..size]);
                    if (zlib == 1) {
                        try std.compress.zlib.compress(in.reader(), packed_bytes.writer(), .{ .level = level });
                    } else {
                        try std.compress.flate.compress(in.reader(), packed_bytes.writer(), .{ .level = level });
                    }
                    try file.append(zlib);
                    try file.writer().writeInt(u32, @intCast(size), .little);
                    try file.writer().writeInt(u32, std.hash.Crc32.hash(buf[0..size]), .little);
                    try file.writer().writeInt(u32, @intCast(packed_bytes.items.len), .little);
                    try file.appendSlice(packed_bytes.items);
                    count += 1;
                }
            }
        }
    }
    std.mem.writeInt(u32, file.items[0..4], count, .little);
    try std.fs.cwd().writeFile(.{ .sub_path = args[1], .data = file.items });
    std.debug.print("{d} streams, {d} bytes\n", .{ count, file.items.len });
}
