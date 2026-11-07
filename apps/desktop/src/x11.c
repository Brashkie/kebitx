/* kebitx's desktop app on Linux, in C with no library at all but libc: it speaks
 * the X11 protocol over the server's socket, so neither libX11 nor its headers are
 * needed to build it or to run it. It works under Wayland too, through XWayland.
 * The engine is libkebitx_ffi.so, next to the executable; the window shows what
 * it says, and decides nothing itself.
 *
 *   kebitx-app [path]
 *
 * For tests, with no one at the screen (under Xvfb, for one):
 *
 *   kebitx-app [path] --steps sp --capture out.bmp
 *
 * as on Windows: see win32.c. It draws into a pixmap and copies that to the window,
 * so a capture is what the window shows, and needs no window manager.
 *
 * Text comes from the server's own fonts, which every X server has. There is no
 * system file dialog to ask for on X11, so the window has its own folder browser.
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <poll.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#include "report.h"

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;

/* ---------- the engine's calls ---------- */

typedef int (*ProgressFn)(const char *, unsigned, unsigned, void *);
typedef char *(*ReportFn)(const char *, ProgressFn, void *);
typedef char *(*PlanFn)(const char *);
typedef char *(*ApplyFn)(const char *, const char *);
typedef char *(*UndoFn)(const char *);
typedef char *(*PlacesFn)(void);
typedef void (*FreeFn)(char *);

typedef enum { JOB_REPORT, JOB_PLAN, JOB_APPLY, JOB_UNDO, JOB_PLACES } JobKind;

typedef struct {
    JobKind kind;
    char *path;
    char *fingerprint;
} Job;

/* What the engine's thread tells the window, through a pipe. */
typedef struct {
    int done;      /* 0: progress, 1: the job's end */
    JobKind kind;
    char *text;    /* the stage, or the JSON */
    unsigned step, total;
} Note;

/* ---------- what the window shows ---------- */

typedef enum { MODE_MAIN, MODE_BROWSE, MODE_ASK } Mode;
typedef enum { LIST_NONE, LIST_HOME, LIST_REPORT } ListKind;
typedef enum { ASK_APPLY, ASK_UNDO } Asking;
typedef enum { FOCUS_LIST, FOCUS_PATH } Focus;

enum {
    B_HOME,
    B_ANALYSE,
    B_FOLDER,
    B_FILE,
    B_PREVIEW,
    B_APPLY,
    B_UNDO,
    B_CANCEL,
    B_USE,
    B_BACK,
    B_YES,
    B_NO,
    BUTTON_COUNT
};

typedef struct {
    int x, y, w, h;
} Rect;

typedef struct {
    char *what, *name, *path;
} Place;

typedef struct {
    char *name;
    int is_dir;
} Entry;

#define COLOR_BACK 0xf0f0f0u
#define COLOR_FIELD 0xffffffu
#define COLOR_TEXT 0x1a1a1au
#define COLOR_DIM 0x8a8a8au
#define COLOR_LINE 0xa8a8a8u
#define COLOR_BUTTON 0xfafafau
#define COLOR_FOCUS 0x0067c0u
#define COLOR_SELECT 0x0067c0u
#define COLOR_ON_SELECT 0xffffffu
#define COLOR_THUMB 0xb0b0b0u

#define MAX_PATH_TEXT 4096

static struct {
    /* the connection */
    int fd;
    u8 out[65536];
    size_t out_len;
    u16 seq;
    u32 next_id;
    u32 id_base, id_mask;
    u32 root, visual;
    u8 depth;
    u8 min_keycode, max_keycode;
    u8 screen_mm_wide;
    u32 *keysyms;
    int syms_per_code, code_count;
    u8 queue[256][32];
    unsigned queue_head, queue_len;

    u32 window, pixmap, gc, font;
    int font16;
    int cw, ch, ascent;
    int w, h;
    u32 pixmap_w, pixmap_h;
    u32 atom_protocols, atom_delete, atom_utf8, atom_net_name, atom_clipboard, atom_paste;

    /* the engine */
    struct {
        ReportFn report;
        PlanFn plan;
        ApplyFn apply;
        UndoFn undo;
        PlacesFn places;
        FreeFn string_free;
    } engine;
    int notes[2]; /* the pipe from the engine's thread */
    int busy;
    JobKind running;
    volatile int cancel;

    /* what is open */
    char *path;
    char *fingerprint;
    char *backup;

    /* the screen */
    Mode mode;
    Asking asking;
    Focus focus;
    char edit[MAX_PATH_TEXT];
    size_t caret;
    char *header, *summary, *detail;
    int dirty;
    Rect buttons[BUTTON_COUNT];
    int visible[BUTTON_COUNT];
    Rect path_rect, list_rect, detail_rect;

    /* the list */
    ListKind list;
    int has_report;
    Report current;
    const char *row_label;
    Place *places;
    size_t place_count;
    char *shortcuts[2];
    Entry *entries;
    size_t entry_count;
    char browse_dir[MAX_PATH_TEXT];
    int browse_files;
    long selected, top;
    u32 last_click_time;
    long last_click_row;

    const char *capture_to;
    const char *steps;
    int exit_code;
    int running_loop;
} app;

/* ---------- text helpers ---------- */

static char *copy_text(const char *s) {
    if (!s) return NULL;
    size_t n = strlen(s) + 1;
    char *out = malloc(n);
    if (out) memcpy(out, s, n);
    return out;
}

static void replace_text(char **slot, const char *s) {
    char *copy = copy_text(s);
    free(*slot);
    *slot = copy;
}

/* The next code point of UTF-8 text, as a font can take it. */
static u32 next_char(const char **text) {
    const u8 *p = (const u8 *)*text;
    u32 c = p[0];
    int more = c < 0x80 ? 0 : c < 0xC0 ? -1 : c < 0xE0 ? 1 : c < 0xF0 ? 2 : c < 0xF8 ? 3 : -1;
    if (more < 0) {
        *text += 1;
        return '?';
    }
    if (more > 0) c &= 0x3Fu >> more;
    for (int i = 1; i <= more; i++) {
        if ((p[i] & 0xC0) != 0x80) {
            *text += (size_t)i;
            return '?';
        }
        c = (c << 6) | (p[i] & 0x3Fu);
    }
    *text += (size_t)more + 1;
    return c > 0xFFFF ? '?' : c;
}

static size_t char_count(const char *text, size_t bytes) {
    const char *p = text, *end = text + bytes;
    size_t n = 0;
    while (p < end && *p) {
        next_char(&p);
        n++;
    }
    return n;
}

/* ---------- the connection ---------- */

static void flush_out(void) {
    size_t at = 0;
    while (at < app.out_len) {
        ssize_t n = write(app.fd, app.out + at, app.out_len - at);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) {
            fprintf(stderr, "kebitx-app: the X server went away\n");
            exit(4);
        }
        at += (size_t)n;
    }
    app.out_len = 0;
}

static void put(const void *data, size_t n) {
    if (n > sizeof(app.out) - app.out_len) flush_out();
    /* nothing here is larger than the buffer: text goes in short pieces */
    memcpy(app.out + app.out_len, data, n);
    app.out_len += n;
}

static void put8(u8 v) { put(&v, 1); }
static void put16(u16 v) { put(&v, 2); }
static void put32(u32 v) { put(&v, 4); }
static void put_i16(int v) { put16((u16)(int16_t)v); }

static void pad4(size_t n) {
    static const u8 zero[4] = {0};
    put(zero, (4 - (n % 4)) % 4);
}

/* Every request starts so; `words` is its whole length in 4-byte units. */
static void request(u8 opcode, u8 data, u16 words) {
    put8(opcode);
    put8(data);
    put16(words);
    app.seq++;
}

static void read_all(void *to, size_t n) {
    u8 *p = to;
    while (n) {
        ssize_t got = read(app.fd, p, n);
        if (got < 0 && errno == EINTR) continue;
        if (got <= 0) {
            fprintf(stderr, "kebitx-app: the X server went away\n");
            exit(4);
        }
        p += got;
        n -= (size_t)got;
    }
}

static u16 get16(const u8 *p) {
    u16 v;
    memcpy(&v, p, 2);
    return v;
}

static u32 get32(const u8 *p) {
    u32 v;
    memcpy(&v, p, 4);
    return v;
}

static void say_error(const u8 *e) {
    fprintf(stderr, "kebitx-app: X error %u on request %u (sequence %u)\n", (unsigned)e[1], (unsigned)e[10],
            (unsigned)get16(e + 2));
}

/* The answer to the last request: its first 32 bytes, and what follows them in new
 * memory (to free). Events that arrive first wait in the queue. NULL extra and
 * return 0 when the server refused it. */
