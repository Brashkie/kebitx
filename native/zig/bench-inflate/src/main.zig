//! Inflates every stream of a corpus with Zig's `std.compress.flate`,
//! checks each size and CRC-32 as kebitx does, and reports the speed.
//! Corpus: `count`, then per stream `packed_len`, `size`, `crc32`
//! (u32 little-endian) and the raw DEFLATE bytes.
const std = @import("std");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const alloc = gpa.allocator();

    const args = try std.process.argsAlloc(alloc);
    defer std.process.argsFree(alloc, args);
    if (args.len < 2) {
        std.debug.print("usage: bench-inflate <corpus.bin> [rounds]\n", .{});
        return error.Usage;
    }
    const rounds: usize = if (args.len > 2) try std.fmt.parseInt(usize, args[2], 10) else 3;
    const corpus = try std.fs.cwd().readFileAlloc(alloc, args[1], 1 << 30);
    defer alloc.free(corpus);

    const count = std.mem.readInt(u32, corpus[0..4], .little);
    var out = try alloc.alloc(u8, 16 << 20);
    defer alloc.free(out);

    var best: u64 = std.math.maxInt(u64);
    var total: usize = 0;
    for (0..rounds) |_| {
        var timer = try std.time.Timer.start();
        var at: usize = 4;
        total = 0;
        for (0..count) |_| {
            const plen = std.mem.readInt(u32, corpus[at..][0..4], .little);
            const size = std.mem.readInt(u32, corpus[at + 4 ..][0..4], .little);
            const crc = std.mem.readInt(u32, corpus[at + 8 ..][0..4], .little);
            const packed_bytes = corpus[at + 12 .. at + 12 + plen];
            if (size > out.len) return error.TooLarge;

            var in_stream = std.io.fixedBufferStream(packed_bytes);
            var out_stream = std.io.fixedBufferStream(out[0..size]);
            try std.compress.flate.decompress(in_stream.reader(), out_stream.writer());
            if (out_stream.pos != size) return error.SizeMismatch;
            if (std.hash.Crc32.hash(out[0..size]) != crc) return error.CrcMismatch;

            total += size;
            at += 12 + plen;
        }
        best = @min(best, timer.read());
    }
    const secs = @as(f64, @floatFromInt(best)) / 1e9;
    std.debug.print(
        "zig std.compress.flate + CRC: {d} streams, {d:.1} MB out, best of {d}: {d:.1} ms ({d:.0} MB/s)\n",
        .{ count, @as(f64, @floatFromInt(total)) / 1e6, rounds, secs * 1e3, @as(f64, @floatFromInt(total)) / 1e6 / secs },
    );
}
