//! Unfilters the corpus of `write_unfilter_corpus` (kebitx-migrate's profile
//! tests) with `unfilter.zig`, and prints the time and the same checksum the
//! Rust measurement prints (the sum of each image's CRC-32), so both can be
//! compared on identical input and output.
const std = @import("std");
const unfilter = @import("unfilter.zig").unfilter;

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();
    const args = try std.process.argsAlloc(alloc);
    defer std.process.argsFree(alloc, args);
    if (args.len < 2) {
        std.debug.print("usage: bench-unfilter <unfilter_corpus.bin>\n", .{});
        return error.Usage;
    }
    const corpus = try std.fs.cwd().readFileAlloc(alloc, args[1], 1 << 31);
    defer alloc.free(corpus);

    const Image = struct { stride: usize, h: usize, bpp: usize, raw: []const u8, out: []u8 };
    const n = std.mem.readInt(u32, corpus[0..4], .little);
    var images = try alloc.alloc(Image, n);
    defer alloc.free(images);
    var total: usize = 0;
    var at: usize = 4;
    for (0..n) |k| {
        const w = std.mem.readInt(u32, corpus[at..][0..4], .little);
        const h = std.mem.readInt(u32, corpus[at + 4 ..][0..4], .little);
        const bpp = std.mem.readInt(u32, corpus[at + 8 ..][0..4], .little);
        const stride = @as(usize, w) * bpp;
        const len = (stride + 1) * h;
        images[k] = .{ .stride = stride, .h = h, .bpp = bpp, .raw = corpus[at + 12 .. at + 12 + len], .out = undefined };
        total += stride * h;
        at += 12 + len;
    }
    const all = try alloc.alloc(u8, total);
    defer alloc.free(all);
    var off: usize = 0;
    for (images) |*img| {
        img.out = all[off .. off + img.stride * img.h];
        off += img.stride * img.h;
    }

    var best: u64 = std.math.maxInt(u64);
    for (0..5) |_| {
        var timer = try std.time.Timer.start();
        for (images) |img| try unfilter(img.raw, img.out, img.stride, img.h, img.bpp);
        best = @min(best, timer.read());
    }
    var sum: u64 = 0;
    for (images) |img| sum +%= std.hash.Crc32.hash(img.out);
    const secs = @as(f64, @floatFromInt(best)) / 1e9;
    std.debug.print("Zig unfilter: {d} images, {d:.1} MB, best of 5: {d:.1} ms ({d:.0} MB/s), checksum {x}\n", .{
        n, @as(f64, @floatFromInt(total)) / 1e6, secs * 1e3, @as(f64, @floatFromInt(total)) / 1e6 / secs, sum,
    });
}