static int wait_reply(u8 head[32], u8 **extra, size_t *extra_len) {
    flush_out();
    if (extra) *extra = NULL;
    if (extra_len) *extra_len = 0;
    for (;;) {
        u8 m[32];
        read_all(m, 32);
        if (m[0] == 1) {
            size_t more = (size_t)get32(m + 4) * 4;
            u8 *body = more ? malloc(more) : NULL;
            if (more && !body) exit(4);
            if (more) read_all(body, more);
            if (get16(m + 2) != app.seq) {
                free(body); /* an answer nobody waits for */
                continue;
            }
            memcpy(head, m, 32);
            if (extra) {
                *extra = body;
                if (extra_len) *extra_len = more;
            } else {
                free(body);
            }
            return 1;
        }
        if (m[0] == 0) {
            if (get16(m + 2) == app.seq) return 0;
            /* a font this server does not have: the next one is tried */
            if (m[1] != 15) say_error(m);
            continue;
        }
        if (app.queue_len < 256) {
            memcpy(app.queue[(app.queue_head + app.queue_len) % 256], m, 32);
            app.queue_len++;
        }
    }
}

/* ---------- connecting ---------- */

static int open_socket(const char *display, int *number) {
    const char *colon = display ? strrchr(display, ':') : NULL;
    if (!colon) return -1;
    *number = atoi(colon + 1);
    size_t host_len = (size_t)(colon - display);
    if (host_len == 0 || (host_len == 4 && memcmp(display, "unix", 4) == 0)) {
        struct sockaddr_un addr;
        memset(&addr, 0, sizeof(addr));
        addr.sun_family = AF_UNIX;
        snprintf(addr.sun_path, sizeof(addr.sun_path), "/tmp/.X11-unix/X%d", *number);
        int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
        if (fd >= 0 && connect(fd, (struct sockaddr *)&addr, sizeof(addr)) == 0) return fd;
        if (fd >= 0) close(fd);
        return -1;
    }
    char host[256], port[16];
    if (host_len >= sizeof(host)) return -1;
    memcpy(host, display, host_len);
    host[host_len] = '\0';
    snprintf(port, sizeof(port), "%d", 6000 + *number);
    struct addrinfo hints, *found = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo(host, port, &hints, &found) != 0) return -1;
    int fd = -1;
    for (struct addrinfo *a = found; a; a = a->ai_next) {
        fd = socket(a->ai_family, a->ai_socktype | SOCK_CLOEXEC, a->ai_protocol);
        if (fd >= 0 && connect(fd, a->ai_addr, a->ai_addrlen) == 0) break;
        if (fd >= 0) close(fd);
        fd = -1;
    }
    freeaddrinfo(found);
    return fd;
}

/* The MIT-MAGIC-COOKIE-1 for this display in ~/.Xauthority, if there is one. Fields
 * there are big-endian: a family, then address, number, name and data, each with
 * its length. An entry with no number fits any display. */
static size_t find_cookie(int number, u8 cookie[16]) {
    char path[MAX_PATH_TEXT];
    const char *file = getenv("XAUTHORITY");
    const char *home = getenv("HOME");
    if (file) snprintf(path, sizeof(path), "%s", file);
    else if (home) snprintf(path, sizeof(path), "%s/.Xauthority", home);
    else return 0;
    FILE *f = fopen(path, "rb");
    if (!f) return 0;
    char wanted[16];
    snprintf(wanted, sizeof(wanted), "%d", number);
    size_t found = 0;
    for (;;) {
        u8 family[2];
        if (fread(family, 1, 2, f) != 2) break;
        char fields[4][256];
        size_t lens[4];
        int ok = 1;
        for (int i = 0; i < 4 && ok; i++) {
            u8 len[2];
            if (fread(len, 1, 2, f) != 2) {
                ok = 0;
                break;
            }
            lens[i] = ((size_t)len[0] << 8) | len[1];
            size_t keep = lens[i] < 256 ? lens[i] : 256;
            if (fread(fields[i], 1, keep, f) != keep) ok = 0;
            if (lens[i] > keep && fseek(f, (long)(lens[i] - keep), SEEK_CUR) != 0) ok = 0;
        }
        if (!ok) break;
        int fits = lens[1] == 0 || (lens[1] == strlen(wanted) && memcmp(fields[1], wanted, lens[1]) == 0);
        if (fits && lens[2] == 18 && memcmp(fields[2], "MIT-MAGIC-COOKIE-1", 18) == 0 && lens[3] == 16) {
            memcpy(cookie, fields[3], 16);
            found = 16;
            if (lens[1] != 0) break; /* one for this very display: the best there is */
        }
    }
    fclose(f);
    return found;
}

static int connect_server(void) {
    int number = 0;
    app.fd = open_socket(getenv("DISPLAY"), &number);
    if (app.fd < 0) return -1;

    u8 cookie[16];
    size_t cookie_len = find_cookie(number, cookie);
    static const char auth_name[] = "MIT-MAGIC-COOKIE-1";
    size_t name_len = cookie_len ? 18 : 0;
    put8('l'); /* this client's byte order, little-endian: the server follows it */
    put8(0);
    put16(11);
    put16(0);
    put16((u16)name_len);
    put16((u16)cookie_len);
    put16(0);
    put(auth_name, name_len);
    pad4(name_len);
    put(cookie, cookie_len);
    pad4(cookie_len);
    flush_out();

    u8 head[8];
    read_all(head, 8);
    size_t more = (size_t)get16(head + 6) * 4;
    u8 *body = malloc(more ? more : 1);
    if (!body) return -1;
    read_all(body, more);
    if (head[0] != 1) {
        fprintf(stderr, "kebitx-app: the X server refused the connection: %.*s\n", (int)head[1], (char *)body);
        free(body);
        return -1;
    }
    if (more < 32) {
        free(body);
        return -1;
    }
    app.id_base = get32(body + 4);
    app.id_mask = get32(body + 8);
    u16 vendor_len = get16(body + 16);
    u8 format_count = body[21];
    app.min_keycode = body[26];
    app.max_keycode = body[27];
    size_t screen_at = 32 + (((size_t)vendor_len + 3) & ~(size_t)3) + (size_t)format_count * 8;
    if (screen_at + 40 > more || body[20] == 0) {
        free(body);
        return -1;
    }
    const u8 *screen = body + screen_at;
    app.root = get32(screen);
    u16 width_px = get16(screen + 20);
    u16 width_mm = get16(screen + 24);
    app.visual = get32(screen + 32);
    app.depth = screen[38];
    /* pixels of 0xRRGGBB, as every desktop today */
    if (app.depth != 24 && app.depth != 32) {
        fprintf(stderr, "kebitx-app: a screen of depth %u is not supported\n", (unsigned)app.depth);
        free(body);
        return -1;
    }
    /* how large the text should be: dots per inch, or so */
    app.screen_mm_wide = width_mm && (long)width_px * 254 / ((long)width_mm * 10) >= 140;
    free(body);
    return 0;
}

static u32 new_id(void) {
    /* ids are the base with bits only where the mask allows: count in those */
    u32 mask = app.id_mask;
    u32 low = mask & (~mask + 1);
    u32 id = app.id_base | ((++app.next_id * low) & mask);
    return id;
}

static u32 intern(const char *name) {
    size_t n = strlen(name);
    request(16, 0, (u16)(2 + (n + 3) / 4));
    put16((u16)n);
    put16(0);
    put(name, n);
    pad4(n);
    u8 reply[32];
    return wait_reply(reply, NULL, NULL) ? get32(reply + 8) : 0;
}

/* Opens the first font in the list the server has. */
static int open_font(void) {
    static const char *large[] = {"-misc-fixed-medium-r-normal--18-120-100-100-c-90-iso10646-1", "10x20", "9x15"};
    static const char *normal[] = {"-misc-fixed-medium-r-normal--13-120-75-75-c-70-iso10646-1", "7x13", "fixed"};
    const char **names = app.screen_mm_wide ? large : normal;
    for (int round = 0; round < 2; round++) {
        for (int i = 0; i < 3; i++) {
            const char *name = names[i];
            size_t n = strlen(name);
            u32 id = new_id();
            request(45, 0, (u16)(3 + (n + 3) / 4)); /* OpenFont */
            put32(id);
            put16((u16)n);
            put16(0);
            put(name, n);
            pad4(n);
            request(47, 0, 2); /* QueryFont: refused if the font was not there */
            put32(id);
            u8 head[32];
            u8 *rest = NULL;
            size_t rest_len = 0;
            if (!wait_reply(head, &rest, &rest_len)) continue;
            /* QueryFont's reply: the widest character's width at 28, in the first
               32 bytes; then, from 32 on, max-byte1 at 50 and ascent and descent
               at 52 and 54 */
            if (rest_len < 24) {
                free(rest);
                continue;
            }
            app.font = id;
            app.cw = (int)(int16_t)get16(head + 28);
            app.font16 = rest[50 - 32] > 0;
            app.ascent = (int)(int16_t)get16(rest + 52 - 32);
            app.ch = app.ascent + (int)(int16_t)get16(rest + 54 - 32);
            if (app.cw <= 0 || app.ch <= 0) {
                free(rest);
                continue;
            }
            free(rest);
            return 0;
        }
        names = names == large ? normal : large;
    }
    return -1;
}

