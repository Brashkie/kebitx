/*
 * bench-inflate (C): zlib or libdeflate on the same input as kebitx's Rust
 * decoder and Zig's (native/zig/bench-inflate), with the same checks: every
 * stream inflated to its known size, and its CRC-32 compared.
 *
 * Corpus: `count`, then per stream `packed_len`, `size`, `crc32` (u32
 * little-endian) and the raw DEFLATE bytes; written by
 *   cargo test --release -p kebitx-java --test profile write_inflate_corpus -- --ignored
 *
 * Build (zig cc is a C compiler), from this folder, with the library's
 * sources somewhere else:
 *   zig cc -O3 -o bench-zlib bench.c -I<zlib> <zlib>/inflate.c <zlib>/inftrees.c \
 *       <zlib>/inffast.c <zlib>/zutil.c <zlib>/adler32.c <zlib>/crc32.c
 *   zig cc -O3 -DUSE_LIBDEFLATE -o bench-libdeflate bench.c -I<libdeflate> \
 *       <libdeflate>/lib/deflate_decompress.c <libdeflate>/lib/utils.c \
 *       <libdeflate>/lib/crc32.c <libdeflate>/lib/x86/cpu_features.c
 *   bench-zlib <corpus.bin> [rounds]
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* The library's sources are not in this repository: its header comes from
 * the -I given at build time. Without it (an editor's code model, or a build
 * without -I), this file still compiles, to a program that says how to
 * build it, instead of failing on a missing header. */
#ifdef USE_LIBDEFLATE
#define NAME "libdeflate"
#if __has_include("libdeflate.h")
#include "libdeflate.h"
#define HAVE_LIBRARY 1
#endif
#else
#define NAME "zlib"
#if __has_include("zlib.h")
#include "zlib.h"
#define HAVE_LIBRARY 1
#endif
#endif

#ifndef HAVE_LIBRARY
int main(void) {
    fprintf(stderr, "bench-inflate: built without %s's header; build it with -I<%s sources>, "
                    "as the comment at the top of bench.c shows\n",
            NAME, NAME);
    return 2;
}
#else

#ifdef _WIN32
#include <windows.h>
static double now(void) {
    LARGE_INTEGER f, c;
    QueryPerformanceFrequency(&f);
    QueryPerformanceCounter(&c);
    return (double)c.QuadPart / (double)f.QuadPart;
}
#else
#include <time.h>
static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}
#endif

static uint32_t le32(const unsigned char *p) {
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: %s <corpus.bin> [rounds]\n", argv[0]);
        return 2;
    }
    int rounds = argc > 2 ? atoi(argv[2]) : 5;
    FILE *f = fopen(argv[1], "rb");
    if (!f) {
        perror(argv[1]);
        return 2;
    }
    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    unsigned char *corpus = malloc((size_t)len);
    if (!corpus || fread(corpus, 1, (size_t)len, f) != (size_t)len) {
        fprintf(stderr, "cannot read the corpus\n");
        return 2;
    }
    fclose(f);
    uint32_t count = le32(corpus);
    size_t cap = 16u << 20;
    unsigned char *out = malloc(cap);

#ifdef USE_LIBDEFLATE
    struct libdeflate_decompressor *d = libdeflate_alloc_decompressor();
#else
    z_stream z;
    memset(&z, 0, sizeof z);
    if (inflateInit2(&z, -15) != Z_OK) return 1; /* raw DEFLATE */
#endif

    double best = 1e9;
    double total = 0;
    for (int r = 0; r < rounds; r++) {
        double t0 = now();
        size_t at = 4;
        total = 0;
        for (uint32_t i = 0; i < count; i++) {
            uint32_t plen = le32(corpus + at), size = le32(corpus + at + 4),
                     crc = le32(corpus + at + 8);
            const unsigned char *in = corpus + at + 12;
            if (size > cap) return 1;
#ifdef USE_LIBDEFLATE
            size_t got = 0;
            if (libdeflate_deflate_decompress(d, in, plen, out, size, &got) != LIBDEFLATE_SUCCESS ||
                got != size) {
                fprintf(stderr, "stream %u: decode error\n", i);
                return 1;
            }
            if (libdeflate_crc32(0, out, size) != crc) {
                fprintf(stderr, "stream %u: CRC mismatch\n", i);
                return 1;
            }
#else
            inflateReset(&z);
            z.next_in = (unsigned char *)in;
            z.avail_in = plen;
            z.next_out = out;
            z.avail_out = size;
            int rc = inflate(&z, Z_FINISH);
            if (rc != Z_STREAM_END || z.total_out != size) {
                fprintf(stderr, "stream %u: decode error %d\n", i, rc);
                return 1;
            }
            if (crc32(0L, out, size) != crc) {
                fprintf(stderr, "stream %u: CRC mismatch\n", i);
                return 1;
            }
#endif
            total += size;
            at += 12 + plen;
        }
        double t = now() - t0;
        if (t < best) best = t;
    }
    printf("%s + CRC: %u streams, %.1f MB out, best of %d: %.1f ms (%.0f MB/s)\n", NAME, count,
           total / 1e6, rounds, best * 1e3, total / 1e6 / best);
    return 0;
}
#endif /* HAVE_LIBRARY */
