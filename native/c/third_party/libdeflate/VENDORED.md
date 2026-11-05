# libdeflate (vendored subset)

- **Upstream:** https://github.com/ebiggers/libdeflate
- **Version:** 1.26 (`libdeflate-1.26.tar.gz`)
- **sha256 of the release archive:**
  `125856d4656e0feab660f94842f835923410c9281fedbcee64598c918da42b5a`
- **License:** MIT (see `COPYING`).
- **Files:** unmodified copies of the decompression side only: DEFLATE, zlib, Adler-32, and the
  CPU feature detection for x86 and ARM. No compressor, no gzip, no CRC-32 (kebitx has its own).
- **Built by:** `native/zig/kebitx-native/build.zig` (with Zig's C compiler), only when kebitx is
  built with `--features native`.

**Why it is here.** It was measured on the 11 383 class files of Java 26.3 with CRC checks:
libdeflate decoded at 597 MB/s, against 187 MB/s for kebitx's Rust decoder. Every output it
produces in kebitx is also checked against the Rust decoder by differential tests
(`crates/kebitx-native/tests/`).

**To update.** Download a new release and check its published sha256. Copy the same files, and
update the version and sha256 above. Then run `cargo test -p kebitx-native --features native`.