/* ---------- drawing ---------- */

static void set_colors(u32 fg, u32 bg) {
    request(56, 0, 5); /* ChangeGC: foreground, background */
    put32(app.gc);
    put32((1u << 2) | (1u << 3));
    put32(fg);
    put32(bg);
}

static void fill(Rect r, u32 color) {
    if (r.w <= 0 || r.h <= 0) return;
    set_colors(color, color);
    request(70, 0, 5); /* PolyFillRectangle */
    put32(app.pixmap);
    put32(app.gc);
    put_i16(r.x);
    put_i16(r.y);
    put16((u16)r.w);
    put16((u16)r.h);
}

static void outline(Rect r, u32 color) {
    fill((Rect){r.x, r.y, r.w, 1}, color);
    fill((Rect){r.x, r.y + r.h - 1, r.w, 1}, color);
    fill((Rect){r.x, r.y, 1, r.h}, color);
    fill((Rect){r.x + r.w - 1, r.y, 1, r.h}, color);
}

/* One line of text, cut at `cols` characters. */
static void text_at(int x, int y, const char *text, size_t bytes, int cols, u32 fg, u32 bg) {
    if (cols <= 0 || !text) return;
    if (cols > 255) cols = 255;
    u16 chars[255];
    int n = 0;
    const char *p = text, *end = text + bytes;
    while (p < end && *p && n < cols) {
        u32 c = next_char(&p);
        if (c < 0x20) c = ' ';
        if (!app.font16 && c > 0xFF) c = '?';
        chars[n++] = (u16)c;
    }
    if (n == 0) return;
    set_colors(fg, bg);
    int baseline = y + app.ascent;
    if (app.font16) {
        request(77, (u8)n, (u16)(4 + ((size_t)n * 2 + 3) / 4)); /* ImageText16 */
        put32(app.pixmap);
        put32(app.gc);
        put_i16(x);
        put_i16(baseline);
        for (int i = 0; i < n; i++) {
            /* a character of two bytes is high byte first, whatever the byte order */
            put8((u8)(chars[i] >> 8));
            put8((u8)chars[i]);
        }
        pad4((size_t)n * 2);
    } else {
        request(76, (u8)n, (u16)(4 + ((size_t)n + 3) / 4)); /* ImageText8 */
        put32(app.pixmap);
        put32(app.gc);
        put_i16(x);
        put_i16(baseline);
        for (int i = 0; i < n; i++) put8((u8)chars[i]);
        pad4((size_t)n);
    }
}

static void line_at(int x, int y, const char *text, int cols, u32 fg, u32 bg) {
    if (text) text_at(x, y, text, strlen(text), cols, fg, bg);
}

/* Text in the box, wrapped at spaces where it can be, as far as `lines` go. */
static void wrapped(Rect r, const char *text, int lines, u32 fg, u32 bg) {
    if (!text) return;
    int cols = r.w / app.cw;
    if (cols <= 0) return;
    const char *p = text;
    for (int i = 0; i < lines && *p; i++) {
        const char *q = p, *space = NULL;
        int n = 0;
        while (*q && *q != '\n' && n < cols) {
            const char *at = q;
            next_char(&q);
            if (*at == ' ') space = at;
            n++;
        }
        const char *end = *q && *q != '\n' && space ? space : q;
        text_at(r.x, r.y + i * app.ch, p, (size_t)(end - p), cols, fg, bg);
        p = end;
        if (*p == ' ' || *p == '\n') p++;
    }
}

static const char *button_label(int b) {
    static const char *labels[BUTTON_COUNT] = {
        "Home",           "Analyse",        "Open folder...", "Open file...", "Preview repair", "Apply repair...",
        "Undo repair...", "Cancel",         "Use this folder", "Back",        "Yes",            "No",
    };
    return labels[b];
}

static int button_enabled(int b) {
    int ready = app.engine.report != NULL && !app.busy;
    switch (b) {
    case B_HOME:
    case B_FOLDER:
    case B_FILE: return ready;
    case B_ANALYSE: return ready && app.edit[0] != '\0';
    case B_PREVIEW: return ready && app.path != NULL;
    case B_APPLY: return ready && app.fingerprint != NULL;
    case B_UNDO: return ready && app.backup != NULL;
    /* an analysis can stop; a repair half done must not */
    case B_CANCEL: return app.busy && app.running == JOB_REPORT;
    default: return 1;
    }
}

static int button_width(int b) { return (int)strlen(button_label(b)) * app.cw + app.cw * 2; }

static void layout(void) {
    int gap = app.ch / 2 + 2;
    int button_h = app.ch + 10;
    for (int i = 0; i < BUTTON_COUNT; i++) app.visible[i] = 0;

    /* where: home, the path, and the ways to choose one */
    int y = gap;
    int browsing = app.mode == MODE_BROWSE;
    int right = app.w - gap;
    if (browsing) {
        app.path_rect = (Rect){0, 0, 0, 0};
    } else {
        for (int b = B_FILE; b >= B_ANALYSE; b--) {
            int w = button_width(b);
            right -= w;
            app.buttons[b] = (Rect){right, y, w, button_h};
            app.visible[b] = 1;
            right -= gap / 2;
        }
        int home_w = button_width(B_HOME);
        app.buttons[B_HOME] = (Rect){gap, y, home_w, button_h};
        app.visible[B_HOME] = 1;
        int px = gap + home_w + gap / 2;
        app.path_rect = (Rect){px, y, right - px, button_h};
        y += button_h + gap;
    }

    /* what it is: the header, and two lines of summary */
    y += app.ch * 3 + gap;

    /* what is in it, and what to do about it, at the end of the reading */
    int bottom = app.h - gap - button_h;
    int detail_h = app.ch * 4 + 8;
    int list_h = bottom - gap - detail_h - gap - y;
    if (list_h < app.ch * 3) list_h = app.ch * 3;
    app.list_rect = (Rect){gap, y, app.w - gap * 2, list_h};
    app.detail_rect = (Rect){gap, y + list_h + gap, app.w - gap * 2, detail_h};

    int first = B_PREVIEW, last = B_UNDO;
    if (browsing) {
        first = app.browse_files ? B_BACK : B_USE;
        last = B_BACK;
    } else if (app.mode == MODE_ASK) {
        first = B_YES;
        last = B_NO;
    }
    int x = gap;
    for (int b = first; b <= last; b++) {
        int w = button_width(b);
        app.buttons[b] = (Rect){x, bottom, w, button_h};
        app.visible[b] = 1;
        x += w + gap / 2;
    }
    if (app.mode == MODE_MAIN) {
        int w = button_width(B_CANCEL);
        app.buttons[B_CANCEL] = (Rect){app.w - gap - w, bottom, w, button_h};
        app.visible[B_CANCEL] = 1;
    }
}

static long row_count(void) {
    if (app.mode == MODE_BROWSE) return (long)app.entry_count;
    if (app.list == LIST_HOME) return (long)app.place_count;
    if (app.list == LIST_REPORT && app.has_report) return (long)app.current.row_count;
    return 0;
}

static int row_height(void) { return app.ch + 2; }

static long rows_shown(void) {
    long n = (app.list_rect.h - row_height() - 2) / row_height();
    return n > 0 ? n : 1;
}

static void keep_selected_shown(void) {
    if (app.selected < 0) return;
    if (app.selected < app.top) app.top = app.selected;
    if (app.selected >= app.top + rows_shown()) app.top = app.selected - rows_shown() + 1;
}

static void clamp_top(void) {
    long most = row_count() - rows_shown();
    if (app.top > most) app.top = most;
    if (app.top < 0) app.top = 0;
}

/* The second and third columns of a row. */
static void row_cells(long i, const char **kind, const char **name, const char **where) {
    *kind = *name = *where = "";
    if (app.mode == MODE_BROWSE) {
        *kind = app.entries[i].is_dir ? "Folder" : "File";
        *name = app.entries[i].name;
    } else if (app.list == LIST_HOME) {
        *kind = app.places[i].what;
        *name = app.places[i].name;
        *where = app.places[i].path;
    } else {
        *kind = app.current.rows[i].is_problem ? app.row_label : app.current.rows[i].is_dead ? "Does nothing" : "Note";
        *name = app.current.rows[i].text;
    }
}

