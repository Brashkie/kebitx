//! kebitx-native: the C ABI of kebitx's native kernels. One static library,
//! built by Zig for the target Cargo builds for: the kernels written in Zig
//! (here) and libdeflate (C, compiled by Zig's C compiler, called directly
//! from Rust). Rust's `kebitx-native` crate is the only caller.
const unfilter = @import("unfilter.zig");

/// PNG unfiltering. Returns 0, 1 for a filter byte above 4, 2 for sizes that
/// do not match (`raw_len != (stride + 1) * height`, `out_len != stride * height`).
export fn kebitx_native_unfilter(
    raw: [*]const u8,
    raw_len: usize,
    out: [*]u8,
    out_len: usize,
    stride: usize,
    height: usize,
    bpp: usize,
) c_int {
    unfilter.unfilter(raw[0..raw_len], out[0..out_len], stride, height, bpp) catch |e| return switch (e) {
        error.BadFilter => 1,
        error.BadSize => 2,
    };
    return 0;
}

/// The ABI version: Rust checks it at start-up.
export fn kebitx_native_abi() c_uint {
    return 1;
}

test {
    _ = unfilter;
}
