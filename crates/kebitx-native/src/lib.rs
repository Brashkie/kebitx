//! kebitx-native — optional native kernels, each one measured against the
//! Rust it replaces and kept only where it is faster:
//!
//!   - DEFLATE and zlib decoding: libdeflate (C), 597 MB/s against kebitx's
//!     187 MB/s on the 11 383 class files of Java 26.3;
//!   - PNG unfiltering: Zig's portable vectors, 1 418 MB/s against 651 MB/s
//!     for kebitx's Rust on the 4 747 8-bit textures of Bedrock (stable Rust
//!     has no portable SIMD).
//!
//! Built only with the `native` feature (Zig 0.14 builds the C and the Zig
//! for the target). Without it every function answers `None` and the
//! callers use their Rust; with it they answer `Some`. Either way the bytes
//! are the same: `kebitx-core`'s differential tests compare every output
//! with the Rust decoder's, hostile inputs included.
//!
//! With `native`, this is the second crate of the workspace with `unsafe`
//! (after `kebitx-ffi`), only at the boundary below; every block says why it
//! holds.

#![cfg_attr(not(feature = "native"), forbid(unsafe_code))]
#![cfg_attr(feature = "native", deny(unsafe_op_in_unsafe_fn))]

/// Whether the native kernels are built in.
pub const AVAILABLE: bool = cfg!(feature = "native");

/// Why native decoding refused the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not valid DEFLATE / zlib data (or its checksum is wrong).
    Invalid,
    /// It decodes to more than the caller's limit.
    TooLarge,
}

#[cfg(feature = "native")]
mod sys {
    use std::ffi::{c_int, c_uint, c_void};

    pub const SUCCESS: c_int = 0;
    pub const INSUFFICIENT_SPACE: c_int = 3;

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
        pub fn libdeflate_alloc_decompressor() -> *mut c_void;
        pub fn libdeflate_free_decompressor(d: *mut c_void);
        pub fn libdeflate_deflate_decompress_ex(
            d: *mut c_void,
            input: *const c_void,
            in_nbytes: usize,
            out: *mut c_void,
            out_nbytes_avail: usize,
            actual_in_nbytes_ret: *mut usize,
            actual_out_nbytes_ret: *mut usize,
        ) -> c_int;
        pub fn libdeflate_zlib_decompress_ex(
            d: *mut c_void,
            input: *const c_void,
            in_nbytes: usize,
            out: *mut c_void,
            out_nbytes_avail: usize,
            actual_in_nbytes_ret: *mut usize,
            actual_out_nbytes_ret: *mut usize,
        ) -> c_int;
    }

    /// One libdeflate decompressor per thread: it holds tables only, no
    /// state between calls.
    pub struct Decompressor(pub *mut c_void);

    impl Drop for Decompressor {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: allocated by libdeflate_alloc_decompressor, freed once.
                unsafe { libdeflate_free_decompressor(self.0) }
            }
        }
    }

    thread_local! {
        pub static DECOMPRESSOR: Decompressor = Decompressor(
            // SAFETY: no arguments; NULL (out of memory) is checked by every user.
            unsafe { libdeflate_alloc_decompressor() },
        );
    }

    /// The library this crate was built with is the one it was written for.
    pub fn abi_ok() -> bool {
        // SAFETY: no arguments, no state.
        unsafe { kebitx_native_abi() == 1 }
    }
}

/// Raw DEFLATE -> bytes, at most `limit`; with the number of input bytes the
/// stream used. `None`: not built with `native`.
pub fn inflate(data: &[u8], limit: usize) -> Option<Result<(Vec<u8>, usize), Error>> {
    #[cfg(feature = "native")]
    {
        decompress(data, limit, false)
    }
    #[cfg(not(feature = "native"))]
    {
        let _ = (data, limit);
        None
    }
}

/// A zlib stream (header, DEFLATE, Adler-32 checked) -> bytes, at most
/// `limit`. `None`: not built with `native`.
pub fn zlib_decompress(data: &[u8], limit: usize) -> Option<Result<Vec<u8>, Error>> {
    #[cfg(feature = "native")]
    {
        decompress(data, limit, true).map(|r| r.map(|(out, _)| out))
    }
    #[cfg(not(feature = "native"))]
    {
        let _ = (data, limit);
        None
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

#[cfg(feature = "native")]
fn decompress(data: &[u8], limit: usize, zlib: bool) -> Option<Result<(Vec<u8>, usize), Error>> {
    if !sys::abi_ok() {
        return None;
    }
    sys::DECOMPRESSOR.with(|d| {
        if d.0.is_null() {
            return None;
        }
        // the output's size is not known: start from a typical ratio, grow
        // up to the limit (a hostile stream cannot make it allocate more)
        let mut cap = limit.min(data.len().saturating_mul(4).max(4096));
        loop {
            let mut out: Vec<u8> = Vec::with_capacity(cap);
            let (mut used, mut written) = (0usize, 0usize);
            let f = if zlib {
                sys::libdeflate_zlib_decompress_ex
            } else {
                sys::libdeflate_deflate_decompress_ex
            };
            // SAFETY: `d` is a live decompressor of this thread; the input is
            // a live slice; the output is `cap` bytes of reserved capacity,
            // of which libdeflate writes at most `cap` and reports how many.
            let rc = unsafe {
                f(
                    d.0,
                    data.as_ptr().cast(),
                    data.len(),
                    out.as_mut_ptr().cast(),
                    cap,
                    &mut used,
                    &mut written,
                )
            };
            match rc {
                sys::SUCCESS => {
                    // SAFETY: libdeflate initialised exactly `written <= cap` bytes.
                    unsafe { out.set_len(written.min(cap)) };
                    return Some(Ok((out, used)));
                }
                sys::INSUFFICIENT_SPACE if cap < limit => {
                    cap = limit.min(cap.saturating_mul(4));
                }
                sys::INSUFFICIENT_SPACE => return Some(Err(Error::TooLarge)),
                _ => return Some(Err(Error::Invalid)),
            }
        }
    })
}