static void draw_list(void) {
    Rect r = app.list_rect;
    fill(r, COLOR_FIELD);
    int kind_cols = 19;
    int kind_w = kind_cols * app.cw;
    int inner = r.w - 4 - app.cw * 2; /* room for the scroll bar */
    int has_where = app.mode == MODE_MAIN && app.list == LIST_HOME;
    int name_w = has_where ? (inner - kind_w) * 2 / 5 : inner - kind_w;
    int y = r.y + 2;

    fill((Rect){r.x + 1, y - 1, r.w - 2, row_height()}, COLOR_BACK);
    line_at(r.x + 4, y, "Type", kind_cols, COLOR_TEXT, COLOR_BACK);
    line_at(r.x + 4 + kind_w, y, has_where ? "Name" : app.mode == MODE_BROWSE ? "Name" : "Finding", name_w / app.cw,
            COLOR_TEXT, COLOR_BACK);
    if (has_where) line_at(r.x + 4 + kind_w + name_w, y, "Folder", 10, COLOR_TEXT, COLOR_BACK);
    y += row_height();

    long total = row_count();
    long shown = rows_shown();
    for (long i = app.top; i < total && i < app.top + shown; i++) {
        const char *kind, *name, *where;
        row_cells(i, &kind, &name, &where);
        int chosen = i == app.selected;
        u32 back = chosen ? COLOR_SELECT : COLOR_FIELD;
        u32 fore = chosen ? COLOR_ON_SELECT : COLOR_TEXT;
        if (chosen) fill((Rect){r.x + 1, y, r.w - 2 - app.cw * 2, row_height()}, back);
        line_at(r.x + 4, y + 1, kind, kind_cols - 1, fore, back);
        line_at(r.x + 4 + kind_w, y + 1, name, name_w / app.cw - 1, fore, back);
        if (has_where) line_at(r.x + 4 + kind_w + name_w, y + 1, where, (inner - kind_w - name_w) / app.cw, fore, back);
        y += row_height();
    }

    /* the scroll bar */
    if (total > shown) {
        Rect track = {r.x + r.w - app.cw * 2 - 1, r.y + 1, app.cw * 2, r.h - 2};
        fill(track, COLOR_BACK);
        int thumb_h = (int)((long)track.h * shown / total);
        if (thumb_h < app.ch) thumb_h = app.ch;
        int thumb_y = track.y + (int)((long)(track.h - thumb_h) * app.top / (total - shown));
        fill((Rect){track.x + 2, thumb_y, track.w - 4, thumb_h}, COLOR_THUMB);
    }
    outline(r, app.focus == FOCUS_LIST ? COLOR_FOCUS : COLOR_LINE);
}

static void draw_buttons(void) {
    for (int b = 0; b < BUTTON_COUNT; b++) {
        if (!app.visible[b]) continue;
        Rect r = app.buttons[b];
        int on = button_enabled(b);
        fill(r, COLOR_BUTTON);
        outline(r, COLOR_LINE);
        line_at(r.x + app.cw, r.y + (r.h - app.ch) / 2, button_label(b), 255, on ? COLOR_TEXT : COLOR_DIM, COLOR_BUTTON);
    }
}

static void draw_path_box(void) {
    Rect r = app.path_rect;
    if (r.w <= 0) return;
    fill(r, COLOR_FIELD);
    outline(r, app.focus == FOCUS_PATH ? COLOR_FOCUS : COLOR_LINE);
    int cols = (r.w - 8) / app.cw;
    int y = r.y + (r.h - app.ch) / 2;
    if (app.edit[0] == '\0' && app.focus != FOCUS_PATH) {
        line_at(r.x + 4, y, "Type or paste the path of a pack, a world, a mod or a mods folder", cols, COLOR_DIM,
                COLOR_FIELD);
        return;
    }
    /* the end of a long path is what tells it from others, and where the caret is */
    size_t before = char_count(app.edit, app.caret);
    size_t skip = before >= (size_t)cols ? before - (size_t)cols + 1 : 0;
    const char *p = app.edit;
    for (size_t i = 0; i < skip; i++) next_char(&p);
    line_at(r.x + 4, y, p, cols, COLOR_TEXT, COLOR_FIELD);
    if (app.focus == FOCUS_PATH)
        fill((Rect){r.x + 4 + (int)(before - skip) * app.cw, y, 1, app.ch}, COLOR_TEXT);
}

static void redraw(void) {
    if (app.w <= 0 || app.h <= 0) return;
    if ((u32)app.w != app.pixmap_w || (u32)app.h != app.pixmap_h) {
        if (app.pixmap) {
            request(54, 0, 2); /* FreePixmap */
            put32(app.pixmap);
        }
        app.pixmap = new_id();
        request(53, app.depth, 4); /* CreatePixmap */
        put32(app.pixmap);
        put32(app.window);
        put16((u16)app.w);
        put16((u16)app.h);
        app.pixmap_w = (u32)app.w;
        app.pixmap_h = (u32)app.h;
    }
    layout();
    clamp_top();
    fill((Rect){0, 0, app.w, app.h}, COLOR_BACK);
    draw_buttons();
    draw_path_box();

    int gap = app.ch / 2 + 2;
    int y = app.path_rect.w > 0 ? app.path_rect.y + app.path_rect.h + gap : gap;
    int cols = (app.w - gap * 2) / app.cw;
    /* the same line twice, a pixel apart: the bold the fixed font does not have */
    line_at(gap, y, app.header, cols, COLOR_TEXT, COLOR_BACK);
    line_at(gap + 1, y, app.header, cols, COLOR_TEXT, COLOR_BACK);
    wrapped((Rect){gap, y + app.ch + 2, app.w - gap * 2, app.ch * 2}, app.summary, 2, COLOR_TEXT, COLOR_BACK);

    draw_list();
    fill(app.detail_rect, COLOR_FIELD);
    outline(app.detail_rect, COLOR_LINE);
    Rect inside = {app.detail_rect.x + 4, app.detail_rect.y + 4, app.detail_rect.w - 8, app.detail_rect.h - 8};
    wrapped(inside, app.detail, 4, COLOR_TEXT, COLOR_FIELD);

    request(62, 0, 7); /* CopyArea: the finished picture, at once */
    put32(app.pixmap);
    put32(app.window);
    put32(app.gc);
    put_i16(0);
    put_i16(0);
    put_i16(0);
    put_i16(0);
    put16((u16)app.w);
    put16((u16)app.h);
    app.dirty = 0;
}

/* The picture as a 32-bit top-down BMP. One colour everywhere means nothing was
 * drawn: that is a failure. */
static int save_capture(const char *path) {
    redraw();
    request(73, 2, 5); /* GetImage, as pixels */
    put32(app.pixmap);
    put_i16(0);
    put_i16(0);
    put16((u16)app.w);
    put16((u16)app.h);
    put32(0xFFFFFFFFu);
    u8 head[32];
    u8 *pixels = NULL;
    size_t size = 0;
    if (!wait_reply(head, &pixels, &size)) return -1;
    size_t need = (size_t)app.w * (size_t)app.h * 4;
    int ok = -1;
    if (pixels && size >= need) {
        int drawn = 0;
        for (size_t i = 4; i + 3 < need && !drawn; i += 4) {
            drawn = (get32(pixels + i) & 0xFFFFFFu) != (get32(pixels) & 0xFFFFFFu);
        }
        FILE *f = drawn ? fopen(path, "wb") : NULL;
        if (f) {
            u8 file[14] = {'B', 'M'};
            u8 info[40] = {40};
            u32 total = (u32)(14 + 40 + need), off = 14 + 40, w = (u32)app.w;
            u32 h = (u32)(-app.h);
            u16 planes = 1, bits = 32;
            memcpy(file + 2, &total, 4);
            memcpy(file + 10, &off, 4);
            memcpy(info + 4, &w, 4);
            memcpy(info + 8, &h, 4);
            memcpy(info + 12, &planes, 2);
            memcpy(info + 14, &bits, 2);
            if (fwrite(file, 14, 1, f) == 1 && fwrite(info, 40, 1, f) == 1 && fwrite(pixels, need, 1, f) == 1) ok = 0;
            fclose(f);
        }
    }
    free(pixels);
    return ok;
}

/* ---------- the engine, on its own thread ---------- */

static void post_note(Note note) {
    Note *copy = malloc(sizeof(Note));
    if (!copy) return;
    *copy = note;
    ssize_t n;
    do {
        n = write(app.notes[1], &copy, sizeof(copy));
    } while (n < 0 && errno == EINTR);
}

