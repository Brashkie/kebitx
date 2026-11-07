//! kebitx-native: the optional native kernel, kebitx's own code, measured
//! against the Rust it replaces and kept only where it is faster: PNG
//! unfiltering in Zig's portable vectors, 1 418 MB/s against 651 MB/s for
//! kebitx's Rust on the 4 747 8-bit textures of Bedrock (stable Rust has no
//! portable SIMD).
//!
//! Built only with the `native` feature (Zig 0.14 builds it for the target).
//! Without it `unfilter` answers `None` and the caller uses its Rust. Either
//! way the bytes are the same: `kebitx-core`'s tests compare them.
//!
//! With `native`, this is the second crate of the workspace with `unsafe`
//! (after `kebitx-ffi`), only at the boundary below; every block says why it
//! holds.

#![cfg_attr(not(feature = "native"), forbid(unsafe_code))]
#![cfg_attr(feature = "native", deny(unsafe_op_in_unsafe_fn))]

/// Whether the native kernel is built in.
pub const AVAILABLE: bool = cfg!(feature = "native");

#[cfg(feature = "native")]
mod sys {
    use std::ffi::{c_int, c_uint};

    extern "C" {
        pub fn kebitx_native_abi() -> c_uint;
        pub fn kebitx_native_unfilter(
            raw: *const u8,
            raw_len: usize,
            out: *mut u8,
            out_len: usize,
            stride: usize,
            height: usize,
            bpp: usize,
        ) -> c_int;
    }

    /// The library this crate was built with is the one it was written for.
    pub fn abi_ok() -> bool {
        // SAFETY: no arguments, no state.
        unsafe { kebitx_native_abi() == 3 }
    }
}

/// PNG rows without their filters (`raw`: `height` rows of a filter byte and
/// `stride` bytes; `bpp`: bytes per pixel). `None`: not built with `native`.
pub fn unfilter(
    raw: &[u8],
    stride: usize,
    height: usize,
    bpp: usize,
) -> Option<Result<Vec<u8>, &'static str>> {
    #[cfg(feature = "native")]
    {
        if !sys::abi_ok() {
            return None;
        }
        let len = stride.checked_mul(height)?;
        if raw.len() != stride.checked_add(1)?.checked_mul(height)? {
            return Some(Err("PNG data of the wrong size"));
        }
        let mut out = vec![0u8; len];
        // SAFETY: both pointers come from live slices with the lengths given;
        // the kernel reads `raw` and writes `out` within those lengths only.
        let rc = unsafe {
            sys::kebitx_native_unfilter(
                raw.as_ptr(),
                raw.len(),
                out.as_mut_ptr(),
                out.len(),
                stride,
                height,
                bpp,
            )
        };
        Some(match rc {
            0 => Ok(out),
            1 => Err("unknown PNG filter"),
            _ => Err("PNG data of the wrong size"),
        })
    }
    #[cfg(not(feature = "native"))]
    {
        let _ = (raw, stride, height, bpp);
        None
    }
}
