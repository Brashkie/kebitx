//! PNG unfiltering (PNG 9.2–9.4) with Zig's portable vectors: for images of
//! 3 or 4 bytes per pixel, a whole pixel per vector operation (the method of
//! libpng's SSE filters), Paeth in 16-bit lanes without branches. Other
//! pixel sizes take a plain loop. Stable Rust has no portable SIMD; this is
//! what Zig brings to kebitx here, and the output is checked byte for byte
//! against kebitx's Rust decoder.
const std = @import("std");

pub const Error = error{ BadFilter, BadSize };

/// `raw`: `height` rows of a filter byte and `stride` bytes; `out`:
/// `stride * height` bytes. `bpp`: bytes per pixel, at least 1.
pub fn unfilter(raw: []const u8, out: []u8, stride: usize, height: usize, bpp_in: usize) Error!void {
    if (raw.len != (stride + 1) * height or out.len != stride * height) return error.BadSize;
    if (stride == 0) return;
    const bpp = @max(@min(bpp_in, stride), 1);
    var y: usize = 0;
    while (y < height) : (y += 1) {
        const filter = raw[y * (stride + 1)];
        const src = raw[y * (stride + 1) + 1 .. (y + 1) * (stride + 1)];
        const cur = out[y * stride .. (y + 1) * stride];
        const prior: ?[]const u8 = if (y == 0) null else out[(y - 1) * stride .. y * stride];
        if (bpp == 4 and stride % 4 == 0) {
            try rowVector(4, filter, src, prior, cur);
        } else if (bpp == 3 and stride % 3 == 0) {
            try rowVector(3, filter, src, prior, cur);
        } else {
            try rowScalar(filter, src, prior, cur, bpp);
        }
    }
}

fn rowVector(comptime N: usize, filter: u8, src: []const u8, prior_opt: ?[]const u8, cur: []u8) Error!void {
    const V = @Vector(N, u8);
    const W = @Vector(N, i16);
    const n = cur.len;
    switch (filter) {
        0 => @memcpy(cur, src),
        1 => {
            var left: V = @splat(0);
            var i: usize = 0;
            while (i < n) : (i += N) {
                const s: V = src[i..][0..N].*;
                left = s +% left;
                cur[i..][0..N].* = left;
            }
        },
        2 => {
            const prior = prior_opt orelse {
                @memcpy(cur, src);
                return;
            };
            upRow(src, prior, cur);
        },
        3 => {
            var left: V = @splat(0);
            var i: usize = 0;
            while (i < n) : (i += N) {
                const s: V = src[i..][0..N].*;
                const b: V = if (prior_opt) |p| p[i..][0..N].* else @splat(0);
                const sum = @as(@Vector(N, u16), left) + @as(@Vector(N, u16), b);
                const avg: V = @truncate(sum >> @splat(1));
                left = s +% avg;
                cur[i..][0..N].* = left;
            }
        },
        4 => {
            const prior = prior_opt orelse {
                // no row above: b = c = 0, so Paeth predicts a (Sub)
                return rowVector(N, 1, src, null, cur);
            };
            var a: V = @splat(0);
            var c: V = @splat(0);
            var i: usize = 0;
            while (i < n) : (i += N) {
                const s: V = src[i..][0..N].*;
                const b: V = prior[i..][0..N].*;
                const a16: W = a;
                const b16: W = b;
                const c16: W = c;
                const pa = @abs(b16 - c16);
                const pb = @abs(a16 - c16);
                const pc = @abs(a16 + b16 - c16 - c16);
                const no: @Vector(N, bool) = @splat(false);
                const use_a = @select(bool, pa <= pb, pa <= pc, no);
                const pred = @select(u8, use_a, a, @select(u8, pb <= pc, b, c));
                a = s +% pred;
                c = b;
                cur[i..][0..N].* = a;
            }
        },
        else => return error.BadFilter,
    }
}

/// Up: no dependency along the row, so 32 bytes at a time.
fn upRow(src: []const u8, prior: []const u8, cur: []u8) void {
    const L = 32;
    const V = @Vector(L, u8);
    var i: usize = 0;
    while (i + L <= cur.len) : (i += L) {
        const s: V = src[i..][0..L].*;
        const p: V = prior[i..][0..L].*;
        cur[i..][0..L].* = s +% p;
    }
    while (i < cur.len) : (i += 1) cur[i] = src[i] +% prior[i];
}

fn rowScalar(filter: u8, src: []const u8, prior_opt: ?[]const u8, cur: []u8, bpp: usize) Error!void {
    const n = cur.len;
    var i: usize = 0;
    while (i < n) : (i += 1) {
        const a: i16 = if (i >= bpp) cur[i - bpp] else 0;
        const b: i16 = if (prior_opt) |p| p[i] else 0;
        const c: i16 = if (i >= bpp) (if (prior_opt) |p| p[i - bpp] else 0) else 0;
        const pred: i16 = switch (filter) {
            0 => 0,
            1 => a,
            2 => b,
            3 => @divTrunc(a + b, 2),
            4 => blk: {
                const pa = @abs(b - c);
                const pb = @abs(a - c);
                const pc = @abs(a + b - c - c);
                break :blk if (pa <= pb and pa <= pc) a else if (pb <= pc) b else c;
            },
            else => return error.BadFilter,
        };
        cur[i] = src[i] +% @as(u8, @intCast(pred));
    }
}

test "every filter, on 3 and 4 bytes per pixel and odd sizes, matches the plain loop" {
    var prng = std.Random.DefaultPrng.init(0x6b6562);
    const rand = prng.random();
    for ([_]usize{ 1, 2, 3, 4, 6, 8 }) |bpp| {
        for ([_]usize{ 1, 2, 5, 17 }) |w| {
            const stride = w * bpp;
            const h = 6;
            var raw: [(8 * 17 + 1) * 6]u8 = undefined;
            var fast: [8 * 17 * 6]u8 = undefined;
            var slow: [8 * 17 * 6]u8 = undefined;
            const r = raw[0 .. (stride + 1) * h];
            rand.bytes(r);
            for (0..h) |y| r[y * (stride + 1)] = @intCast(y % 5);
            try unfilter(r, fast[0 .. stride * h], stride, h, bpp);
            for (0..h) |y| {
                const prior: ?[]const u8 = if (y == 0) null else slow[(y - 1) * stride .. y * stride];
                try rowScalar(r[y * (stride + 1)], r[y * (stride + 1) + 1 .. (y + 1) * (stride + 1)], prior, slow[y * stride .. (y + 1) * stride], bpp);
            }
            try std.testing.expectEqualSlices(u8, slow[0 .. stride * h], fast[0 .. stride * h]);
        }
    }
}

test "a filter byte above 4 is an error" {
    var raw = [_]u8{ 5, 1, 2, 3, 4 };
    var out: [4]u8 = undefined;
    try std.testing.expectError(error.BadFilter, unfilter(&raw, &out, 4, 1, 4));
}