static int on_progress(const char *stage, unsigned done, unsigned total, void *user) {
    (void)user;
    post_note((Note){0, JOB_REPORT, copy_text(stage), done, total});
    return __atomic_load_n(&app.cancel, __ATOMIC_SEQ_CST);
}

static void *run_job(void *arg) {
    Job *job = arg;
    char *json = NULL;
    switch (job->kind) {
    case JOB_REPORT: json = app.engine.report(job->path, on_progress, NULL); break;
    case JOB_PLAN: json = app.engine.plan(job->path); break;
    case JOB_APPLY: json = app.engine.apply(job->path, job->fingerprint); break;
    case JOB_UNDO: json = app.engine.undo(job->path); break;
    case JOB_PLACES: json = app.engine.places(); break;
    }
    post_note((Note){1, job->kind, json, 0, 0});
    free(job->path);
    free(job->fingerprint);
    free(job);
    return NULL;
}

static void set_screen(const char *header, const char *summary) {
    replace_text(&app.header, header);
    replace_text(&app.summary, summary);
    app.dirty = 1;
}

static void forget_list(void) {
    if (app.has_report) {
        report_free(&app.current);
        app.has_report = 0;
    }
    app.list = LIST_NONE;
    app.selected = -1;
    app.top = 0;
    replace_text(&app.detail, NULL);
    app.dirty = 1;
}

static void start_job(JobKind kind, const char *path, const char *fingerprint, const char *saying) {
    if (app.busy || !path) return;
    Job *job = calloc(1, sizeof(Job));
    if (job) {
        job->kind = kind;
        job->path = copy_text(path);
        job->fingerprint = copy_text(fingerprint);
    }
    if (!job || !job->path || (fingerprint && !job->fingerprint)) {
        if (job) {
            free(job->path);
            free(job->fingerprint);
            free(job);
        }
        set_screen("Out of memory", "Nothing was changed.");
        return;
    }
    __atomic_store_n(&app.cancel, 0, __ATOMIC_SEQ_CST);
    forget_list();
    set_screen(saying, "");
    app.busy = 1;
    app.running = kind;
    pthread_t thread;
    if (pthread_create(&thread, NULL, run_job, job) == 0) {
        pthread_detach(thread);
    } else {
        free(job->path);
        free(job->fingerprint);
        free(job);
        app.busy = 0;
        set_screen("Could not start", "Nothing was changed.");
    }
}

/* ---------- what the engine answers ---------- */

static const Report *keep_report(const char *json, const char *row_label) {
    forget_list();
    Report r;
    if (report_parse(json, &r) != 0) return NULL;
    app.current = r;
    app.has_report = 1;
    app.list = LIST_REPORT;
    app.row_label = row_label;
    return &app.current;
}

static void show_analysis(const char *json) {
    const Report *r = keep_report(json, "Problem");
    if (!r) {
        set_screen("The engine could not read that", "Check the path: it must be a folder, or a .jar or .zip file.");
        replace_text(&app.path, NULL);
        return;
    }
    if (r->nothing) {
        set_screen(r->kind, r->summary);
        replace_text(&app.path, NULL);
        return;
    }
    if (r->cancelled) {
        set_screen("Analysis cancelled", "Nothing was changed.");
        return;
    }
    size_t dead = 0;
    for (size_t i = 0; i < r->row_count; i++) dead += r->rows[i].is_dead ? 1 : 0;
    char head[512];
    if (r->broken == 0) snprintf(head, sizeof(head), "%s: OK: nothing proven broken", r->kind);
    else if (r->broken == 1) snprintf(head, sizeof(head), "%s: 1 problem", r->kind);
    else snprintf(head, sizeof(head), "%s: %ld problems", r->kind, r->broken);
    if (dead) {
        size_t at = strlen(head);
        snprintf(head + at, sizeof(head) - at, ", and %zu that do nothing", dead);
    }
    set_screen(head, r->summary);
}

static void show_plan(const char *json) {
    const Report *r = keep_report(json, "Change");
    if (!r) {
        set_screen("The engine could not plan a repair", "Nothing was changed.");
        return;
    }
    if (r->row_count == 0) {
        set_screen("Nothing to repair", r->summary);
    } else {
        char head[96];
        snprintf(head, sizeof(head), "Repair preview: %zu change%s. Nothing has been written.", r->row_count,
                 r->row_count == 1 ? "" : "s");
        replace_text(&app.fingerprint, r->fingerprint);
        set_screen(head, r->summary);
    }
}

static void show_outcome(const char *json, const char *when_unreadable) {
    const Report *r = keep_report(json, "New problem");
    if (!r) {
        set_screen(when_unreadable, "Check the folder: the engine gave no answer.");
        return;
    }
    if (r->kept) replace_text(&app.backup, r->backup);
    set_screen(r->kind, r->summary);
}

static void free_places(void) {
    for (size_t i = 0; i < app.place_count; i++) {
        free(app.places[i].what);
        free(app.places[i].name);
        free(app.places[i].path);
    }
    free(app.places);
    app.places = NULL;
    app.place_count = 0;
    for (int i = 0; i < 2; i++) {
        free(app.shortcuts[i]);
        app.shortcuts[i] = NULL;
    }
}

/* The first screen: what is on this computer, ready to open. */
static void show_places(const char *json) {
    free_places();
    forget_list();
    Report r;
    if (report_parse(json, &r) == 0) {
        app.places = calloc(r.row_count ? r.row_count : 1, sizeof(Place));
        for (size_t i = 0; app.places && i < r.row_count; i++) {
            /* what\tname\tpath */
            char *what = r.rows[i].text;
            char *name = strchr(what, '\t');
            char *path = name ? strchr(name + 1, '\t') : NULL;
            if (!path) continue;
            *name++ = '\0';
            *path++ = '\0';
            if (strcmp(what, "Shortcut") == 0) {
                int slot = app.shortcuts[0] ? 1 : 0;
                if (!app.shortcuts[slot]) app.shortcuts[slot] = copy_text(path);
                continue;
            }
            Place place = {copy_text(what), copy_text(name), copy_text(path)};
            if (place.what && place.name && place.path) {
                app.places[app.place_count++] = place;
            } else {
                free(place.what);
                free(place.name);
                free(place.path);
            }
        }
        report_free(&r);
    }
    app.list = LIST_HOME;
    set_screen("Minecraft on this computer",
               app.place_count
                   ? "Double-click one to analyse it, or press Enter. For anything else: type or paste a path, or "
                     "choose a folder or a file."
                   : "Nothing to check in the game's usual places. Type or paste a path, or choose a folder or a file.");
}

/* ---------- actions ---------- */

static void set_edit(const char *text) {
    snprintf(app.edit, sizeof(app.edit), "%s", text ? text : "");
    app.caret = strlen(app.edit);
    app.dirty = 1;
}

static void analyse_again(void) {
    replace_text(&app.fingerprint, NULL);
    start_job(JOB_REPORT, app.path, NULL, "Analysing...");
}

static void open_path(const char *path) {
    if (app.busy) return;
    char *copy = copy_text(path);
    if (!copy) return;
    free(app.path);
    app.path = copy;
    set_edit(path);
    app.mode = MODE_MAIN;
    /* another folder's backup is not undone from here: its path stays on the
       report that gave it, for `kebitx repair --undo` */
    replace_text(&app.backup, NULL);
    analyse_again();
}

/* What is in the path box, without quotes and the spaces around them. The same
 * path again keeps its backup to undo. */
static void analyse_typed(void) {
    if (app.busy) return;
    char text[MAX_PATH_TEXT];
    snprintf(text, sizeof(text), "%s", app.edit);
    char *start = text;
    while (*start == ' ' || *start == '"' || *start == '\'') start++;
    size_t len = strlen(start);
    while (len && (start[len - 1] == ' ' || start[len - 1] == '"' || start[len - 1] == '\'')) start[--len] = '\0';
    if (!len) return;
    if (app.path && strcmp(start, app.path) == 0) {
        set_edit(start);
        analyse_again();
    } else {
        open_path(start);
    }
}

static void go_home(void) {
    if (app.busy) return;
    replace_text(&app.path, NULL);
    replace_text(&app.fingerprint, NULL);
    set_edit("");
    app.mode = MODE_MAIN;
    start_job(JOB_PLACES, "", NULL, "Looking for Minecraft...");
}

static void preview_repair(void) {
    replace_text(&app.fingerprint, NULL);
    start_job(JOB_PLAN, app.path, NULL, "Planning the repair...");
}

static void apply_repair(void) {
    if (!app.fingerprint) return;
    char *fingerprint = app.fingerprint;
    app.fingerprint = NULL;
    start_job(JOB_APPLY, app.path, fingerprint, "Repairing...");
    free(fingerprint);
}

static void undo_repair(void) {
    if (!app.backup) return;
    char *backup = app.backup;
    app.backup = NULL;
    replace_text(&app.fingerprint, NULL);
    start_job(JOB_UNDO, backup, NULL, "Undoing the repair...");
    free(backup);
}

/* The saved screen while a question or the browser is up, to put back. */
static char *saved_header, *saved_summary;
static Focus saved_focus;

static void save_screen(void) {
    replace_text(&saved_header, app.header);
    replace_text(&saved_summary, app.summary);
    saved_focus = app.focus;
}

static void restore_screen(void) {
    app.mode = MODE_MAIN;
    set_screen(saved_header, saved_summary);
    app.focus = saved_focus;
}

static void ask(Asking what) {
    if (what == ASK_APPLY ? !app.fingerprint : !app.backup) return;
    save_screen();
    app.mode = MODE_ASK;
    app.asking = what;
    if (what == ASK_APPLY) {
        set_screen("Apply the changes in the preview?",
                   "Every file is backed up first. The folder is analysed again, and the result stays only if it is "
                   "strictly better.");
    } else {
        set_screen("Undo the repair?",
                   "Every file it changed goes back to how it was before, including any change made to those files "
                   "since.");
    }
}

static void answer(int yes) {
    Asking what = app.asking;
    restore_screen();
    if (!yes) return;
    if (what == ASK_APPLY) apply_repair();
    else undo_repair();
}

/* ---------- the folder browser ---------- */

static void free_entries(void) {
    for (size_t i = 0; i < app.entry_count; i++) free(app.entries[i].name);
    free(app.entries);
    app.entries = NULL;
    app.entry_count = 0;
}

static int by_name(const void *a, const void *b) {
    const Entry *x = a, *y = b;
    if (x->is_dir != y->is_dir) return y->is_dir - x->is_dir;
    return strcasecmp(x->name, y->name);
}

static int ends_with(const char *s, const char *tail) {
    size_t a = strlen(s), b = strlen(tail);
    return a >= b && strcasecmp(s + a - b, tail) == 0;
}

/* The folder's entries: folders, and in a file browser the files that can be opened. */
static void browse_to(const char *dir) {
    char real[MAX_PATH_TEXT];
    if (!realpath(dir, real)) return;
    DIR *d = opendir(real);
    if (!d) {
        replace_text(&app.detail, "That folder cannot be opened.");
        app.dirty = 1;
        return;
    }
    free_entries();
    snprintf(app.browse_dir, sizeof(app.browse_dir), "%s", real);
    size_t room = 0;
    struct dirent *e;
    while ((e = readdir(d)) != NULL) {
        if (strcmp(e->d_name, ".") == 0 || strcmp(e->d_name, "..") == 0) continue;
        char full[MAX_PATH_TEXT + 256];
        snprintf(full, sizeof(full), "%s/%s", real, e->d_name);
        struct stat st;
        if (stat(full, &st) != 0) continue;
        int is_dir = S_ISDIR(st.st_mode);
        /* hidden folders stay out, but the game's own is the one people look for */
        if (e->d_name[0] == '.' && strcmp(e->d_name, ".minecraft") != 0) continue;
        if (!is_dir && !(app.browse_files && (ends_with(e->d_name, ".jar") || ends_with(e->d_name, ".zip")))) continue;
        if (app.entry_count == room) {
            size_t bigger = room ? room * 2 : 64;
            Entry *grown = realloc(app.entries, bigger * sizeof(Entry));
            if (!grown) break;
            app.entries = grown;
            room = bigger;
        }
        char *name = copy_text(e->d_name);
        if (!name) break;
        app.entries[app.entry_count++] = (Entry){name, is_dir};
    }
    closedir(d);
    if (app.entry_count) qsort(app.entries, app.entry_count, sizeof(Entry), by_name);
    /* room for the way up */
    if (strcmp(real, "/") != 0) {
        Entry *grown = realloc(app.entries, (app.entry_count + 1) * sizeof(Entry));
        char *up = copy_text("..");
        if (grown && up) {
            app.entries = grown;
            memmove(app.entries + 1, app.entries, app.entry_count * sizeof(Entry));
            app.entries[0] = (Entry){up, 1};
            app.entry_count++;
        } else {
            if (grown) app.entries = grown;
            free(up);
        }
    }
    app.selected = app.entry_count ? 0 : -1;
    app.top = 0;
    set_screen(app.browse_files ? "Choose a mod or a pack" : "Choose a pack, a world or a mods folder", app.browse_dir);
}

static void open_browser(int files) {
    if (app.busy) return;
    save_screen();
    app.mode = MODE_BROWSE;
    app.browse_files = files;
    app.focus = FOCUS_LIST;
    const char *start = app.browse_dir[0] ? app.browse_dir : app.shortcuts[0] ? app.shortcuts[0] : getenv("HOME");
    browse_to(start ? start : "/");
    app.dirty = 1;
}

static void leave_browser(void) {
    /* the list under it is still the one from before */
    long keep_selected = -1;
    free_entries();
    restore_screen();
    app.selected = keep_selected;
    app.top = 0;
    app.dirty = 1;
}

static void browser_activate(long row) {
    if (row < 0 || (size_t)row >= app.entry_count) return;
    char full[MAX_PATH_TEXT + 256];
    snprintf(full, sizeof(full), "%s/%s", app.browse_dir, app.entries[row].name);
    if (app.entries[row].is_dir) {
        browse_to(full);
    } else {
        free_entries();
        app.mode = MODE_MAIN;
        open_path(full);
    }
}

static void browser_use_folder(void) {
    char dir[MAX_PATH_TEXT];
    snprintf(dir, sizeof(dir), "%s", app.browse_dir);
    free_entries();
    app.mode = MODE_MAIN;
    open_path(dir);
}

/* ---------- the list ---------- */

static void select_row(long row) {
    long total = row_count();
    if (total == 0) return;
    if (row < 0) row = 0;
    if (row >= total) row = total - 1;
    app.selected = row;
    keep_selected_shown();
    const char *kind, *name, *where;
    row_cells(row, &kind, &name, &where);
    if (app.mode == MODE_BROWSE) {
        replace_text(&app.detail, NULL);
    } else {
        replace_text(&app.detail, app.list == LIST_HOME ? where : name);
    }
    app.dirty = 1;
}

static void activate_row(long row) {
    if (app.mode == MODE_BROWSE) browser_activate(row);
    else if (app.list == LIST_HOME && row >= 0 && (size_t)row < app.place_count) open_path(app.places[row].path);
}

/* ---------- tests: see the top of this file ---------- */

static void next_step(void) {
    if (!app.capture_to) return;
    while (app.steps && *app.steps) {
        char step = *app.steps++;
        if (step == 's') {
            select_row(0);
            continue;
        }
        if (step == 'p') preview_repair();
        else if (step == 'a') apply_repair();
        else if (step == 'u') undo_repair();
        else continue;
        if (app.busy) return; /* it comes back here when it is done */
    }
    app.exit_code = save_capture(app.capture_to) == 0 ? 0 : 3;
    app.running_loop = 0;
}

/* ---------- input ---------- */

static void press(int b) {
    if (!app.visible[b] || !button_enabled(b)) return;
    switch (b) {
    case B_HOME: go_home(); break;
    case B_ANALYSE: analyse_typed(); break;
    case B_FOLDER: open_browser(0); break;
    case B_FILE: open_browser(1); break;
    case B_PREVIEW: preview_repair(); break;
    case B_APPLY: ask(ASK_APPLY); break;
    case B_UNDO: ask(ASK_UNDO); break;
    case B_CANCEL:
        __atomic_store_n(&app.cancel, 1, __ATOMIC_SEQ_CST);
        set_screen("Cancelling...", "");
        break;
    case B_USE: browser_use_folder(); break;
    case B_BACK: leave_browser(); break;
    case B_YES: answer(1); break;
    case B_NO: answer(0); break;
    }
    app.dirty = 1;
}

static int inside(Rect r, int x, int y) { return x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h; }

static void on_button(const u8 *e) {
    int button = e[1];
    int x = (int16_t)get16(e + 24), y = (int16_t)get16(e + 26);
    u32 time = get32(e + 4);
    if (button == 4 || button == 5) {
        if (inside(app.list_rect, x, y)) {
            app.top += button == 4 ? -3 : 3;
            clamp_top();
            app.dirty = 1;
        }
        return;
    }
    if (button == 2 && inside(app.path_rect, x, y)) {
        /* the middle button pastes the current selection, as everywhere on X */
        app.focus = FOCUS_PATH;
        request(24, 0, 6); /* ConvertSelection: PRIMARY (atom 1) */
        put32(app.window);
        put32(1);
        put32(app.atom_utf8);
        put32(app.atom_paste);
        put32(time);
        return;
    }
    if (button != 1) return;
    for (int b = 0; b < BUTTON_COUNT; b++) {
        if (app.visible[b] && inside(app.buttons[b], x, y)) {
            press(b);
            return;
        }
    }
    if (inside(app.path_rect, x, y)) {
        app.focus = FOCUS_PATH;
        app.caret = strlen(app.edit);
        app.dirty = 1;
        return;
    }
    if (inside(app.list_rect, x, y)) {
        app.focus = FOCUS_LIST;
        app.dirty = 1;
        Rect r = app.list_rect;
        if (x >= r.x + r.w - app.cw * 2 - 1 && row_count() > rows_shown()) {
            /* the scroll bar: a page towards the click */
            long at = (long)(y - r.y) * row_count() / r.h;
            app.top += at < app.top ? -rows_shown() : rows_shown();
            clamp_top();
            return;
        }
        int first_row_y = r.y + 2 + row_height();
        if (y < first_row_y) return;
        long row = app.top + (y - first_row_y) / row_height();
        if (row >= row_count()) return;
        int twice = row == app.last_click_row && time - app.last_click_time < 450;
        app.last_click_row = row;
        app.last_click_time = time;
        select_row(row);
        if (twice) {
            app.last_click_row = -1;
            activate_row(row);
        }
    }
}

static void edit_insert(const char *text, size_t n) {
    size_t have = strlen(app.edit);
    if (have + n >= sizeof(app.edit)) return;
    memmove(app.edit + app.caret + n, app.edit + app.caret, have - app.caret + 1);
    memcpy(app.edit + app.caret, text, n);
    app.caret += n;
    app.dirty = 1;
}

static size_t put_utf8(char *to, u32 c) {
    if (c < 0x80) {
        to[0] = (char)c;
        return 1;
    }
    if (c < 0x800) {
        to[0] = (char)(0xC0 | (c >> 6));
        to[1] = (char)(0x80 | (c & 0x3F));
        return 2;
    }
    to[0] = (char)(0xE0 | (c >> 12));
    to[1] = (char)(0x80 | ((c >> 6) & 0x3F));
    to[2] = (char)(0x80 | (c & 0x3F));
    return 3;
}

static size_t char_before(size_t at) {
    while (at > 0) {
        at--;
        if (((u8)app.edit[at] & 0xC0) != 0x80) break;
    }
    return at;
}

static size_t char_after(size_t at) {
    if (!app.edit[at]) return at;
    at++;
    while (((u8)app.edit[at] & 0xC0) == 0x80) at++;
    return at;
}

enum {
    KEY_BACKSPACE = 0xFF08,
    KEY_TAB = 0xFF09,
    KEY_RETURN = 0xFF0D,
    KEY_ESCAPE = 0xFF1B,
    KEY_HOME = 0xFF50,
    KEY_LEFT = 0xFF51,
    KEY_UP = 0xFF52,
    KEY_RIGHT = 0xFF53,
    KEY_DOWN = 0xFF54,
    KEY_PAGE_UP = 0xFF55,
    KEY_PAGE_DOWN = 0xFF56,
    KEY_END = 0xFF57,
    KEY_PAD_ENTER = 0xFF8D,
    KEY_F5 = 0xFFC2,
    KEY_DELETE = 0xFFFF
};

static u32 keysym_of(u8 code, int shift) {
    if (!app.keysyms || code < app.min_keycode) return 0;
    long row = (long)code - app.min_keycode;
    if (row >= app.code_count) return 0;
    u32 plain = app.keysyms[row * app.syms_per_code];
    u32 shifted = app.syms_per_code > 1 ? app.keysyms[row * app.syms_per_code + 1] : 0;
    if (shift && shifted) return shifted;
    return plain;
}

static void on_key(const u8 *e) {
    u16 state = get16(e + 28);
    int shift = state & 1, control = state & 4;
    u32 sym = keysym_of(e[1], shift);
    if (!sym) return;

    if (control) {
        u32 lower = sym >= 'A' && sym <= 'Z' ? sym + 32 : sym;
        if (lower == 'o' && app.mode == MODE_MAIN) press(shift ? B_FILE : B_FOLDER);
        else if (lower == 'l' && app.mode == MODE_MAIN) {
            app.focus = FOCUS_PATH;
            app.caret = strlen(app.edit);
            app.dirty = 1;
        } else if (lower == 'v' && app.focus == FOCUS_PATH) {
            request(24, 0, 6); /* ConvertSelection: CLIPBOARD */
            put32(app.window);
            put32(app.atom_clipboard);
            put32(app.atom_utf8);
            put32(app.atom_paste);
            put32(get32(e + 4));
        }
        return;
    }
    if (app.mode == MODE_ASK) {
        if (sym == KEY_RETURN || sym == 'y') answer(1);
        else if (sym == KEY_ESCAPE || sym == 'n') answer(0);
        return;
    }
    if (sym == KEY_ESCAPE) {
        if (app.mode == MODE_BROWSE) leave_browser();
        else press(B_CANCEL);
        return;
    }
    if (sym == KEY_F5 && app.mode == MODE_MAIN) {
        press(B_ANALYSE);
        return;
    }
    if (sym == KEY_TAB && app.mode == MODE_MAIN) {
        app.focus = app.focus == FOCUS_LIST ? FOCUS_PATH : FOCUS_LIST;
        app.dirty = 1;
        return;
    }

    if (app.focus == FOCUS_PATH && app.mode == MODE_MAIN) {
        switch (sym) {
        case KEY_RETURN:
        case KEY_PAD_ENTER: analyse_typed(); return;
        case KEY_BACKSPACE:
            if (app.caret) {
                size_t from = char_before(app.caret);
                memmove(app.edit + from, app.edit + app.caret, strlen(app.edit + app.caret) + 1);
                app.caret = from;
            }
            break;
        case KEY_DELETE:
            if (app.edit[app.caret]) {
                size_t to = char_after(app.caret);
                memmove(app.edit + app.caret, app.edit + to, strlen(app.edit + to) + 1);
            }
            break;
        case KEY_LEFT: app.caret = char_before(app.caret); break;
        case KEY_RIGHT: app.caret = char_after(app.caret); break;
        case KEY_HOME: app.caret = 0; break;
        case KEY_END: app.caret = strlen(app.edit); break;
        default: {
            /* a character: Latin-1 keysyms are their code point, others say it */
            u32 c = sym >= 0x01000000u ? sym & 0x00FFFFFFu : sym;
            if ((c >= 0x20 && c < 0x7F) || (c >= 0xA0 && c < 0x10000 && sym < 0xFD00)) {
                char utf8[4];
                edit_insert(utf8, put_utf8(utf8, c));
            }
        }
        }
        app.dirty = 1;
        return;
    }

    switch (sym) {
    case KEY_UP: select_row(app.selected - 1); break;
    case KEY_DOWN: select_row(app.selected + 1); break;
    case KEY_PAGE_UP: select_row(app.selected - rows_shown()); break;
    case KEY_PAGE_DOWN: select_row(app.selected + rows_shown()); break;
    case KEY_HOME: select_row(0); break;
    case KEY_END: select_row(row_count() - 1); break;
    case KEY_RETURN:
    case KEY_PAD_ENTER: activate_row(app.selected); break;
    case KEY_BACKSPACE:
        if (app.mode == MODE_BROWSE && strcmp(app.browse_dir, "/") != 0) browser_activate(0);
        break;
    default: break;
    }
}

/* What was pasted: the server has put it in a property of the window. */
static void on_selection(const u8 *e) {
    u32 property = get32(e + 20);
    if (property == 0) return; /* nobody has anything to paste */
    request(20, 1, 6); /* GetProperty, deleting it */
    put32(app.window);
    put32(property);
    put32(0);
    put32(0);
    put32(MAX_PATH_TEXT / 4);
    u8 head[32];
    u8 *data = NULL;
    size_t size = 0;
    if (!wait_reply(head, &data, &size)) return;
    size_t n = get32(head + 16);
    if (head[1] == 8 && data && n <= size) {
        for (size_t i = 0; i < n; i++) {
            if (data[i] == '\n' || data[i] == '\r') n = i;
        }
        edit_insert((char *)data, n);
    }
    free(data);
}

static void on_event(const u8 *e) {
    switch (e[0] & 0x7F) {
    case 0: say_error(e); break;
    case 2: on_key(e); break;
    case 4: on_button(e); break;
    case 12: app.dirty = 1; break; /* Expose */
    case 22:                       /* ConfigureNotify */
        if (get32(e + 8) == app.window) {
            int w = get16(e + 20), h = get16(e + 22);
            if (w != app.w || h != app.h) {
                app.w = w;
                app.h = h;
                app.dirty = 1;
            }
        }
        break;
    case 31: on_selection(e); break;
    case 33: /* ClientMessage: the window manager asks to close */
        if (get32(e + 8) == app.atom_protocols && get32(e + 12) == app.atom_delete) {
            /* a repair cut short would leave files half changed */
            if (app.busy && (app.running == JOB_APPLY || app.running == JOB_UNDO)) {
                set_screen("A repair is running", "It will finish in a moment.");
            } else {
                app.running_loop = 0;
            }
        }
        break;
    default: break;
    }
}

static void on_note(Note *note) {
    if (!note->done) {
        if (app.busy && app.running == JOB_REPORT && note->text) {
            char line[512];
            snprintf(line, sizeof(line), "Step %u of %u: %s", note->step + 1, note->total, note->text);
            replace_text(&app.summary, line);
            app.dirty = 1;
        }
        free(note->text);
        free(note);
        return;
    }
    char *json = note->text;
    JobKind kind = note->kind;
    free(note);
    app.busy = 0;
    switch (kind) {
    case JOB_REPORT: show_analysis(json); break;
    case JOB_PLAN: show_plan(json); break;
    case JOB_APPLY: show_outcome(json, "The repair gave no answer"); break;
    case JOB_UNDO: show_outcome(json, "The undo gave no answer"); break;
    case JOB_PLACES: show_places(json); break;
    }
    if (json) app.engine.string_free(json);
    app.dirty = 1;
    next_step();
}

/* ---------- start ---------- */

static int load_engine(void) {
    char path[MAX_PATH_TEXT];
    ssize_t n = readlink("/proc/self/exe", path, sizeof(path) - 32);
    if (n <= 0) return -1;
    path[n] = '\0';
    char *slash = strrchr(path, '/');
    if (!slash) return -1;
    strcpy(slash + 1, "libkebitx_ffi.so");
    void *engine = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!engine) {
        fprintf(stderr, "kebitx-app: %s\n", dlerror());
        return -1;
    }
    /* POSIX gives every symbol the same pointer type */
    ReportFn report = (ReportFn)dlsym(engine, "kebitx_report_with_progress");
    app.engine.plan = (PlanFn)dlsym(engine, "kebitx_repair_plan");
    app.engine.apply = (ApplyFn)dlsym(engine, "kebitx_repair_apply");
    app.engine.undo = (UndoFn)dlsym(engine, "kebitx_repair_undo");
    app.engine.places = (PlacesFn)dlsym(engine, "kebitx_places");
    app.engine.string_free = (FreeFn)dlsym(engine, "kebitx_string_free");
    if (!report || !app.engine.plan || !app.engine.apply || !app.engine.undo || !app.engine.places ||
        !app.engine.string_free)
        return -1;
    app.engine.report = report;
    return 0;
}

static void read_keyboard(void) {
    u8 count = (u8)(app.max_keycode - app.min_keycode + 1);
    request(101, 0, 2); /* GetKeyboardMapping */
    put8(app.min_keycode);
    put8(count);
    put16(0);
    u8 head[32];
    u8 *data = NULL;
    size_t size = 0;
    if (!wait_reply(head, &data, &size)) return;
    app.syms_per_code = head[1];
    app.code_count = count;
    if (app.syms_per_code <= 0 || size < (size_t)count * (size_t)app.syms_per_code * 4) {
        free(data);
        return;
    }
    free(app.keysyms);
    app.keysyms = (u32 *)(void *)data; /* from malloc: fit for any type */
}

static void make_window(void) {
    app.w = app.cw * 110;
    app.h = app.ch * 40;
    app.window = new_id();
    /* KeyPress, ButtonPress, Exposure, StructureNotify */
    u32 events = (1u << 0) | (1u << 2) | (1u << 15) | (1u << 17);
    request(1, 0, 10); /* CreateWindow, with the screen's own depth and visual */
    put32(app.window);
    put32(app.root);
    put_i16(0);
    put_i16(0);
    put16((u16)app.w);
    put16((u16)app.h);
    put16(0);
    put16(1); /* InputOutput */
    put32(0);
    put32((1u << 1) | (1u << 11)); /* background pixel, event mask */
    put32(COLOR_BACK);
    put32(events);

    app.gc = new_id();
    request(55, 0, 6); /* CreateGC */
    put32(app.gc);
    put32(app.window);
    put32((1u << 14) | (1u << 16)); /* the font, and no events about copies */
    put32(app.font);
    put32(0);

    static const char title[] = "Kebitx";
    size_t n = strlen(title);
    request(18, 0, (u16)(6 + (n + 3) / 4)); /* ChangeProperty: WM_NAME (39), STRING (31) */
    put32(app.window);
    put32(39);
    put32(31);
    put8(8);
    put8(0);
    put16(0);
    put32((u32)n);
    put(title, n);
    pad4(n);
    request(18, 0, (u16)(6 + (n + 3) / 4)); /* and as the newer desktops read it */
    put32(app.window);
    put32(app.atom_net_name);
    put32(app.atom_utf8);
    put8(8);
    put8(0);
    put16(0);
    put32((u32)n);
    put(title, n);
    pad4(n);
    request(18, 0, 7); /* WM_PROTOCOLS: tell me when closed, do not just cut me off */
    put32(app.window);
    put32(app.atom_protocols);
    put32(4); /* ATOM */
    put8(32);
    put8(0);
    put16(0);
    put32(1);
    put32(app.atom_delete);
}

int main(int argc, char **argv) {
    const char *start_path = NULL;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--capture") == 0 && i + 1 < argc) app.capture_to = argv[++i];
        else if (strcmp(argv[i], "--steps") == 0 && i + 1 < argc) app.steps = argv[++i];
        else start_path = argv[i];
    }
    if (connect_server() != 0) {
        fprintf(stderr, "kebitx-app: no X server to talk to (is DISPLAY set?)\n");
        return 4;
    }
    app.atom_protocols = intern("WM_PROTOCOLS");
    app.atom_delete = intern("WM_DELETE_WINDOW");
    app.atom_utf8 = intern("UTF8_STRING");
    app.atom_net_name = intern("_NET_WM_NAME");
    app.atom_clipboard = intern("CLIPBOARD");
    app.atom_paste = intern("KEBITX_PASTE");
    if (open_font() != 0) {
        fprintf(stderr, "kebitx-app: the X server has no usable font\n");
        return 4;
    }
    read_keyboard();
    make_window();
    if (pipe2(app.notes, O_CLOEXEC) != 0) return 4;

    app.selected = -1;
    app.last_click_row = -1;
    app.running_loop = 1;
    int engine = load_engine();
    if (engine != 0) {
        if (app.capture_to) return 2;
        set_screen("The engine is missing", "A current libkebitx_ffi.so must be next to this program.");
    }
    request(8, 0, 2); /* MapWindow */
    put32(app.window);
    app.dirty = 1;

    if (engine == 0) {
        if (start_path) open_path(start_path);
        else go_home();
    }

    while (app.running_loop) {
        while (app.queue_len && app.running_loop) {
            u8 e[32];
            memcpy(e, app.queue[app.queue_head], 32);
            app.queue_head = (app.queue_head + 1) % 256;
            app.queue_len--;
            on_event(e);
        }
        if (!app.running_loop) break;
        if (app.dirty) redraw();
        flush_out();
        struct pollfd fds[2] = {{app.fd, POLLIN, 0}, {app.notes[0], POLLIN, 0}};
        if (poll(fds, 2, -1) < 0) {
            if (errno == EINTR) continue;
            return 4;
        }
        if (fds[1].revents & POLLIN) {
            Note *note = NULL;
            if (read(app.notes[0], &note, sizeof(note)) == (ssize_t)sizeof(note) && note) on_note(note);
        }
        if (app.running_loop && (fds[0].revents & (POLLIN | POLLHUP))) {
            u8 m[32];
            read_all(m, 32);
            if (m[0] == 1) {
                /* an answer nobody waits for: its body must not be read as events */
                size_t more = (size_t)get32(m + 4) * 4;
                while (more) {
                    u8 skip[256];
                    size_t take = more < sizeof(skip) ? more : sizeof(skip);
                    read_all(skip, take);
                    more -= take;
                }
            } else {
                on_event(m);
            }
        }
    }
    flush_out();
    return app.exit_code;
}
