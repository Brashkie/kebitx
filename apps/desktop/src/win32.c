/* kebitx's desktop app on Windows, in C on Win32 alone. The engine is
 * kebitx_ffi.dll, next to the executable; the window shows what it says, and
 * decides nothing itself. What the window says it asks the engine to translate.
 *
 * The look is drawn here, on real controls: buttons, the list and the boxes stay
 * Windows' own, so the keyboard, screen readers and text input work as anywhere.
 *
 *   kebitx-app.exe [path]
 *
 * For tests, with no one at the screen:
 *
 *   kebitx-app.exe [path] --steps sp --capture out.bmp
 *
 * opens path, or the list of places without one, runs the steps in order (s:
 * select the first row, p: preview the repair, a: apply it, without asking, u:
 * undo it, k: package it to the file `--package-to` names, w: read the world),
 * saves the window and
 * exits. `--theme dark|light` and `--lang es` override what the settings say, and
 * a test leaves no settings or recent items.
 */
#define WIN32_LEAN_AND_MEAN
#define UNICODE
#define _UNICODE
#define COBJMACROS
#include <windows.h>
#include <commctrl.h>
#include <commdlg.h>
#include <dwmapi.h>
#include <math.h>
#include <objbase.h>
#include <shellapi.h>
#include <shlwapi.h>
#include <shobjidl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <uxtheme.h>
#include <wchar.h>

#include "report.h"

enum {
    /* the buttons, in the order of app.buttons */
    ID_HOME = 100,
    ID_OPEN_FOLDER,
    ID_OPEN_FILE,
    ID_LANGUAGE,
    ID_THEME,
    ID_ANALYSE,
    ID_TILE_0,
    ID_TILE_1,
    ID_TILE_2,
    ID_TILE_3,
    ID_GROUP,
    ID_PREVIEW,
    ID_APPLY,
    ID_UNDO,
    ID_SHOW,
    ID_COPY,
    ID_PACKAGE,
    ID_WORLD,
    ID_CANCEL,

    ID_PATH = 120,
    ID_FOCUS_PATH,
    ID_FILTER,
    ID_FOCUS_FILTER,
    ID_COUNT,
    ID_HEADER,
    ID_SUMMARY,
    ID_LIST,
    ID_DETAIL,

    ID_LANGUAGE_FIRST = 200,

    TIMER_COPIED = 1,
    TIMER_ANIMATION = 2,
    WM_APP_DONE = WM_APP + 1,
    WM_APP_PROGRESS
};

#define BUTTON_COUNT (ID_CANCEL - ID_HOME + 1)
#define BUTTON(id) app.buttons[(id) - ID_HOME]
#define TILES 4

typedef enum { JOB_REPORT, JOB_PLAN, JOB_APPLY, JOB_UNDO, JOB_PLACES, JOB_PACKAGE, JOB_WORLD } JobKind;
typedef enum { VERDICT_NONE, VERDICT_GOOD, VERDICT_BAD, VERDICT_NOTHING } Verdict;

/* What a thread takes: one call into the engine. */
typedef struct {
    JobKind kind;
    char *path;        /* the folder or file; the backup, for JOB_UNDO */
    char *fingerprint; /* for JOB_APPLY; the file to write, for JOB_PACKAGE */
} Job;

/* A row of the first screen: something on this computer to analyse. */
typedef struct {
    char *what; /* in English: it says which colour and which tile */
    WCHAR *name, *path;
} Place;

/* One thing to fix, and everything it touches. */
typedef struct {
    int count, is_problem, is_dead;
    WCHAR *what, *fix, *places, *line;
} Cause;

typedef int (*ProgressFn)(const char *, unsigned, unsigned, void *);
typedef char *(*ReportFn)(const char *, ProgressFn, void *);
typedef char *(*WorldFn)(const char *, ProgressFn, void *);
typedef char *(*PlanFn)(const char *);
typedef char *(*ApplyFn)(const char *, const char *);
typedef char *(*UndoFn)(const char *);
typedef char *(*PlacesFn)(void);
typedef void (*FreeFn)(char *);
typedef int (*SetLanguageFn)(const char *);
typedef const char *(*LanguagesFn)(void);
typedef const char *(*TrFn)(const char *);

typedef struct {
    COLORREF back, rail, card, field, line, text, dim, accent, accent2, alt, good, bad, warn, primary_a, primary_b, on_primary;
} Palette;

static struct {
    HWND window, buttons[BUTTON_COUNT], path_box, filter, count, header, summary, list, detail, tips;
    HFONT font, font_small, font_title, font_brand, font_number, font_icon, font_icon_big;
    UINT dpi;
    struct {
        ReportFn report;
        PlanFn plan;
        ApplyFn apply;
        UndoFn undo;
        ApplyFn package;
        WorldFn world;
        PlacesFn places;
        FreeFn string_free;
        SetLanguageFn set_language;
        LanguagesFn languages;
        TrFn tr;
    } engine;

    int busy;             /* a thread is in the engine */
    JobKind running;      /* which job it is, while busy */
    volatile LONG cancel; /* the analysis was asked to stop */
    unsigned step, steps_total;
    int phase;            /* of the animation while busy */

    char *path;        /* what is open, as UTF-8 */
    char *fingerprint; /* of the preview on screen */
    char *backup;      /* of the last repair kept */

    int has_report;
    Report current;
    WCHAR **row_text;      /* each row, as Windows wants it */
    const char *row_label; /* for rows that are problems */
    Cause *causes;
    size_t cause_count;
    int by_cause; /* the list shows causes, when the report has them */

    int home; /* the list shows the places, not a report */
    int inspecting; /* the list shows what a world has saved */
    WCHAR *world_details; /* the rest of what is known of it */
    int can_inspect; /* what is open is a world with saved data */
    Place *places;
    size_t place_count;
    WCHAR *shortcuts[2]; /* game folders, for the file dialogs */

    size_t *view; /* the rows the filter lets through */
    size_t view_count;
    int tile; /* which tile narrows the list; 0 is all */
    size_t tile_counts[TILES];
    int tiles_known;

    Verdict verdict;
    Palette c;
    int dark;
    char theme[8];    /* dark, light or system */
    char language[8];
    int hot;          /* the button under the mouse */
    HBRUSH back_brush, card_brush, field_brush;
    RECT path_card, hero_card, filter_card, list_card, detail_card;
    DWORD *logo; /* the logo at this window's size, or NULL */
    int logo_size;
    int compact; /* too narrow for every label: some buttons are only their icon */

    const WCHAR *capture_to; /* tests: see the top of this file */
    const WCHAR *steps;
    const WCHAR *forced_theme;
    const WCHAR *forced_language;
    const WCHAR *package_to;
} app;

/* ---------- text ---------- */

static WCHAR *wide(const char *utf8) {
    int n = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, NULL, 0);
    WCHAR *out = n > 0 ? malloc((size_t)n * sizeof(WCHAR)) : NULL;
    if (out) MultiByteToWideChar(CP_UTF8, 0, utf8, -1, out, n);
    return out;
}

static char *narrow(const WCHAR *text) {
    int n = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, text, -1, NULL, 0, NULL, NULL);
    char *out = n > 0 ? malloc((size_t)n) : NULL;
    if (out) WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, text, -1, out, n, NULL, NULL);
    return out;
}

static char *copy_text(const char *s) {
    if (!s) return NULL;
    char *out = malloc(strlen(s) + 1);
    if (out) strcpy(out, s);
    return out;
}

static WCHAR *copy_wide(const WCHAR *s) {
    WCHAR *out = malloc((wcslen(s) + 1) * sizeof(WCHAR));
    if (out) wcscpy(out, s);
    return out;
}

static void replace_text(char **slot, const char *s) {
    free(*slot);
    *slot = copy_text(s);
}

static void set_text(HWND control, const char *utf8) {
    WCHAR *text = wide(utf8 ? utf8 : "");
    SetWindowTextW(control, text ? text : L"");
    free(text);
}

/* What the window says, in the language in use. The answers are kept until the
 * language changes: drawing asks for them many times a second. */
#define SAID_SLOTS 512
static struct {
    char *key;
    WCHAR *value;
} said[SAID_SLOTS];

static void forget_said(void) {
    for (int i = 0; i < SAID_SLOTS; i++) {
        free(said[i].key);
        free(said[i].value);
        said[i].key = NULL;
        said[i].value = NULL;
    }
}

static const WCHAR *T(const char *english) {
    unsigned hash = 2166136261u;
    for (const char *p = english; *p; p++) hash = (hash ^ (unsigned char)*p) * 16777619u;
    for (unsigned probe = 0; probe < SAID_SLOTS; probe++) {
        unsigned i = (hash + probe) % SAID_SLOTS;
        if (said[i].key && strcmp(said[i].key, english) == 0) return said[i].value;
        if (!said[i].key) {
            const char *text = app.engine.tr ? app.engine.tr(english) : english;
            said[i].key = copy_text(english);
            said[i].value = wide(text);
            return said[i].value ? said[i].value : L"";
        }
    }
    return L"";
}

/* `{0}`, `{1}` in a text, replaced. */
static void fill_in(WCHAR *out, size_t room, const WCHAR *text, int count, ...) {
    const WCHAR *values[4] = {0};
    va_list list;
    va_start(list, count);
    for (int i = 0; i < count && i < 4; i++) values[i] = va_arg(list, const WCHAR *);
    va_end(list);
    size_t at = 0;
    for (const WCHAR *p = text; *p && at + 1 < room; p++) {
        if (p[0] == L'{' && p[1] >= L'0' && p[1] <= L'3' && p[2] == L'}' && values[p[1] - L'0']) {
            for (const WCHAR *v = values[p[1] - L'0']; *v && at + 1 < room; v++) out[at++] = *v;
            p += 2;
        } else {
            out[at++] = *p;
        }
    }
    out[at] = L'\0';
}

/* ---------- drawing, with smooth corners ---------- */

/* The window paints into pixels of its own, then copies them at once, so nothing
 * flickers. Text goes on top through the DC, as Windows draws it best. */
typedef struct {
    HDC dc;
    HBITMAP bitmap;
    HGDIOBJ old;
    DWORD *pixels;
    int w, h;
} Canvas;

static int canvas_begin(Canvas *c, HDC target, int w, int h) {
    memset(c, 0, sizeof(*c));
    c->w = w;
    c->h = h;
    BITMAPINFO info = {0};
    info.bmiHeader.biSize = sizeof(info.bmiHeader);
    info.bmiHeader.biWidth = w;
    info.bmiHeader.biHeight = -h;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    c->dc = CreateCompatibleDC(target);
    c->bitmap = c->dc ? CreateDIBSection(c->dc, &info, DIB_RGB_COLORS, (void **)&c->pixels, NULL, 0) : NULL;
    if (!c->bitmap) {
        if (c->dc) DeleteDC(c->dc);
        return -1;
    }
    c->old = SelectObject(c->dc, c->bitmap);
    SetBkMode(c->dc, TRANSPARENT);
    return 0;
}

static void canvas_end(Canvas *c, HDC target, int x, int y) {
    GdiFlush();
    BitBlt(target, x, y, c->w, c->h, c->dc, 0, 0, SRCCOPY);
    SelectObject(c->dc, c->old);
    DeleteObject(c->bitmap);
    DeleteDC(c->dc);
}

static DWORD mix(DWORD under, COLORREF color, unsigned alpha) {
    unsigned r = GetRValue(color), g = GetGValue(color), b = GetBValue(color);
    unsigned ur = (under >> 16) & 255, ug = (under >> 8) & 255, ub = under & 255;
    unsigned nr = (r * alpha + ur * (255 - alpha)) / 255;
    unsigned ng = (g * alpha + ug * (255 - alpha)) / 255;
    unsigned nb = (b * alpha + ub * (255 - alpha)) / 255;
    return (nr << 16) | (ng << 8) | nb;
}

static COLORREF between(COLORREF a, COLORREF b, float t) {
    return RGB((int)((float)GetRValue(a) + ((float)GetRValue(b) - (float)GetRValue(a)) * t),
               (int)((float)GetGValue(a) + ((float)GetGValue(b) - (float)GetGValue(a)) * t),
               (int)((float)GetBValue(a) + ((float)GetBValue(b) - (float)GetBValue(a)) * t));
}

/* A rounded rectangle: each pixel covered as much as its distance to the edge says.
 * With `to` different from `color`, a gradient from left to right. */
static void round_rect(Canvas *c, int x, int y, int w, int h, int radius, COLORREF color, COLORREF to, unsigned alpha) {
    if (w <= 0 || h <= 0) return;
    float cx = (float)x + (float)w / 2, cy = (float)y + (float)h / 2;
    float hw = (float)w / 2 - (float)radius, hh = (float)h / 2 - (float)radius;
    int left = x < 0 ? 0 : x, top = y < 0 ? 0 : y;
    int right = x + w > c->w ? c->w : x + w, bottom = y + h > c->h ? c->h : y + h;
    for (int py = top; py < bottom; py++) {
        DWORD *row = c->pixels + (size_t)py * (size_t)c->w;
        for (int px_ = left; px_ < right; px_++) {
            float qx = fabsf((float)px_ + 0.5f - cx) - hw, qy = fabsf((float)py + 0.5f - cy) - hh;
            float ax = qx > 0 ? qx : 0, ay = qy > 0 ? qy : 0;
            float inner = qx > qy ? qx : qy;
            float d = sqrtf(ax * ax + ay * ay) + (inner < 0 ? inner : 0) - (float)radius;
            float cover = 0.5f - d;
            if (cover <= 0) continue;
            if (cover > 1) cover = 1;
            COLORREF ink = color == to ? color : between(color, to, (float)(px_ - x) / (float)w);
            row[px_] = mix(row[px_], ink, (unsigned)(cover * (float)alpha));
        }
    }
}

static void card(Canvas *c, RECT r, int radius, COLORREF fill, COLORREF line) {
    int w = r.right - r.left, h = r.bottom - r.top;
    round_rect(c, r.left, r.top, w, h, radius, line, line, 255);
    round_rect(c, r.left + 1, r.top + 1, w - 2, h - 2, radius - 1, fill, fill, 255);
}

static void text_in(Canvas *c, HFONT font, COLORREF color, const WCHAR *text, RECT r, UINT format) {
    HGDIOBJ old = SelectObject(c->dc, font);
    SetTextColor(c->dc, color);
    DrawTextW(c->dc, text, -1, &r, format | DT_NOPREFIX);
    SelectObject(c->dc, old);
}

/* ---------- the colours ---------- */

static int px(int design) { return MulDiv(design, (int)app.dpi, 96); }

static int system_is_dark(void) {
    DWORD light = 1, size = sizeof(light);
    LSTATUS got = RegGetValueW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
                               L"AppsUseLightTheme", RRF_RT_REG_DWORD, NULL, &light, &size);
    return got == ERROR_SUCCESS && light == 0;
}

static void set_palette(int dark) {
    /* the logo's colours: violet and white on black */
    static const Palette night = {
        RGB(8, 7, 14),      RGB(12, 10, 21),    RGB(18, 15, 31),    RGB(13, 11, 23),    RGB(46, 38, 72),
        RGB(238, 234, 250), RGB(152, 143, 178), RGB(167, 139, 250), RGB(192, 132, 252), RGB(103, 212, 232),
        RGB(52, 211, 153),  RGB(251, 113, 133), RGB(251, 191, 36),  RGB(139, 92, 246),  RGB(109, 40, 217),
        RGB(255, 255, 255),
    };
    static const Palette day = {
        RGB(245, 243, 251), RGB(255, 255, 255), RGB(255, 255, 255), RGB(249, 248, 253), RGB(219, 213, 236),
        RGB(24, 18, 44),    RGB(92, 84, 118),   RGB(109, 40, 217),  RGB(147, 51, 234),  RGB(8, 124, 158),
        RGB(4, 128, 90),    RGB(200, 30, 60),   RGB(170, 105, 0),   RGB(124, 58, 237),  RGB(91, 33, 182),
        RGB(255, 255, 255),
    };
    app.c = dark ? night : day;
    app.dark = dark;
}

/* The colours, and for each control the theme that draws it to match. */
static void apply_theme(void) {
    int dark;
    if (app.forced_theme) dark = wcscmp(app.forced_theme, L"dark") == 0;
    else if (strcmp(app.theme, "light") == 0) dark = 0;
    else if (strcmp(app.theme, "system") == 0) dark = system_is_dark();
    else dark = 1;
    set_palette(dark);
    if (app.back_brush) DeleteObject(app.back_brush);
    if (app.card_brush) DeleteObject(app.card_brush);
    if (app.field_brush) DeleteObject(app.field_brush);
    app.back_brush = CreateSolidBrush(app.c.back);
    app.card_brush = CreateSolidBrush(app.c.card);
    app.field_brush = CreateSolidBrush(app.c.field);

    BOOL on = dark;
    DwmSetWindowAttribute(app.window, 20 /* DWMWA_USE_IMMERSIVE_DARK_MODE */, &on, sizeof(on));
    COLORREF caption = app.c.rail;
    DwmSetWindowAttribute(app.window, 35 /* DWMWA_CAPTION_COLOR, Windows 11 */, &caption, sizeof(caption));
    const WCHAR *scrolled = dark ? L"DarkMode_Explorer" : L"Explorer";
    SetWindowTheme(app.detail, dark ? scrolled : NULL, NULL);
    SetWindowTheme(app.list, scrolled, NULL);
    SetWindowTheme(app.tips, dark ? scrolled : NULL, NULL);
    ListView_SetBkColor(app.list, app.c.card);
    ListView_SetTextBkColor(app.list, app.c.card);
    ListView_SetTextColor(app.list, app.c.text);
    RedrawWindow(app.window, NULL, NULL, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN);
}

static HFONT make_font(const WCHAR *face, int points_px, int weight) {
    LOGFONTW f = {0};
    f.lfHeight = -px(points_px);
    f.lfWeight = weight;
    f.lfCharSet = DEFAULT_CHARSET;
    f.lfQuality = CLEARTYPE_QUALITY;
    wcsncpy(f.lfFaceName, face, LF_FACESIZE - 1);
    return CreateFontIndirectW(&f);
}

static void make_logo(int size);

static void make_fonts(void) {
    HFONT *all[] = {&app.font, &app.font_small, &app.font_title, &app.font_brand, &app.font_number, &app.font_icon, &app.font_icon_big};
    for (size_t i = 0; i < sizeof(all) / sizeof(all[0]); i++) {
        if (*all[i]) DeleteObject(*all[i]);
    }
    /* Bahnschrift is Windows' own technical face; it has no Chinese, and Windows
       draws Chinese best with its own */
    int chinese = strcmp(app.language, "zh") == 0;
    const WCHAR *body = chinese ? L"Microsoft YaHei UI" : L"Segoe UI";
    const WCHAR *display = chinese ? L"Microsoft YaHei UI" : L"Bahnschrift";
    app.font = make_font(body, 14, FW_NORMAL);
    app.font_small = make_font(body, 12, FW_NORMAL);
    app.font_title = make_font(display, 23, FW_SEMIBOLD);
    app.font_brand = make_font(L"Bahnschrift", 20, FW_BOLD);
    app.font_number = make_font(display, 26, FW_SEMIBOLD);
    app.font_icon = make_font(L"Segoe MDL2 Assets", 15, FW_NORMAL);
    app.font_icon_big = make_font(L"Segoe MDL2 Assets", 18, FW_NORMAL);
    make_logo(px(46));
    for (int i = 0; i < BUTTON_COUNT; i++) SendMessageW(app.buttons[i], WM_SETFONT, (WPARAM)app.font, FALSE);
    SendMessageW(app.path_box, WM_SETFONT, (WPARAM)app.font, TRUE);
    SendMessageW(app.filter, WM_SETFONT, (WPARAM)app.font, TRUE);
    SendMessageW(app.count, WM_SETFONT, (WPARAM)app.font_small, TRUE);
    SendMessageW(app.summary, WM_SETFONT, (WPARAM)app.font, TRUE);
    SendMessageW(app.list, WM_SETFONT, (WPARAM)app.font, TRUE);
    SendMessageW(app.detail, WM_SETFONT, (WPARAM)app.font, TRUE);
    SendMessageW(app.header, WM_SETFONT, (WPARAM)app.font_title, TRUE);
}

/* The logo at the window's size: each pixel the average of those it covers. */
static void make_logo(int size) {
    enum { SOURCE = 96 };
    free(app.logo);
    app.logo = NULL;
    HINSTANCE self = GetModuleHandleW(NULL);
    HRSRC found = FindResourceW(self, MAKEINTRESOURCEW(2), MAKEINTRESOURCEW(10) /* RT_RCDATA */);
    HGLOBAL loaded = found ? LoadResource(self, found) : NULL;
    const BYTE *bytes = loaded ? LockResource(loaded) : NULL;
    if (!bytes || SizeofResource(self, found) != SOURCE * SOURCE * 4) return;
    app.logo = malloc((size_t)size * (size_t)size * sizeof(DWORD));
    if (!app.logo) return;
    app.logo_size = size;
    for (int y = 0; y < size; y++) {
        int y0 = y * SOURCE / size, y1 = (y + 1) * SOURCE / size;
        if (y1 <= y0) y1 = y0 + 1;
        for (int x = 0; x < size; x++) {
            int x0 = x * SOURCE / size, x1 = (x + 1) * SOURCE / size;
            if (x1 <= x0) x1 = x0 + 1;
            unsigned r = 0, g = 0, b = 0, n = 0;
            for (int sy = y0; sy < y1; sy++) {
                for (int sx = x0; sx < x1; sx++) {
                    const BYTE *p = bytes + ((size_t)sy * SOURCE + (size_t)sx) * 4;
                    b += p[0];
                    g += p[1];
                    r += p[2];
                    n++;
                }
            }
            app.logo[(size_t)y * (size_t)size + (size_t)x] = ((r / n) << 16) | ((g / n) << 8) | (b / n);
        }
    }
}

/* ---------- saved between runs ---------- */

/* A file in %APPDATA%\kebitx. */
static int settings_path(WCHAR *out, size_t room, const WCHAR *name, int create_folder) {
    const WCHAR *base = _wgetenv(L"APPDATA");
    if (!base) return -1;
    int n = _snwprintf(out, room, L"%s\\kebitx", base);
    if (n < 0 || (size_t)n + wcslen(name) + 2 >= room) return -1;
    if (create_folder) CreateDirectoryW(out, NULL);
    _snwprintf(out + n, room - (size_t)n, L"\\%s", name);
    out[room - 1] = L'\0';
    return 0;
}

static void load_settings(void) {
    WCHAR path[32768];
    if (settings_path(path, 32768, L"settings.txt", 0) != 0) return;
    FILE *f = _wfopen(path, L"rb");
    if (!f) return;
    char line[128];
    while (fgets(line, sizeof(line), f)) {
        line[strcspn(line, "\r\n")] = '\0';
        if (strncmp(line, "language=", 9) == 0) snprintf(app.language, sizeof(app.language), "%s", line + 9);
        if (strncmp(line, "theme=", 6) == 0) snprintf(app.theme, sizeof(app.theme), "%s", line + 6);
    }
    fclose(f);
}

static void save_settings(void) {
    if (app.capture_to) return; /* a test leaves no trace */
    WCHAR path[32768];
    if (settings_path(path, 32768, L"settings.txt", 1) != 0) return;
    FILE *f = _wfopen(path, L"wb");
    if (!f) return;
    fprintf(f, "language=%s\ntheme=%s\n", app.language, app.theme);
    fclose(f);
}

#define RECENT_MAX 6

static int recent_read(char lines[RECENT_MAX][4096]) {
    WCHAR path[32768];
    if (settings_path(path, 32768, L"recent.txt", 0) != 0) return 0;
    FILE *f = _wfopen(path, L"rb");
    if (!f) return 0;
    int n = 0;
    while (n < RECENT_MAX && fgets(lines[n], 4096, f)) {
        lines[n][strcspn(lines[n], "\r\n")] = '\0';
        if (lines[n][0]) n++;
    }
    fclose(f);
    return n;
}

static void recent_add(const char *utf8) {
    if (!utf8 || app.capture_to) return;
    static char lines[RECENT_MAX][4096];
    int n = recent_read(lines);
    WCHAR path[32768];
    if (settings_path(path, 32768, L"recent.txt", 1) != 0) return;
    FILE *f = _wfopen(path, L"wb");
    if (!f) return;
    fprintf(f, "%s\n", utf8);
    int kept = 1;
    for (int i = 0; i < n && kept < RECENT_MAX; i++) {
        if (strcmp(lines[i], utf8) != 0) {
            fprintf(f, "%s\n", lines[i]);
            kept++;
        }
    }
    fclose(f);
}

/* ---------- layout ---------- */

static const WCHAR *button_icon(int id) {
    switch (id) {
    case ID_HOME: return L"\xE80F";
    case ID_OPEN_FOLDER: return L"\xED25";
    case ID_OPEN_FILE: return L"\xE8E5";
    case ID_LANGUAGE: return L"\xE774";
    case ID_THEME: return L"\xE793";
    case ID_ANALYSE: return L"\xE768";
    case ID_PREVIEW: return L"\xE890";
    case ID_APPLY: return L"\xE90F";
    case ID_UNDO: return L"\xE7A7";
    case ID_SHOW: return L"\xED25";
    case ID_COPY: return L"\xE8C8";
    case ID_CANCEL: return L"\xE711";
    case ID_GROUP: return L"\xE8FD";
    case ID_PACKAGE: return L"\xE7B8";
    case ID_WORLD: return L"\xE909";
    default: return L"";
    }
}

/* Wide enough for its words in whatever language. */
static int action_width(int id) {
    if (app.compact && id >= ID_SHOW && id <= ID_PACKAGE) return px(40);
    WCHAR text[128];
    GetWindowTextW(BUTTON(id), text, 128);
    HDC dc = GetDC(app.window);
    HGDIOBJ old = SelectObject(dc, app.font);
    SIZE size = {0};
    GetTextExtentPoint32W(dc, text, (int)wcslen(text), &size);
    SelectObject(dc, old);
    ReleaseDC(app.window, dc);
    return size.cx + px(46);
}

static RECT rect_of(int x, int y, int w, int h) {
    RECT r = {x, y, x + w, y + h};
    return r;
}

static void place(HWND control, RECT r) {
    MoveWindow(control, r.left, r.top, r.right - r.left, r.bottom - r.top, TRUE);
    /* its text wraps anew: what was drawn before must not stay */
    InvalidateRect(control, NULL, TRUE);
}

static int place_rows(void);

/* A report has one long column; the places, a name and where it is. */
static void size_columns(void) {
    RECT area;
    GetClientRect(app.list, &area);
    int kind = px(170);
    int rest = area.right - kind - GetSystemMetricsForDpi(SM_CXVSCROLL, app.dpi);
    if (rest < px(100)) rest = px(100);
    int name = place_rows() ? rest * 3 / 5 : rest;
    if (app.home) name = rest * 2 / 5;
    LVCOLUMNW column = {LVCF_TEXT};
    column.pszText = (WCHAR *)T("Type");
    ListView_SetColumn(app.list, 0, &column);
    column.pszText = (WCHAR *)(place_rows() ? T("Name") : T("Finding"));
    ListView_SetColumn(app.list, 1, &column);
    column.pszText = (WCHAR *)(app.inspecting ? T("Amount") : T("Folder"));
    ListView_SetColumn(app.list, 2, &column);
    ListView_SetColumnWidth(app.list, 0, kind);
    ListView_SetColumnWidth(app.list, 1, name);
    ListView_SetColumnWidth(app.list, 2, place_rows() ? rest - name : 0);
}

static void layout(void) {
    RECT area;
    GetClientRect(app.window, &area);
    int w = area.right, h = area.bottom;
    int rail = px(212), m = px(16), gap = px(12);

    /* the rail */
    int nav_x = px(10), nav_w = rail - px(20), nav_h = px(38);
    int y = px(92);
    for (int id = ID_HOME; id <= ID_OPEN_FILE; id++) {
        place(BUTTON(id), rect_of(nav_x, y, nav_w, nav_h));
        y += nav_h + px(4);
    }
    int low = h - px(16) - nav_h * 2 - px(4);
    place(BUTTON(ID_LANGUAGE), rect_of(nav_x, low, nav_w, nav_h));
    place(BUTTON(ID_THEME), rect_of(nav_x, low + nav_h + px(4), nav_w, nav_h));

    /* where */
    int x = rail + m, wide_ = w - x - m;
    y = m;
    int analyse_w = action_width(ID_ANALYSE);
    int top_h = px(40);
    app.path_card = rect_of(x, y, wide_ - analyse_w - px(8), top_h);
    place(app.path_box, rect_of(x + px(36), y + px(10), wide_ - analyse_w - px(8) - px(46), px(22)));
    place(BUTTON(ID_ANALYSE), rect_of(x + wide_ - analyse_w, y, analyse_w, top_h));
    y += top_h + gap;

    /* what it is */
    int hero_h = px(104);
    app.hero_card = rect_of(x, y, wide_, hero_h);
    place(app.header, rect_of(x + px(52), y + px(12), wide_ - px(68), px(32)));
    place(app.summary, rect_of(x + px(52), y + px(48), wide_ - px(68), px(44)));
    y += hero_h + gap;

    /* how much of each kind; each also narrows the list */
    int tile_h = px(68), tile_gap = px(10);
    int tile_w = (wide_ - tile_gap * (TILES - 1)) / TILES;
    for (int i = 0; i < TILES; i++) place(BUTTON(ID_TILE_0 + i), rect_of(x + i * (tile_w + tile_gap), y, tile_w, tile_h));
    y += tile_h + gap;

    /* narrowing what is in it */
    int count_w = px(60), filter_h = px(34), group_w = action_width(ID_GROUP);
    int filter_w = wide_ - count_w - group_w - px(16);
    app.filter_card = rect_of(x, y, filter_w, filter_h);
    place(app.filter, rect_of(x + px(36), y + px(7), filter_w - px(46), px(21)));
    place(BUTTON(ID_GROUP), rect_of(x + filter_w + px(8), y, group_w, filter_h));
    place(app.count, rect_of(x + wide_ - count_w, y + px(9), count_w, px(18)));
    y += filter_h + px(8);

    /* what is in it, and what to do about it, at the end of the reading */
    int bar_h = px(38), detail_h = px(70);
    int list_h = h - y - gap - detail_h - gap - bar_h - m;
    if (list_h < px(90)) list_h = px(90);
    app.list_card = rect_of(x, y, wide_, list_h);
    place(app.list, rect_of(x + 1, y + px(4), wide_ - 2, list_h - px(8)));
    y += list_h + gap;
    app.detail_card = rect_of(x, y, wide_, detail_h);
    place(app.detail, rect_of(x + px(12), y + px(8), wide_ - px(20), detail_h - px(16)));
    y += detail_h + gap;

    /* if the words do not fit, the secondary ones become icons; their tips say */
    app.compact = 0;
    int needed = 0;
    for (int id = ID_PREVIEW; id <= ID_WORLD; id++) {
        if (id != ID_WORLD || app.can_inspect) needed += action_width(id) + px(8);
    }
    app.compact = needed > wide_;
    int bx = x;
    for (int id = ID_PREVIEW; id <= ID_WORLD; id++) {
        if (id == ID_WORLD && !app.can_inspect) continue;
        int bw = action_width(id);
        place(BUTTON(id), rect_of(bx, y, bw, bar_h));
        bx += bw + px(8);
    }
    int cancel_w = action_width(ID_CANCEL);
    place(BUTTON(ID_CANCEL), rect_of(x + wide_ - cancel_w, y, cancel_w, bar_h));

    size_columns();
    InvalidateRect(app.window, NULL, TRUE);
}

/* What can be pressed now. */
static void update_buttons(void) {
    int ready = app.engine.report != NULL && !app.busy;
    int open = ready && app.path != NULL;
    EnableWindow(BUTTON(ID_HOME), ready);
    EnableWindow(BUTTON(ID_ANALYSE), ready && GetWindowTextLengthW(app.path_box) > 0);
    EnableWindow(BUTTON(ID_OPEN_FOLDER), ready);
    EnableWindow(BUTTON(ID_OPEN_FILE), ready);
    EnableWindow(BUTTON(ID_LANGUAGE), !app.busy);
    EnableWindow(BUTTON(ID_PREVIEW), open);
    EnableWindow(BUTTON(ID_APPLY), ready && app.fingerprint != NULL);
    EnableWindow(BUTTON(ID_UNDO), ready && app.backup != NULL);
    EnableWindow(BUTTON(ID_SHOW), ready && (app.path != NULL || (app.home && ListView_GetNextItem(app.list, -1, LVNI_SELECTED) >= 0)));
    EnableWindow(BUTTON(ID_COPY), ready && app.has_report);
    EnableWindow(BUTTON(ID_WORLD), ready && app.path && app.can_inspect && !app.inspecting);
    EnableWindow(BUTTON(ID_GROUP), ready && app.has_report && app.cause_count > 0);
    EnableWindow(BUTTON(ID_PACKAGE), ready && app.has_report && app.path && app.current.package);
    /* an analysis can stop; a repair half done must not */
    int can_cancel = app.busy && (app.running == JOB_REPORT || app.running == JOB_WORLD);
    EnableWindow(BUTTON(ID_CANCEL), can_cancel);
    ShowWindow(BUTTON(ID_CANCEL), can_cancel ? SW_SHOWNA : SW_HIDE);
    /* while it works, those have nothing to do, and Cancel needs their place */
    for (int id = ID_SHOW; id <= ID_PACKAGE; id++) ShowWindow(BUTTON(id), app.busy ? SW_HIDE : SW_SHOWNA);
    /* only a world with saved data has it */
    ShowWindow(BUTTON(ID_WORLD), !app.busy && app.can_inspect ? SW_SHOWNA : SW_HIDE);
    for (int id = ID_PREVIEW; id <= ID_CANCEL; id++) InvalidateRect(BUTTON(id), NULL, FALSE);
}

/* ---------- the rows, through the tile and the filter ---------- */

static int place_rows(void) { return app.home || app.inspecting; }

static int use_causes(void) { return !place_rows() && app.has_report && app.by_cause && app.cause_count > 0; }

static size_t row_total(void) {
    if (place_rows()) return app.place_count;
    if (use_causes()) return app.cause_count;
    return app.has_report ? app.current.row_count : 0;
}

static const char *place_group(const char *what) {
    if (strcmp(what, "World") == 0) return "World";
    if (strstr(what, "ehavior")) return "Behavior";
    if (strstr(what, "esource")) return "Resource";
    return "Other";
}

static const char *row_type_key(size_t i) {
    if (place_rows()) return app.places[i].what;
    if (use_causes()) return app.causes[i].is_problem ? app.row_label : "Does nothing";
    const Row *row = &app.current.rows[i];
    return row->is_problem ? app.row_label : row->is_dead ? "Does nothing" : "Note";
}

/* Which tile a row counts for, besides All. */
static int row_tile(size_t i) {
    if (app.inspecting) {
        const char *what = app.places[i].what;
        return strcmp(what, "Block") == 0 ? 1 : strcmp(what, "Entity") == 0 ? 2 : 3;
    }
    if (app.home) {
        const char *group = place_group(app.places[i].what);
        return strcmp(group, "World") == 0 ? 1 : strcmp(group, "Behavior") == 0 ? 2 : strcmp(group, "Resource") == 0 ? 3 : 0;
    }
    if (use_causes()) return app.causes[i].is_problem ? 1 : 2;
    const Row *row = &app.current.rows[i];
    return row->is_problem ? 1 : row->is_dead ? 2 : 3;
}

static int row_matches(size_t i, const WCHAR *wanted) {
    if (app.tile && row_tile(i) != app.tile) return 0;
    if (!wanted[0] || StrStrIW(T(row_type_key(i)), wanted)) return 1;
    if (place_rows()) return StrStrIW(app.places[i].name, wanted) || StrStrIW(app.places[i].path, wanted);
    if (use_causes()) return StrStrIW(app.causes[i].line, wanted) || StrStrIW(app.causes[i].fix, wanted) || StrStrIW(app.causes[i].places, wanted);
    return app.row_text[i] && StrStrIW(app.row_text[i], wanted);
}

static void set_tile_labels(void) {
    static const char *const report_tiles[TILES] = {"All", "Problems", "Do nothing", "Notes"};
    static const char *const home_tiles[TILES] = {"All", "Worlds", "Behavior packs", "Resource packs"};
    static const char *const world_tiles[TILES] = {"All", "Block types", "Entity types", "Block entity types"};
    for (int i = 0; i < TILES; i++) {
        SetWindowTextW(BUTTON(ID_TILE_0 + i), T(app.inspecting ? world_tiles[i] : app.home ? home_tiles[i] : report_tiles[i]));
        InvalidateRect(BUTTON(ID_TILE_0 + i), NULL, FALSE);
    }
}

static void count_tiles(void) {
    memset(app.tile_counts, 0, sizeof(app.tile_counts));
    size_t total = row_total();
    for (size_t i = 0; i < total; i++) {
        /* a cause counts every finding it stands for: grouped or not, the same */
        size_t weight = use_causes() ? (size_t)app.causes[i].count : 1;
        int tile = row_tile(i);
        app.tile_counts[0] += weight;
        if (tile) app.tile_counts[tile] += weight;
    }
    app.tiles_known = place_rows() || app.has_report;
    set_tile_labels();
}

/* The list again, as the tile and the filter say. The rows chosen no longer mean
 * the same. */
static void rebuild_view(void) {
    WCHAR wanted[256] = L"";
    GetWindowTextW(app.filter, wanted, 256);
    size_t total = row_total();
    free(app.view);
    app.view = malloc((total ? total : 1) * sizeof(size_t));
    app.view_count = 0;
    for (size_t i = 0; app.view && i < total; i++) {
        if (row_matches(i, wanted)) app.view[app.view_count++] = i;
    }
    ListView_SetItemState(app.list, -1, 0, LVIS_SELECTED);
    ListView_SetItemCountEx(app.list, (int)app.view_count, 0);
    InvalidateRect(app.list, NULL, TRUE);
    WCHAR count[64] = L"";
    if (total && (wanted[0] || app.tile)) _snwprintf(count, 64, L"%zu / %zu", app.view_count, total);
    else if (total) _snwprintf(count, 64, L"%zu", total);
    count[63] = L'\0';
    SetWindowTextW(app.count, count);
    InvalidateRect(app.count, NULL, TRUE);
    SetWindowTextW(app.detail, L"");
}

static void set_group_label(void) {
    SetWindowTextW(BUTTON(ID_GROUP), app.by_cause ? T("Show all findings") : T("Group by cause"));
    InvalidateRect(BUTTON(ID_GROUP), NULL, FALSE);
}

static void refresh_list(void) {
    app.tile = 0;
    SetWindowTextW(app.filter, L"");
    count_tiles();
    size_columns();
    rebuild_view();
}

/* ---------- what is on screen ---------- */

static void set_verdict(Verdict v) {
    app.verdict = v;
    RECT hero = app.hero_card;
    InvalidateRect(app.window, &hero, TRUE);
    InvalidateRect(app.header, NULL, TRUE);
}

static void forget_report(void) {
    ListView_SetItemCountEx(app.list, 0, 0);
    app.view_count = 0;
    app.tiles_known = 0;
    SetWindowTextW(app.count, L"");
    app.verdict = VERDICT_NONE;
    app.home = 0;
    app.inspecting = 0;
    for (size_t i = 0; i < app.cause_count; i++) {
        free(app.causes[i].what);
        free(app.causes[i].fix);
        free(app.causes[i].places);
        free(app.causes[i].line);
    }
    free(app.causes);
    app.causes = NULL;
    app.cause_count = 0;
    if (app.has_report) {
        for (size_t i = 0; i < app.current.row_count; i++) free(app.row_text[i]);
        free(app.row_text);
        app.row_text = NULL;
        report_free(&app.current);
        app.has_report = 0;
    }
    SetWindowTextW(app.detail, L"");
    for (int i = 0; i < TILES; i++) InvalidateRect(BUTTON(ID_TILE_0 + i), NULL, FALSE);
}

/* The engine's causes: count\twhat\tfix\tplaces. */
static void keep_causes(void) {
    if (!app.current.cause_count) return;
    app.causes = calloc(app.current.cause_count, sizeof(Cause));
    if (!app.causes) return;
    for (size_t i = 0; i < app.current.cause_count; i++) {
        const Row *row = &app.current.causes[i];
        WCHAR *text = wide(row->text);
        WCHAR *what = text ? wcschr(text, L'\t') : NULL;
        WCHAR *fix = what ? wcschr(what + 1, L'\t') : NULL;
        WCHAR *places = fix ? wcschr(fix + 1, L'\t') : NULL;
        if (places) {
            *what++ = L'\0';
            *fix++ = L'\0';
            *places++ = L'\0';
            Cause c = {_wtoi(text), row->is_problem, row->is_dead, copy_wide(what), copy_wide(fix), copy_wide(places), NULL};
            size_t size = wcslen(what) + 24;
            c.line = malloc(size * sizeof(WCHAR));
            if (c.what && c.fix && c.places && c.line) {
                if (c.count > 1) _snwprintf(c.line, size, L"%d \x00D7  %s", c.count, what);
                else _snwprintf(c.line, size, L"%s", what);
                c.line[size - 1] = L'\0';
                app.causes[app.cause_count++] = c;
            } else {
                free(c.what);
                free(c.fix);
                free(c.places);
                free(c.line);
            }
        }
        free(text);
    }
}

/* Keeps the report for the list to read; returns it, or NULL when the text is
 * not one. */
static const Report *keep_report(const char *json, const char *row_label) {
    forget_report();
    Report r;
    if (report_parse(json, &r) != 0) return NULL;
    app.row_text = calloc(r.row_count ? r.row_count : 1, sizeof(WCHAR *));
    if (!app.row_text) {
        report_free(&r);
        return NULL;
    }
    for (size_t i = 0; i < r.row_count; i++) app.row_text[i] = wide(r.rows[i].text);
    app.current = r;
    app.has_report = 1;
    app.row_label = row_label;
    keep_causes();
    refresh_list();
    return &app.current;
}

static void show_failure(const WCHAR *what, const WCHAR *why) {
    SetWindowTextW(app.header, what);
    SetWindowTextW(app.summary, why);
    set_verdict(VERDICT_NOTHING);
}

static void show_analysis(const char *json) {
    const Report *r = keep_report(json, "Problem");
    if (!r) {
        show_failure(T("The engine could not read that"), T("Check the path: it must be a folder, or a .jar or .zip file."));
        replace_text(&app.path, NULL);
        return;
    }
    app.can_inspect = r->inspect;
    if (r->nothing) {
        set_verdict(VERDICT_NOTHING);
        set_text(app.header, r->kind);
        set_text(app.summary, r->summary);
        app.tiles_known = 0;
        for (int i = 0; i < TILES; i++) InvalidateRect(BUTTON(ID_TILE_0 + i), NULL, FALSE);
        SetWindowTextW(app.count, L"");
        if (!r->inspect) replace_text(&app.path, NULL);
        else recent_add(app.path);
        return;
    }
    if (r->cancelled) {
        show_failure(T("Analysis cancelled"), T("Nothing was changed."));
        return;
    }
    size_t dead = 0;
    for (size_t i = 0; i < r->row_count; i++) dead += r->rows[i].is_dead ? 1 : 0;
    WCHAR number[32], line[256], extra[128] = L"";
    _snwprintf(number, 32, L"%ld", r->broken);
    if (r->broken == 0) wcsncpy(line, T("OK: nothing proven broken"), 255);
    else if (r->broken == 1) wcsncpy(line, T("1 problem"), 255);
    else fill_in(line, 256, T("{0} problems"), 1, number);
    line[255] = L'\0';
    if (dead == 1) {
        wcsncpy(extra, T("1 that does nothing"), 127);
        extra[127] = L'\0';
    } else if (dead) {
        _snwprintf(number, 32, L"%zu", dead);
        fill_in(extra, 128, T("{0} that do nothing"), 1, number);
    }
    WCHAR *kind = wide(r->kind);
    size_t size = (kind ? wcslen(kind) : 0) + wcslen(line) + wcslen(extra) + 16;
    WCHAR *head = malloc(size * sizeof(WCHAR));
    if (head) {
        _snwprintf(head, size, extra[0] ? L"%s: %s, %s" : L"%s: %s%s", kind ? kind : L"", line, extra);
        head[size - 1] = L'\0';
        SetWindowTextW(app.header, head);
        free(head);
    }
    free(kind);
    set_verdict(r->broken == 0 ? VERDICT_GOOD : VERDICT_BAD);
    recent_add(app.path);
    set_text(app.summary, r->summary);
}

static void show_plan(const char *json) {
    const Report *r = keep_report(json, "Change");
    if (!r) {
        show_failure(T("The engine could not plan a repair"), T("Nothing was changed."));
        return;
    }
    if (r->row_count == 0) {
        SetWindowTextW(app.header, T("Nothing to repair"));
        set_verdict(VERDICT_NOTHING);
    } else {
        WCHAR number[32], head[256];
        _snwprintf(number, 32, L"%zu", r->row_count);
        if (r->row_count == 1) {
            wcsncpy(head, T("Repair preview: 1 change. Nothing has been written."), 255);
            head[255] = L'\0';
        } else {
            fill_in(head, 256, T("Repair preview: {0} changes. Nothing has been written."), 1, number);
        }
        SetWindowTextW(app.header, head);
        replace_text(&app.fingerprint, r->fingerprint);
        set_verdict(VERDICT_NONE);
    }
    set_text(app.summary, r->summary);
}

/* The outcome of a repair or of its undo, in the engine's words. */
static void show_outcome(const char *json, const WCHAR *when_unreadable) {
    const Report *r = keep_report(json, "New problem");
    if (!r) {
        show_failure(when_unreadable, T("Check the folder: the engine gave no answer."));
        return;
    }
    set_text(app.header, r->kind);
    set_text(app.summary, r->summary);
    if (r->kept) replace_text(&app.backup, r->backup);
    set_verdict(r->kept ? VERDICT_GOOD : VERDICT_NONE);
}

static void show_package(const char *json) {
    const Report *r = keep_report(json, "Problem");
    if (!r) {
        show_failure(T("Could not create the package"), T("Check the folder: the engine gave no answer."));
        return;
    }
    set_text(app.header, r->kind);
    set_text(app.summary, r->summary);
    app.tiles_known = 0;
    for (int i = 0; i < TILES; i++) InvalidateRect(BUTTON(ID_TILE_0 + i), NULL, FALSE);
    SetWindowTextW(app.count, L"");
    set_verdict(r->package_file ? VERDICT_GOOD : VERDICT_NOTHING);
}

static void free_place_rows(void) {
    for (size_t i = 0; i < app.place_count; i++) {
        free(app.places[i].what);
        free(app.places[i].name);
        free(app.places[i].path);
    }
    free(app.places);
    app.places = NULL;
    app.place_count = 0;
}

static void free_places(void) {
    free_place_rows();
    for (int i = 0; i < 2; i++) {
        free(app.shortcuts[i]);
        app.shortcuts[i] = NULL;
    }
}

/* One note of the engine, as what\tname\tpath. */
static void add_place(char *note) {
    char *name = strchr(note, '\t');
    char *path = name ? strchr(name + 1, '\t') : NULL;
    if (!path) return;
    *name++ = '\0';
    *path++ = '\0';
    if (strcmp(note, "Shortcut") == 0) {
        int slot = app.shortcuts[0] ? 1 : 0;
        if (!app.shortcuts[slot]) app.shortcuts[slot] = wide(path);
        return;
    }
    Place place_ = {copy_text(note), wide(name), wide(path)};
    if (place_.what && place_.name && place_.path) {
        app.places[app.place_count++] = place_;
    } else {
        free(place_.what);
        free(place_.name);
        free(place_.path);
    }
}

/* What a world has saved: rows of the same shape as the places. */
static void show_world(const char *json) {
    const Report *r = keep_report(json, "Note");
    if (!r) {
        show_failure(T("Could not read the world"), T("Check the folder: the engine gave no answer."));
        return;
    }
    set_text(app.header, r->kind);
    set_text(app.summary, r->summary);
    if (r->cancelled) {
        show_failure(T("Analysis cancelled"), T("Nothing was changed."));
        return;
    }
    if (!r->world_view) {
        set_verdict(VERDICT_NOTHING);
        return;
    }
    free_place_rows();
    app.places = calloc(r->row_count + 1, sizeof(Place));
    for (size_t i = 0; app.places && i < r->row_count; i++) {
        char *copy = copy_text(r->rows[i].text);
        if (copy) add_place(copy);
        free(copy);
    }
    /* 381738682 cannot be read at a glance: digits as this computer groups them */
    for (size_t i = 0; i < app.place_count; i++) {
        NUMBERFMTW format = {0};
        WCHAR grouping[8] = L"", separator[8] = L",", decimal[8] = L".";
        GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_STHOUSAND, separator, 8);
        GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_SDECIMAL, decimal, 8);
        (void)grouping;
        format.Grouping = 3;
        format.lpDecimalSep = decimal;
        format.lpThousandSep = separator;
        WCHAR grouped[64];
        if (GetNumberFormatEx(LOCALE_NAME_USER_DEFAULT, 0, app.places[i].path, &format, grouped, 64) > 0) {
            WCHAR *kept = copy_wide(grouped);
            if (kept) {
                free(app.places[i].path);
                app.places[i].path = kept;
            }
        }
    }
    free(app.world_details);
    app.world_details = NULL;
    if (r->details) {
        /* one per line, as Windows wants lines */
        size_t size = strlen(r->details) * 2 + 1;
        char *lines = malloc(size);
        if (lines) {
            char *to = lines;
            for (const char *from = r->details; *from; from++) {
                if (*from == '\n') *to++ = '\r';
                *to++ = *from;
            }
            *to = '\0';
            app.world_details = wide(lines);
            free(lines);
        }
    }
    app.inspecting = 1;
    refresh_list();
    if (app.world_details) SetWindowTextW(app.detail, app.world_details);
    set_verdict(VERDICT_GOOD);
}

/* What was opened lately and is still there comes first. */
static void add_recent_places(void) {
    static char lines[RECENT_MAX][4096];
    int n = recent_read(lines);
    for (int i = 0; i < n; i++) {
        WCHAR *path = wide(lines[i]);
        if (path && GetFileAttributesW(path) != INVALID_FILE_ATTRIBUTES) {
            const WCHAR *slash = wcsrchr(path, L'\\');
            Place place_ = {copy_text("Recent"), copy_wide(slash && slash[1] ? slash + 1 : path), path};
            if (place_.what && place_.name) {
                app.places[app.place_count++] = place_;
                continue;
            }
            free(place_.what);
            free(place_.name);
        }
        free(path);
    }
}

/* The first screen: what is on this computer, ready to open. */
static void show_places(const char *json) {
    free_places();
    forget_report();
    SetWindowTextW(app.header, T("What do you want to check?"));
    Report r;
    if (report_parse(json, &r) == 0) {
        app.places = calloc(r.row_count + RECENT_MAX + 1, sizeof(Place));
        if (app.places) add_recent_places();
        for (size_t i = 0; app.places && i < r.row_count; i++) add_place(r.rows[i].text);
        report_free(&r);
    }
    app.home = 1;
    refresh_list();
    SetWindowTextW(app.summary, app.place_count
                                    ? T("These are on this computer: double-click one, or press Enter. For anything else, type or paste a path, choose a folder or a file, or drop one on this window.")
                                    : T("Nothing to check in the game's usual places. Type or paste a path, choose a folder or a file, or drop one on this window."));
}

/* The window as a 32-bit top-down BMP. One colour everywhere means nothing was
 * drawn: that is a failure. */
static int save_capture(const WCHAR *path) {
    RECT area;
    GetClientRect(app.window, &area);
    int w = area.right, h = area.bottom;
    HDC screen = GetDC(app.window);
    Canvas c;
    int ok = -1;
    if (canvas_begin(&c, screen, w, h) == 0) {
        size_t count = (size_t)w * (size_t)h;
        int drawn = 0;
        if (PrintWindow(app.window, c.dc, PW_CLIENTONLY | 2 /* PW_RENDERFULLCONTENT */)) {
            GdiFlush();
            for (size_t i = 1; i < count && !drawn; i++) drawn = c.pixels[i] != c.pixels[0];
        }
        if (drawn) {
            FILE *f = _wfopen(path, L"wb");
            if (f) {
                DWORD size = (DWORD)w * (DWORD)h * 4;
                BITMAPFILEHEADER head = {0};
                BITMAPINFOHEADER info = {0};
                info.biSize = sizeof(info);
                info.biWidth = w;
                info.biHeight = -h;
                info.biPlanes = 1;
                info.biBitCount = 32;
                info.biCompression = BI_RGB;
                head.bfType = 0x4D42;
                head.bfOffBits = sizeof(head) + sizeof(info);
                head.bfSize = head.bfOffBits + size;
                if (fwrite(&head, sizeof(head), 1, f) == 1 && fwrite(&info, sizeof(info), 1, f) == 1 &&
                    fwrite(c.pixels, size, 1, f) == 1)
                    ok = 0;
                fclose(f);
            }
        }
        SelectObject(c.dc, c.old);
        DeleteObject(c.bitmap);
        DeleteDC(c.dc);
    }
    ReleaseDC(app.window, screen);
    return ok;
}

/* ---------- the engine, on its own thread ---------- */

/* On the engine's thread: post the step, and say whether to go on. */
static int on_progress(const char *stage, unsigned done, unsigned total, void *user) {
    (void)user;
    WCHAR *text = wide(stage);
    if (text) {
        /* the window frees it; a window already gone frees nothing, and the
           process is ending then */
        if (!PostMessageW(app.window, WM_APP_PROGRESS, MAKEWPARAM(done, total), (LPARAM)text)) free(text);
    }
    return InterlockedCompareExchange(&app.cancel, 0, 0) != 0;
}

static DWORD WINAPI run_job(void *arg) {
    Job *job = arg;
    char *json = NULL;
    switch (job->kind) {
    case JOB_REPORT: json = app.engine.report(job->path, on_progress, NULL); break;
    case JOB_PLAN: json = app.engine.plan(job->path); break;
    case JOB_APPLY: json = app.engine.apply(job->path, job->fingerprint); break;
    case JOB_UNDO: json = app.engine.undo(job->path); break;
    case JOB_PLACES: json = app.engine.places(); break;
    case JOB_PACKAGE: json = app.engine.package(job->path, job->fingerprint); break;
    case JOB_WORLD: json = app.engine.world(job->path, on_progress, NULL); break;
    }
    PostMessageW(app.window, WM_APP_DONE, (WPARAM)job->kind, (LPARAM)json);
    free(job->path);
    free(job->fingerprint);
    free(job);
    return 0;
}

static int animations_on(void) {
    BOOL on = TRUE;
    SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &on, 0);
    return on && !app.capture_to;
}

static void start_job(JobKind kind, const char *path, const char *fingerprint, const WCHAR *saying) {
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
        show_failure(T("Out of memory"), T("Nothing was changed."));
        return;
    }
    InterlockedExchange(&app.cancel, 0);
    forget_report();
    SetWindowTextW(app.header, saying);
    SetWindowTextW(app.summary, L"");
    set_verdict(VERDICT_NONE);
    app.busy = 1;
    app.running = kind;
    app.step = 0;
    app.steps_total = 0;
    update_buttons();
    SetCursor(LoadCursorW(NULL, IDC_WAIT));
    if (animations_on()) SetTimer(app.window, TIMER_ANIMATION, 33, NULL);
    HANDLE thread = CreateThread(NULL, 0, run_job, job, 0, NULL);
    if (thread) {
        CloseHandle(thread);
    } else {
        free(job->path);
        free(job->fingerprint);
        free(job);
        app.busy = 0;
        KillTimer(app.window, TIMER_ANIMATION);
        update_buttons();
        show_failure(T("Could not start"), T("Nothing was changed."));
    }
}

static void analyse_again(void) {
    /* what was previewed may not hold after this */
    replace_text(&app.fingerprint, NULL);
    start_job(JOB_REPORT, app.path, NULL, T("Analysing..."));
}

static void open_path(const WCHAR *given) {
    if (app.busy) return;
    /* the whole path: a relative one means nothing tomorrow */
    WCHAR path[32768];
    DWORD got = GetFullPathNameW(given, 32768, path, NULL);
    if (got == 0 || got >= 32768) {
        wcsncpy(path, given, 32767);
        path[32767] = L'\0';
    }
    char *utf8 = narrow(path);
    if (!utf8) {
        show_failure(T("That path cannot be read as text"), path);
        return;
    }
    free(app.path);
    app.path = utf8;
    app.can_inspect = 0;
    SetWindowTextW(app.path_box, path);
    /* another folder's backup is not undone from here: its path stays on the
       report that gave it, for `kebitx repair --undo` */
    replace_text(&app.backup, NULL);
    analyse_again();
}

/* What is in the path box, as "Copy as path" gives it too: without its quotes and
 * the spaces around them. The same path again keeps its backup to undo. */
static void analyse_typed(void) {
    if (app.busy) return;
    int n = GetWindowTextLengthW(app.path_box);
    if (n == 0) return;
    WCHAR *text = malloc(((size_t)n + 1) * sizeof(WCHAR));
    if (!text) return;
    GetWindowTextW(app.path_box, text, n + 1);
    WCHAR *start = text;
    while (*start == L' ' || *start == L'"') start++;
    size_t len = wcslen(start);
    while (len > 0 && (start[len - 1] == L' ' || start[len - 1] == L'"')) start[--len] = L'\0';
    if (len > 0) {
        WCHAR full[32768];
        DWORD got = GetFullPathNameW(start, 32768, full, NULL);
        char *utf8 = narrow(got && got < 32768 ? full : start);
        int same = utf8 && app.path && strcmp(utf8, app.path) == 0;
        free(utf8);
        if (same) {
            SetWindowTextW(app.path_box, full);
            analyse_again();
        } else {
            open_path(start);
        }
    }
    free(text);
}

static void inspect_world(void) {
    if (app.busy || !app.path || !app.can_inspect) return;
    start_job(JOB_WORLD, app.path, NULL, T("Reading the world..."));
}

static void go_home(void) {
    if (app.busy) return;
    app.can_inspect = 0;
    replace_text(&app.path, NULL);
    replace_text(&app.fingerprint, NULL);
    SetWindowTextW(app.path_box, L"");
    start_job(JOB_PLACES, "", NULL, T("Looking for Minecraft..."));
}

/* The first row: its full text goes to the box below. */
static void select_first_row(void) {
    ListView_SetItemState(app.list, 0, LVIS_SELECTED | LVIS_FOCUSED, LVIS_SELECTED | LVIS_FOCUSED);
}

static void preview_repair(void) {
    replace_text(&app.fingerprint, NULL);
    start_job(JOB_PLAN, app.path, NULL, T("Planning the repair..."));
}

static int confirm(const WCHAR *question, const WCHAR *details) {
    size_t size = wcslen(question) + wcslen(details) + 8;
    WCHAR *text = malloc(size * sizeof(WCHAR));
    if (!text) return 0;
    _snwprintf(text, size, L"%s\n\n%s", question, details);
    text[size - 1] = L'\0';
    int yes = MessageBoxW(app.window, text, L"Kebitx", MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2) == IDYES;
    free(text);
    return yes;
}

static void apply_repair(int ask) {
    if (!app.fingerprint) return;
    if (ask && !confirm(T("Apply the changes in the preview?"),
                        T("Every file is backed up first. The folder is analysed again, and the result stays only if it is strictly better.")))
        return;
    char *fingerprint = app.fingerprint;
    app.fingerprint = NULL;
    start_job(JOB_APPLY, app.path, fingerprint, T("Repairing..."));
    free(fingerprint);
}

static void undo_repair(int ask) {
    if (!app.backup) return;
    if (ask && !confirm(T("Undo the repair?"), T("Every file it changed goes back to how it was before, including any change made to those files since.")))
        return;
    char *backup = app.backup;
    app.backup = NULL;
    replace_text(&app.fingerprint, NULL);
    start_job(JOB_UNDO, backup, NULL, T("Undoing the repair..."));
    free(backup);
}

/* The system's file dialog. Its GUIDs here: asking for them by name would bring in
 * a library just for these three constants. */
static const CLSID file_open_dialog = {0xDC1C5A9C, 0xE88A, 0x4DDE, {0xA5, 0xA1, 0x60, 0xF8, 0x2A, 0x20, 0xAE, 0xF7}};
static const IID file_open_iface = {0xD57C7288, 0xD4AD, 0x4768, {0xBE, 0x02, 0x9D, 0x96, 0x95, 0x32, 0xD9, 0x60}};
static const IID shell_item_iface = {0x43826D1E, 0xE718, 0x42EE, {0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE}};
/* so each dialog remembers where it was last, apart from the other */
static const GUID folder_memory = {0x6B7E2C10, 0x4F3A, 0x4E21, {0x9C, 0x1D, 0x2A, 0x51, 0x7E, 0x90, 0x11, 0xA1}};
static const GUID file_memory = {0x6B7E2C11, 0x4F3A, 0x4E21, {0x9C, 0x1D, 0x2A, 0x51, 0x7E, 0x90, 0x11, 0xA2}};

/* Asks for a folder or a file, and opens it. */
static void choose(int folder) {
    if (app.busy) return;
    IFileOpenDialog *dialog = NULL;
    HRESULT hr = CoCreateInstance(&file_open_dialog, NULL, CLSCTX_INPROC_SERVER, &file_open_iface, (void **)&dialog);
    if (FAILED(hr)) {
        show_failure(T("Windows could not open its file dialog"), T("Type or paste the path above, or drop the folder here."));
        return;
    }
    FILEOPENDIALOGOPTIONS options = 0;
    IFileOpenDialog_GetOptions(dialog, &options);
    options |= FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | (folder ? FOS_PICKFOLDERS : FOS_FILEMUSTEXIST);
    IFileOpenDialog_SetOptions(dialog, options);
    IFileOpenDialog_SetClientGuid(dialog, folder ? &folder_memory : &file_memory);
    IFileOpenDialog_SetTitle(dialog, folder ? T("Choose a pack, a world or a mods folder") : T("Choose a mod or a pack"));
    IFileOpenDialog_SetOkButtonLabel(dialog, T("Analyse"));
    if (!folder) {
        COMDLG_FILTERSPEC types[] = {{T("Mods and packs (*.jar, *.zip)"), L"*.jar;*.zip"}, {T("All files"), L"*.*"}};
        IFileOpenDialog_SetFileTypes(dialog, 2, types);
    }
    /* the game's folders in the side bar, and the first of them to start from,
       until the dialog has somewhere of its own to remember */
    for (int i = 0; i < 2 && app.shortcuts[i]; i++) {
        IShellItem *where = NULL;
        if (SUCCEEDED(SHCreateItemFromParsingName(app.shortcuts[i], NULL, &shell_item_iface, (void **)&where))) {
            IFileOpenDialog_AddPlace(dialog, where, FDAP_TOP);
            if (i == 0) IFileOpenDialog_SetDefaultFolder(dialog, where);
            IShellItem_Release(where);
        }
    }
    if (SUCCEEDED(IFileOpenDialog_Show(dialog, app.window))) {
        IShellItem *item = NULL;
        if (SUCCEEDED(IFileOpenDialog_GetResult(dialog, &item))) {
            WCHAR *path = NULL;
            if (SUCCEEDED(IShellItem_GetDisplayName(item, SIGDN_FILESYSPATH, &path))) {
                open_path(path);
                CoTaskMemFree(path);
            }
            IShellItem_Release(item);
        }
    }
    IFileOpenDialog_Release(dialog);
}

static const CLSID file_save_dialog = {0xC0B4E2F3, 0xBA21, 0x4773, {0x8D, 0xBA, 0x33, 0x5E, 0xC9, 0x46, 0xEB, 0x8B}};
static const IID file_save_iface = {0x84BCCD23, 0x5FDE, 0x4CDB, {0xAE, 0xA4, 0xAF, 0x64, 0xB8, 0x3D, 0x78, 0xAB}};

static void package_to(const WCHAR *file) {
    char *out = narrow(file);
    if (!out) return;
    start_job(JOB_PACKAGE, app.path, out, T("Creating the package..."));
    free(out);
}

/* One file for the game, after saying what is wrong with it, if anything is. */
static void package_now(void) {
    if (app.busy || !app.has_report || !app.path || !app.current.package) return;
    if (app.current.broken > 0) {
        WCHAR number[32], question[256];
        _snwprintf(number, 32, L"%ld", app.current.broken);
        if (app.current.broken == 1) wcsncpy(question, T("It has 1 problem. Package it anyway?"), 255);
        else fill_in(question, 256, T("It has {0} problems. Package it anyway?"), 1, number);
        question[255] = L'\0';
        if (!confirm(question, T("Whoever installs it will have them too."))) return;
    }
    int world = strcmp(app.current.package, "mcworld") == 0;
    IFileSaveDialog *dialog = NULL;
    if (FAILED(CoCreateInstance(&file_save_dialog, NULL, CLSCTX_INPROC_SERVER, &file_save_iface, (void **)&dialog))) {
        show_failure(T("Windows could not open its file dialog"), T("Nothing was changed."));
        return;
    }
    COMDLG_FILTERSPEC type = {world ? T("Minecraft world (*.mcworld)") : T("Minecraft pack (*.mcpack)"), world ? L"*.mcworld" : L"*.mcpack"};
    IFileSaveDialog_SetFileTypes(dialog, 1, &type);
    IFileSaveDialog_SetDefaultExtension(dialog, world ? L"mcworld" : L"mcpack");
    IFileSaveDialog_SetTitle(dialog, T("Save the package"));
    WCHAR *whole = wide(app.path);
    if (whole) {
        const WCHAR *slash = wcsrchr(whole, L'\\');
        IFileSaveDialog_SetFileName(dialog, slash && slash[1] ? slash + 1 : whole);
    }
    free(whole);
    if (SUCCEEDED(IFileSaveDialog_Show(dialog, app.window))) {
        IShellItem *item = NULL;
        if (SUCCEEDED(IFileSaveDialog_GetResult(dialog, &item))) {
            WCHAR *file = NULL;
            if (SUCCEEDED(IShellItem_GetDisplayName(item, SIGDN_FILESYSPATH, &file))) {
                package_to(file);
                CoTaskMemFree(file);
            }
            IShellItem_Release(item);
        }
    }
    IFileSaveDialog_Release(dialog);
}

/* The folder in Explorer, or the file chosen inside its folder. */
static void show_in_explorer(void) {
    WCHAR *mine = NULL;
    const WCHAR *path = NULL;
    if (app.home) {
        int row = ListView_GetNextItem(app.list, -1, LVNI_SELECTED);
        if (row >= 0 && (size_t)row < app.view_count) path = app.places[app.view[row]].path;
    } else if (app.path) {
        mine = wide(app.path);
        path = mine;
    }
    if (path) {
        DWORD kind = GetFileAttributesW(path);
        if (kind != INVALID_FILE_ATTRIBUTES && (kind & FILE_ATTRIBUTE_DIRECTORY)) {
            ShellExecuteW(app.window, L"open", path, NULL, NULL, SW_SHOWNORMAL);
        } else if (kind != INVALID_FILE_ATTRIBUTES) {
            size_t size = wcslen(path) + 16;
            WCHAR *args = malloc(size * sizeof(WCHAR));
            if (args) {
                _snwprintf(args, size, L"/select,\"%s\"", path);
                args[size - 1] = L'\0';
                ShellExecuteW(app.window, L"open", L"explorer.exe", args, NULL, SW_SHOWNORMAL);
                free(args);
            }
        }
    }
    free(mine);
}

static int copy_to_clipboard(const WCHAR *text) {
    size_t bytes = (wcslen(text) + 1) * sizeof(WCHAR);
    HGLOBAL memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
    if (!memory) return -1;
    void *to = GlobalLock(memory);
    if (to) {
        memcpy(to, text, bytes);
        GlobalUnlock(memory);
    }
    if (!to || !OpenClipboard(app.window)) {
        GlobalFree(memory);
        return -1;
    }
    EmptyClipboard();
    /* the clipboard owns it from here, if it took it */
    if (!SetClipboardData(CF_UNICODETEXT, memory)) GlobalFree(memory);
    CloseClipboard();
    return 0;
}

/* The report as text to paste anywhere: what is open, the verdict, every row
 * whatever the filter shows. */
static void copy_report(void) {
    if (!app.has_report) return;
    int head_len = GetWindowTextLengthW(app.header), sum_len = GetWindowTextLengthW(app.summary);
    size_t size = (size_t)head_len + (size_t)sum_len + 16;
    WCHAR *where = app.path ? wide(app.path) : NULL;
    if (where) size += wcslen(where) + 2;
    /* every finding, grouped on screen or not */
    int grouped = app.by_cause, inspecting = app.inspecting;
    app.by_cause = 0;
    app.inspecting = 0;
    for (size_t i = 0; i < app.current.row_count; i++)
        size += wcslen(T(row_type_key(i))) + 3 + (app.row_text[i] ? wcslen(app.row_text[i]) : 0) + 2;
    WCHAR *text = malloc(size * sizeof(WCHAR));
    if (text) {
        WCHAR *p = text;
        if (where) p += _snwprintf(p, size, L"%s\r\n", where);
        GetWindowTextW(app.header, p, head_len + 1);
        p += head_len;
        *p++ = L'\r';
        *p++ = L'\n';
        GetWindowTextW(app.summary, p, sum_len + 1);
        p += sum_len;
        *p++ = L'\r';
        *p++ = L'\n';
        for (size_t i = 0; i < app.current.row_count; i++) {
            size_t left = size - (size_t)(p - text);
            p += _snwprintf(p, left, L"%s: %s\r\n", T(row_type_key(i)), app.row_text[i] ? app.row_text[i] : L"");
        }
        *p = L'\0';
        if (copy_to_clipboard(text) == 0) {
            SetWindowTextW(BUTTON(ID_COPY), T("Copied"));
            InvalidateRect(BUTTON(ID_COPY), NULL, FALSE);
            SetTimer(app.window, TIMER_COPIED, 1500, NULL);
        }
        free(text);
    }
    app.by_cause = grouped;
    app.inspecting = inspecting;
    free(where);
}

/* ---------- words that change with the language ---------- */

static void add_tip(int id, const WCHAR *text) {
    TOOLINFOW tool = {sizeof(tool)};
    tool.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
    tool.hwnd = app.window;
    tool.uId = (UINT_PTR)BUTTON(id);
    tool.lpszText = (WCHAR *)text;
    SendMessageW(app.tips, TTM_DELTOOLW, 0, (LPARAM)&tool);
    SendMessageW(app.tips, TTM_ADDTOOLW, 0, (LPARAM)&tool);
}

static void theme_label(void) {
    SetWindowTextW(BUTTON(ID_THEME), strcmp(app.theme, "light") == 0    ? T("Light")
                                     : strcmp(app.theme, "system") == 0 ? T("System")
                                                                        : T("Dark"));
}

/* The language's name for itself, from the engine's list. */
static void language_label(void) {
    const char *list = app.engine.languages ? app.engine.languages() : "";
    size_t want = strlen(app.language);
    for (const char *line = list; *line;) {
        const char *end = strchr(line, '\n');
        size_t len = end ? (size_t)(end - line) : strlen(line);
        const char *tab = memchr(line, '\t', len);
        if (tab && (size_t)(tab - line) == want && memcmp(line, app.language, want) == 0) {
            char name[64];
            snprintf(name, sizeof(name), "%.*s", (int)(len - want - 1), tab + 1);
            set_text(BUTTON(ID_LANGUAGE), name);
            return;
        }
        line += len + (end ? 1 : 0);
    }
    SetWindowTextW(BUTTON(ID_LANGUAGE), L"English");
}

static void set_labels(void) {
    SetWindowTextW(BUTTON(ID_HOME), T("Home"));
    SetWindowTextW(BUTTON(ID_OPEN_FOLDER), T("Open folder..."));
    SetWindowTextW(BUTTON(ID_OPEN_FILE), T("Open file..."));
    SetWindowTextW(BUTTON(ID_ANALYSE), T("Analyse"));
    SetWindowTextW(BUTTON(ID_PREVIEW), T("Preview repair"));
    SetWindowTextW(BUTTON(ID_APPLY), T("Apply repair..."));
    SetWindowTextW(BUTTON(ID_UNDO), T("Undo repair..."));
    SetWindowTextW(BUTTON(ID_SHOW), T("Show folder"));
    SetWindowTextW(BUTTON(ID_COPY), T("Copy report"));
    SetWindowTextW(BUTTON(ID_PACKAGE), T("Package..."));
    SetWindowTextW(BUTTON(ID_WORLD), T("Inspect world"));
    set_group_label();
    SetWindowTextW(BUTTON(ID_CANCEL), T("Cancel"));
    theme_label();
    language_label();
    set_tile_labels();
    add_tip(ID_PREVIEW, T("See what a repair would change. Nothing is written."));
    add_tip(ID_APPLY, T("Backs up every file first, and keeps the result only if it is strictly better."));
    add_tip(ID_UNDO, T("Puts back every file the repair changed."));
    add_tip(ID_COPY, T("Copies the whole report, to paste anywhere."));
    add_tip(ID_SHOW, T("Opens it in the file explorer."));
    add_tip(ID_PACKAGE, T("Puts everything in one file, ready to open in Minecraft."));
    add_tip(ID_WORLD, T("Reads everything saved in this world. Nothing is written."));
    InvalidateRect(app.path_box, NULL, TRUE);
    InvalidateRect(app.filter, NULL, TRUE);
}

/* Another language: every word again, and what is open said again by the engine. */
static void change_language(const char *code) {
    snprintf(app.language, sizeof(app.language), "%s", code);
    if (app.engine.set_language) app.engine.set_language(code);
    forget_said();
    make_fonts();
    set_labels();
    layout();
    save_settings();
    if (app.busy) return;
    if (app.path) analyse_again();
    else go_home();
}

static void pick_language(void) {
    if (!app.engine.languages) return;
    HMENU menu = CreatePopupMenu();
    if (!menu) return;
    static char codes[32][8];
    int n = 0;
    for (const char *line = app.engine.languages(); *line && n < 32;) {
        const char *end = strchr(line, '\n');
        size_t len = end ? (size_t)(end - line) : strlen(line);
        const char *tab = memchr(line, '\t', len);
        if (tab) {
            snprintf(codes[n], sizeof(codes[n]), "%.*s", (int)(tab - line), line);
            char name[64];
            snprintf(name, sizeof(name), "%.*s", (int)(len - (size_t)(tab - line) - 1), tab + 1);
            WCHAR *label = wide(name);
            UINT flags = MF_STRING | (strcmp(codes[n], app.language) == 0 ? MF_CHECKED : 0);
            AppendMenuW(menu, flags, (UINT_PTR)(ID_LANGUAGE_FIRST + n), label ? label : L"?");
            free(label);
            n++;
        }
        line += len + (end ? 1 : 0);
    }
    RECT r;
    GetWindowRect(BUTTON(ID_LANGUAGE), &r);
    int chosen = TrackPopupMenuEx(menu, TPM_RETURNCMD | TPM_LEFTALIGN | TPM_BOTTOMALIGN, r.left, r.top, app.window, NULL);
    DestroyMenu(menu);
    if (chosen >= ID_LANGUAGE_FIRST && chosen < ID_LANGUAGE_FIRST + n) change_language(codes[chosen - ID_LANGUAGE_FIRST]);
}

static void next_theme(void) {
    const char *next = strcmp(app.theme, "dark") == 0 ? "light" : strcmp(app.theme, "light") == 0 ? "system" : "dark";
    snprintf(app.theme, sizeof(app.theme), "%s", next);
    theme_label();
    apply_theme();
    save_settings();
}

/* Tests: the next step, or the capture when there are none left. */
static void next_step(void) {
    if (!app.capture_to) return;
    while (app.steps && *app.steps) {
        WCHAR step = *app.steps++;
        if (step == L's') {
            select_first_row();
            continue;
        }
        if (step == L'p') preview_repair();
        else if (step == L'a') apply_repair(0);
        else if (step == L'u') undo_repair(0);
        else if (step == L'k' && app.package_to) package_to(app.package_to);
        else if (step == L'w') inspect_world();
        else continue;
        if (app.busy) return; /* it comes back here when it is done */
    }
    UpdateWindow(app.window);
    PostQuitMessage(save_capture(app.capture_to) == 0 ? 0 : 3);
}

/* ---------- painting ---------- */

static COLORREF verdict_color(void) {
    switch (app.verdict) {
    case VERDICT_GOOD: return app.c.good;
    case VERDICT_BAD: return app.c.bad;
    case VERDICT_NOTHING: return app.c.warn;
    default: return app.c.accent;
    }
}

static const WCHAR *verdict_icon(void) {
    if (app.busy) return L"\xE895"; /* sync */
    switch (app.verdict) {
    case VERDICT_GOOD: return L"\xE73E"; /* check */
    case VERDICT_BAD: return L"\xE783";  /* error */
    case VERDICT_NOTHING: return L"\xE7BA"; /* warning */
    default: return L"\xE721";           /* search */
    }
}

static void paint_brand(Canvas *c) {
    int x = px(14), y = px(18), size = px(46);
    for (int i = 4; i >= 1; i--)
        round_rect(c, x - i * 2, y - i * 2, size + i * 4, size + i * 4, px(11) + i * 2, app.c.accent, app.c.accent, 9);
    /* its own black tile, to read the same on either theme, with round corners */
    if (app.logo) {
        float half = (float)size / 2, inner = half - (float)px(11);
        for (int py = 0; py < size; py++) {
            for (int px_ = 0; px_ < size; px_++) {
                float qx = fabsf((float)px_ + 0.5f - half) - inner, qy = fabsf((float)py + 0.5f - half) - inner;
                float ax = qx > 0 ? qx : 0, ay = qy > 0 ? qy : 0;
                float cover = 0.5f - (sqrtf(ax * ax + ay * ay) - (float)px(11));
                if (cover <= 0) continue;
                if (cover > 1) cover = 1;
                DWORD *to = c->pixels + (size_t)(y + py) * (size_t)c->w + (size_t)(x + px_);
                DWORD from = app.logo[(size_t)py * (size_t)size + (size_t)px_];
                *to = mix(*to, RGB((from >> 16) & 255, (from >> 8) & 255, from & 255), (unsigned)(cover * 255));
            }
        }
    } else {
        round_rect(c, x, y, size, size, px(11), app.c.primary_a, app.c.primary_b, 255);
    }
    HGDIOBJ old = SelectObject(c->dc, app.font_brand);
    SetTextCharacterExtra(c->dc, px(2));
    SetTextColor(c->dc, app.c.text);
    RECT name = rect_of(x + size + px(10), y + px(1), px(140), px(24));
    DrawTextW(c->dc, L"KEBITX", -1, &name, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);
    SetTextCharacterExtra(c->dc, 0);
    SelectObject(c->dc, old);
    text_in(c, app.font_small, app.c.dim, T("Semantic engine for Minecraft packs"), rect_of(x + size + px(10), y + px(24), px(134), px(32)), DT_LEFT | DT_WORDBREAK);
}

static void paint(HDC target) {
    RECT area;
    GetClientRect(app.window, &area);
    int w = area.right, h = area.bottom, rail = px(212);
    Canvas c;
    if (canvas_begin(&c, target, w, h) != 0) return;

    /* the ground, with a faint grid of points */
    FillRect(c.dc, &area, app.back_brush);
    GdiFlush();
    int step = px(22);
    for (int y = step / 2; y < h; y += step) {
        for (int x = rail + step / 2; x < w; x += step) {
            DWORD *p = c.pixels + (size_t)y * (size_t)w + x;
            *p = mix(*p, app.c.line, 150);
        }
    }

    /* the rail, and the line of light above the content */
    RECT rail_rect = rect_of(0, 0, rail, h);
    HBRUSH rail_brush = CreateSolidBrush(app.c.rail);
    FillRect(c.dc, &rail_rect, rail_brush);
    DeleteObject(rail_brush);
    GdiFlush();
    round_rect(&c, rail - 1, 0, 1, h, 0, app.c.line, app.c.line, 255);
    round_rect(&c, rail, 0, w - rail, px(2), 0, app.c.accent, app.c.accent2, 255);
    paint_brand(&c);

    card(&c, app.path_card, px(10), app.c.field, GetFocus() == app.path_box ? app.c.accent : app.c.line);
    text_in(&c, app.font_icon, app.c.dim, L"\xED25", rect_of(app.path_card.left + px(10), app.path_card.top, px(22), px(40)), DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    /* the verdict: its colour along the edge, and a glow around its mark */
    COLORREF tone = verdict_color();
    card(&c, app.hero_card, px(12), app.c.card, app.c.line);
    RECT hero = app.hero_card;
    int hero_w = hero.right - hero.left;
    round_rect(&c, hero.left, hero.top + px(12), px(4), hero.bottom - hero.top - px(24), px(2), tone, tone, 255);
    int mark = px(32), mx = hero.left + px(12), my = hero.top + px(14);
    for (int i = 4; i >= 1; i--)
        round_rect(&c, mx - i * 2, my - i * 2, mark + i * 4, mark + i * 4, mark / 2 + i * 2, tone, tone, 14);
    round_rect(&c, mx, my, mark, mark, mark / 2, tone, tone, 40);
    text_in(&c, app.font_icon_big, tone, verdict_icon(), rect_of(mx, my, mark, mark), DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    /* working: how far along, and a light that runs over it */
    if (app.busy) {
        int track_y = hero.bottom - px(5), track_w = hero_w - px(24), track_x = hero.left + px(12);
        round_rect(&c, track_x, track_y, track_w, px(3), px(1), app.c.line, app.c.line, 255);
        if (app.steps_total) {
            int done = track_w * (int)(app.step + 1) / (int)app.steps_total;
            round_rect(&c, track_x, track_y, done, px(3), px(1), app.c.accent, app.c.accent2, 255);
        } else {
            int run = track_w / 4, at = (app.phase * px(6)) % (track_w + run) - run;
            round_rect(&c, track_x + at, track_y, run, px(3), px(1), app.c.accent, app.c.accent2, 255);
        }
    }

    card(&c, app.filter_card, px(9), app.c.field, GetFocus() == app.filter ? app.c.accent : app.c.line);
    text_in(&c, app.font_icon, app.c.dim, L"\xE721", rect_of(app.filter_card.left + px(10), app.filter_card.top, px(22), px(34)), DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    card(&c, app.list_card, px(10), app.c.card, app.c.line);
    card(&c, app.detail_card, px(10), app.c.card, app.c.line);

    canvas_end(&c, target, 0, 0);
}

/* The way forward is the one that stands out. */
static int is_primary(int id) {
    if (id == ID_WORLD) return app.can_inspect && app.has_report && app.current.nothing;
    if (id == ID_APPLY) return app.fingerprint != NULL;
    if (id == ID_PREVIEW) return app.has_report && !app.fingerprint && app.path && app.current.broken > 0;
    return 0;
}

static void paint_tile(Canvas *c, int id, const DRAWITEMSTRUCT *d, const WCHAR *label) {
    int index = id - ID_TILE_0;
    int active = app.tile == index;
    int hot = app.hot == id;
    RECT r = d->rcItem;
    FillRect(c->dc, &r, app.back_brush);
    GdiFlush();
    COLORREF tone = app.c.accent;
    if (app.inspecting) {
        tone = index == 2 ? app.c.good : index == 3 ? app.c.alt : app.c.accent;
    } else if (!app.home) {
        if (index == 1) tone = app.c.bad;
        if (index == 2) tone = app.c.warn;
        if (index == 3) tone = app.c.dim;
    }
    if (app.home && index == 1) tone = app.c.alt;
    if (app.home && index == 2) tone = app.c.accent2;
    if (app.home && index == 3) tone = app.c.good;
    if (!place_rows() && index == 1 && app.tiles_known && app.tile_counts[1] == 0) tone = app.c.good;
    card(c, r, px(10), hot && !active ? between(app.c.card, tone, 0.08f) : app.c.card, active ? tone : hot ? between(app.c.line, tone, 0.5f) : app.c.line);
    if (active) round_rect(c, r.left + px(12), r.bottom - px(4), r.right - r.left - px(24), px(2), px(1), tone, tone, 255);
    WCHAR number[32] = L"\x2013";
    if (app.tiles_known) _snwprintf(number, 32, L"%zu", app.tile_counts[index]);
    number[31] = L'\0';
    text_in(c, app.font_number, app.tiles_known ? tone : app.c.dim, number, rect_of(r.left + px(14), r.top + px(6), r.right - r.left - px(20), px(34)), DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    text_in(c, app.font_small, app.c.dim, label, rect_of(r.left + px(14), r.top + px(41), r.right - r.left - px(20), px(20)), DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
}

/* A button, drawn as this window's: Windows still does the clicking and the keys. */
static void draw_button(const DRAWITEMSTRUCT *d) {
    int id = (int)d->CtlID;
    RECT area = d->rcItem;
    int w = area.right, h = area.bottom;
    Canvas c;
    if (canvas_begin(&c, d->hDC, w, h) != 0) return;
    WCHAR label[128];
    GetWindowTextW(d->hwndItem, label, 128);
    int disabled = d->itemState & ODS_DISABLED;
    int pressed = d->itemState & ODS_SELECTED;
    int focused = d->itemState & ODS_FOCUS;
    int hot = app.hot == id && !disabled;

    if (id >= ID_TILE_0 && id <= ID_TILE_3) {
        paint_tile(&c, id, d, label);
        if (focused) {
            RECT ring = area;
            HBRUSH brush = CreateSolidBrush(app.c.accent);
            FrameRect(c.dc, &ring, brush);
            DeleteObject(brush);
        }
        canvas_end(&c, d->hDC, 0, 0);
        return;
    }

    int nav = id <= ID_THEME;
    COLORREF ground = nav ? app.c.rail : app.c.back;
    HBRUSH ground_brush = CreateSolidBrush(ground);
    FillRect(c.dc, &area, ground_brush);
    DeleteObject(ground_brush);
    GdiFlush();

    COLORREF ink = disabled ? between(ground, app.c.dim, 0.55f) : app.c.text;
    int primary = is_primary(id) && !disabled;
    if (primary) {
        for (int i = 3; i >= 1; i--) round_rect(&c, -i + 3, -i + 3, w + i * 2 - 6, h + i * 2 - 6, px(10), app.c.accent, app.c.accent, 16);
        COLORREF a = pressed ? between(app.c.primary_a, app.c.back, 0.2f) : app.c.primary_a;
        COLORREF b = pressed ? between(app.c.primary_b, app.c.back, 0.2f) : app.c.primary_b;
        round_rect(&c, 1, 1, w - 2, h - 2, px(9), a, b, 255);
        ink = app.c.on_primary;
    } else if (nav) {
        if (hot || pressed || (id == ID_HOME && app.home)) {
            COLORREF tint = between(ground, app.c.accent, pressed ? 0.22f : 0.13f);
            round_rect(&c, 0, 0, w, h, px(8), tint, tint, 255);
        }
        if (id == ID_HOME && app.home) round_rect(&c, 0, h / 4, px(3), h / 2, px(1), app.c.accent, app.c.accent, 255);
    } else {
        COLORREF fill = disabled ? app.c.back : pressed ? between(app.c.card, app.c.accent, 0.25f) : hot ? between(app.c.card, app.c.accent, 0.12f) : app.c.card;
        COLORREF line = disabled ? between(app.c.back, app.c.line, 0.6f) : hot ? app.c.accent : app.c.line;
        RECT whole = rect_of(0, 0, w, h);
        card(&c, whole, px(9), fill, line);
    }
    if (focused) {
        RECT whole = rect_of(1, 1, w - 2, h - 2);
        HBRUSH brush = CreateSolidBrush(app.c.accent);
        FrameRect(c.dc, &whole, brush);
        DeleteObject(brush);
    }
    /* only its icon: centred, and its tip has the words */
    if (w < px(60)) {
        COLORREF only = primary || disabled ? ink : app.c.accent;
        text_in(&c, app.font_icon, only, button_icon(id), rect_of(0, 0, w, h), DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        canvas_end(&c, d->hDC, 0, 0);
        return;
    }
    int icon_x = nav ? px(14) : px(12);
    COLORREF icon_ink = primary || disabled ? ink : nav ? (hot ? app.c.accent : app.c.dim) : app.c.accent;
    text_in(&c, app.font_icon, icon_ink, button_icon(id), rect_of(icon_x, 0, px(22), h), DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    text_in(&c, app.font, ink, label, rect_of(icon_x + px(24), 0, w - icon_x - px(28), h), DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    canvas_end(&c, d->hDC, 0, 0);
}

static COLORREF row_tone(size_t i) {
    if (app.inspecting) {
        int tile = row_tile(i);
        return tile == 1 ? app.c.accent : tile == 2 ? app.c.good : app.c.alt;
    }
    if (app.home) {
        const char *group = place_group(app.places[i].what);
        if (strcmp(app.places[i].what, "Recent") == 0) return app.c.accent;
        if (strcmp(group, "World") == 0) return app.c.alt;
        if (strcmp(group, "Behavior") == 0) return app.c.accent2;
        if (strcmp(group, "Resource") == 0) return app.c.good;
        return app.c.warn;
    }
    if (use_causes()) return app.causes[i].is_problem ? app.c.bad : app.c.warn;
    const Row *row = &app.current.rows[i];
    if (row->is_problem) return strcmp(app.row_label, "Change") == 0 ? app.c.accent : app.c.bad;
    return row->is_dead ? app.c.warn : app.c.dim;
}

/* A row: its kind as a tag in its colour, then its words. */
static void draw_row(const DRAWITEMSTRUCT *d) {
    size_t shown = d->itemID;
    if (!app.view || shown >= app.view_count) return;
    size_t i = app.view[shown];
    RECT r = d->rcItem;
    int w = r.right - r.left, h = r.bottom - r.top;
    Canvas c;
    if (canvas_begin(&c, d->hDC, w, h) != 0) return;
    RECT whole = rect_of(0, 0, w, h);
    FillRect(c.dc, &whole, app.card_brush);
    GdiFlush();
    int chosen = d->itemState & ODS_SELECTED;
    if (chosen) {
        COLORREF tint = between(app.c.card, app.c.accent, app.dark ? 0.17f : 0.12f);
        round_rect(&c, px(4), 1, w - px(8), h - 2, px(6), tint, tint, 255);
        round_rect(&c, px(4), px(6), px(3), h - px(12), px(1), app.c.accent, app.c.accent, 255);
    }
    int kind_w = ListView_GetColumnWidth(app.list, 0);
    COLORREF tone = row_tone(i);
    const WCHAR *kind = T(row_type_key(i));
    HGDIOBJ old = SelectObject(c.dc, app.font_small);
    SIZE size = {0};
    GetTextExtentPoint32W(c.dc, kind, (int)wcslen(kind), &size);
    SelectObject(c.dc, old);
    int tag_w = size.cx + px(16);
    if (tag_w > kind_w - px(18)) tag_w = kind_w - px(18);
    int tag_h = px(20), tag_y = (h - tag_h) / 2;
    round_rect(&c, px(12), tag_y, tag_w, tag_h, px(5), tone, tone, 38);
    text_in(&c, app.font_small, tone, kind, rect_of(px(12), tag_y, tag_w, tag_h), DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);

    int name_w = ListView_GetColumnWidth(app.list, 1);
    const WCHAR *main_text = place_rows() ? app.places[i].name : use_causes() ? app.causes[i].line : app.row_text[i];
    if (main_text) text_in(&c, app.font, app.c.text, main_text, rect_of(kind_w + px(4), 0, name_w - px(8), h), DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    if (place_rows()) text_in(&c, app.font_small, app.c.dim, app.places[i].path, rect_of(kind_w + name_w + px(4), 0, w - kind_w - name_w - px(12), h), DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS);
    canvas_end(&c, d->hDC, r.left, r.top);
}

/* The list: a header of this window's colours, and when it is empty, why. */
static LRESULT CALLBACK on_list_message(HWND list, UINT message, WPARAM w, LPARAM l, UINT_PTR id, DWORD_PTR data) {
    (void)id;
    (void)data;
    if (message == WM_NOTIFY && ((NMHDR *)l)->hwndFrom == ListView_GetHeader(list) && ((NMHDR *)l)->code == NM_CUSTOMDRAW) {
        NMCUSTOMDRAW *draw = (NMCUSTOMDRAW *)l;
        if (draw->dwDrawStage == CDDS_PREPAINT) {
            RECT all;
            GetClientRect(draw->hdr.hwndFrom, &all);
            FillRect(draw->hdc, &all, app.card_brush);
            return CDRF_NOTIFYITEMDRAW | CDRF_NOTIFYPOSTPAINT;
        }
        if (draw->dwDrawStage == CDDS_POSTPAINT) {
            /* past the last column, the theme paints its own colour */
            RECT last = {0}, all;
            GetClientRect(draw->hdr.hwndFrom, &all);
            Header_GetItemRect(draw->hdr.hwndFrom, Header_GetItemCount(draw->hdr.hwndFrom) - 1, &last);
            all.left = last.right;
            FillRect(draw->hdc, &all, app.card_brush);
            return CDRF_DODEFAULT;
        }
        if (draw->dwDrawStage == CDDS_ITEMPREPAINT) {
            WCHAR text[64] = L"";
            HDITEMW item = {HDI_TEXT};
            item.pszText = text;
            item.cchTextMax = 64;
            Header_GetItem(draw->hdr.hwndFrom, draw->dwItemSpec, &item);
            RECT r = draw->rc;
            r.left += px(12);
            HGDIOBJ old = SelectObject(draw->hdc, app.font_small);
            SetBkMode(draw->hdc, TRANSPARENT);
            SetTextColor(draw->hdc, app.c.dim);
            DrawTextW(draw->hdc, text, -1, &r, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
            SelectObject(draw->hdc, old);
            return CDRF_SKIPDEFAULT;
        }
    }
    LRESULT result = DefSubclassProc(list, message, w, l);
    if (message == WM_PAINT && app.view_count > 0) {
        /* below the last row the theme draws column lines */
        RECT last, area;
        if (ListView_GetItemRect(list, (int)app.view_count - 1, &last, LVIR_BOUNDS)) {
            GetClientRect(list, &area);
            if (last.bottom < area.bottom) {
                area.top = last.bottom;
                HDC dc = GetDC(list);
                FillRect(dc, &area, app.card_brush);
                ReleaseDC(list, dc);
            }
        }
    }
    if (message == WM_PAINT && app.view_count == 0 && !app.busy && (app.has_report || place_rows())) {
        WCHAR wanted[8] = L"";
        GetWindowTextW(app.filter, wanted, 8);
        const WCHAR *say = wanted[0] || app.tile ? T("Nothing in the list matches the filter.")
                           : app.home            ? T("Nothing found here. Choose a folder or a file above.")
                                                 : T("No findings to list.");
        RECT area, head;
        GetClientRect(list, &area);
        GetWindowRect(ListView_GetHeader(list), &head);
        area.top += head.bottom - head.top;
        HDC dc = GetDC(list);
        /* over the column lines the theme draws in an empty list */
        FillRect(dc, &area, app.card_brush);
        area.top += px(24);
        HGDIOBJ old = SelectObject(dc, app.font);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, app.c.dim);
        DrawTextW(dc, say, -1, &area, DT_CENTER | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(dc, old);
        ReleaseDC(list, dc);
    }
    return result;
}

/* A box's hint, in the language in use. The frame around it is the window's, and
 * lights up with the focus. */
static LRESULT CALLBACK on_box_message(HWND box, UINT message, WPARAM w, LPARAM l, UINT_PTR id, DWORD_PTR data) {
    (void)id;
    if (message == WM_SETFOCUS || message == WM_KILLFOCUS) {
        InvalidateRect(box, NULL, TRUE);
        RECT frame;
        GetWindowRect(box, &frame);
        MapWindowPoints(NULL, app.window, (POINT *)&frame, 2);
        InflateRect(&frame, px(40), px(10));
        InvalidateRect(app.window, &frame, FALSE);
    }
    LRESULT result = DefSubclassProc(box, message, w, l);
    if (message == WM_PAINT && GetWindowTextLengthW(box) == 0 && GetFocus() != box) {
        RECT area;
        SendMessageW(box, EM_GETRECT, 0, (LPARAM)&area);
        HDC dc = GetDC(box);
        HGDIOBJ old = SelectObject(dc, app.font);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, app.c.dim);
        DrawTextW(dc, T((const char *)data), -1, &area, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
        SelectObject(dc, old);
        ReleaseDC(box, dc);
    }
    return result;
}

/* A button that lights up under the mouse. A second click soon after the first is
 * one more click, not a double-click. */
static LRESULT CALLBACK on_button_message(HWND button, UINT message, WPARAM w, LPARAM l, UINT_PTR id, DWORD_PTR data) {
    (void)data;
    if (message == WM_LBUTTONDBLCLK) message = WM_LBUTTONDOWN;
    if (message == WM_MOUSEMOVE && app.hot != (int)id) {
        int was = app.hot;
        app.hot = (int)id;
        if (was) InvalidateRect(BUTTON(was), NULL, FALSE);
        InvalidateRect(button, NULL, FALSE);
        TRACKMOUSEEVENT track = {sizeof(track), TME_LEAVE, button, 0};
        TrackMouseEvent(&track);
    } else if (message == WM_MOUSELEAVE && app.hot == (int)id) {
        app.hot = 0;
        InvalidateRect(button, NULL, FALSE);
    }
    return DefSubclassProc(button, message, w, l);
}

/* ---------- the window ---------- */

/* A finding says what is wrong, then in brackets how to fix it: apart, the fix can
 * be read at a glance. */
static void show_detail(const WCHAR *text) {
    const WCHAR *open = wcsstr(text, L" (");
    size_t len = wcslen(text);
    WCHAR *split = NULL;
    if (open && len && text[len - 1] == L')') {
        size_t before = (size_t)(open - text), inside = len - before - 3;
        split = malloc((len + 16) * sizeof(WCHAR));
        if (split) {
            wmemcpy(split, text, before);
            static const WCHAR arrow[] = L"\r\n\x2192 ";
            size_t arrow_len = wcslen(arrow);
            wmemcpy(split + before, arrow, arrow_len);
            wmemcpy(split + before + arrow_len, open + 2, inside);
            split[before + arrow_len + inside] = L'\0';
        }
    }
    SetWindowTextW(app.detail, split ? split : text);
    free(split);
}

/* A thing a world has, how many, and the rest of what is known of it. */
static void show_world_row(const Place *row) {
    const WCHAR *rest = app.world_details ? app.world_details : L"";
    size_t size = wcslen(row->name) + wcslen(row->path) + wcslen(rest) + 16;
    WCHAR *text = malloc(size * sizeof(WCHAR));
    if (!text) return;
    _snwprintf(text, size, L"%s: %s\r\n%s", row->name, row->path, rest);
    text[size - 1] = L'\0';
    SetWindowTextW(app.detail, text);
    free(text);
}

/* What to fix, how, and everything it touches, one per line as Windows wants. */
static void show_cause(const Cause *c) {
    const WCHAR *affects = T("Affects:");
    size_t lines = 1;
    for (const WCHAR *p = c->places; *p; p++) lines += *p == L'\n';
    size_t size = wcslen(c->what) + wcslen(c->fix) + wcslen(affects) + wcslen(c->places) + lines + 16;
    WCHAR *text = malloc(size * sizeof(WCHAR));
    if (!text) return;
    WCHAR *p = text;
    if (!c->fix[0]) {
        SetWindowTextW(app.detail, c->what);
        free(text);
        return;
    }
    p += _snwprintf(p, size, L"%s\r\n\x2192 %s\r\n%s ", c->what, c->fix, affects);
    for (const WCHAR *from = c->places; *from; from++) {
        if (*from == L'\n') {
            *p++ = L',';
            *p++ = L' ';
        } else {
            *p++ = *from;
        }
    }
    *p = L'\0';
    SetWindowTextW(app.detail, text);
    free(text);
}

static void on_notify(NMHDR *note) {
    if (note->idFrom != ID_LIST) return;
    if (note->code == LVN_GETDISPINFOW) {
        /* the rows are drawn in draw_row; this is what screen readers ask for */
        LVITEMW *item = &((NMLVDISPINFOW *)note)->item;
        size_t shown = (size_t)item->iItem;
        if (!(item->mask & LVIF_TEXT) || !app.view || shown >= app.view_count) return;
        size_t i = app.view[shown];
        if (item->iSubItem == 0) {
            item->pszText = (WCHAR *)T(row_type_key(i));
        } else if (place_rows()) {
            item->pszText = item->iSubItem == 1 ? app.places[i].name : app.places[i].path;
        } else if (item->iSubItem == 1 && use_causes()) {
            item->pszText = app.causes[i].line;
        } else if (item->iSubItem == 1 && app.row_text[i]) {
            item->pszText = app.row_text[i];
        }
    } else if (note->code == LVN_ITEMCHANGED) {
        NMLISTVIEW *change = (NMLISTVIEW *)note;
        size_t shown = (size_t)change->iItem;
        if (!(change->uNewState & LVIS_SELECTED) || shown >= app.view_count) return;
        size_t i = app.view[shown];
        if (app.home) SetWindowTextW(app.detail, app.places[i].path);
        else if (app.inspecting) show_world_row(&app.places[i]);
        else if (use_causes()) show_cause(&app.causes[i]);
        else show_detail(app.row_text[i] ? app.row_text[i] : L"");
        update_buttons();
    } else if (note->code == LVN_ITEMACTIVATE) {
        size_t shown = (size_t)((NMITEMACTIVATE *)note)->iItem;
        if (app.home && shown < app.view_count) open_path(app.places[app.view[shown]].path);
    } else if (note->code == LVN_KEYDOWN) {
        /* Ctrl+C on a row: its text */
        if (((NMLVKEYDOWN *)note)->wVKey == 'C' && (GetKeyState(VK_CONTROL) & 0x8000)) {
            int row = ListView_GetNextItem(app.list, -1, LVNI_SELECTED);
            if (row >= 0 && (size_t)row < app.view_count) {
                size_t i = app.view[row];
                const WCHAR *text = app.home ? app.places[i].path : app.inspecting ? app.places[i].name : use_causes() ? app.causes[i].line : app.row_text[i];
                if (text) copy_to_clipboard(text);
            }
        }
    }
}

static LRESULT CALLBACK on_message(HWND window, UINT message, WPARAM w, LPARAM l) {
    switch (message) {
    case WM_ERASEBKGND:
        return 1; /* WM_PAINT covers everything */
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(window, &ps);
        paint(dc);
        EndPaint(window, &ps);
        return 0;
    }
    case WM_DRAWITEM: {
        const DRAWITEMSTRUCT *d = (const DRAWITEMSTRUCT *)l;
        if (d->CtlType == ODT_BUTTON) draw_button(d);
        else if (d->CtlType == ODT_LISTVIEW) draw_row(d);
        return TRUE;
    }
    case WM_MEASUREITEM: {
        MEASUREITEMSTRUCT *m = (MEASUREITEMSTRUCT *)l;
        if (m->CtlType == ODT_LISTVIEW) m->itemHeight = (UINT)px(30);
        return TRUE;
    }
    case WM_CTLCOLORSTATIC: {
        HDC dc = (HDC)w;
        HWND control = (HWND)l;
        if (control == app.count) {
            SetTextColor(dc, app.c.dim);
            SetBkColor(dc, app.c.back);
            return (LRESULT)app.back_brush;
        }
        COLORREF ink = app.c.text;
        if (control == app.header && app.verdict != VERDICT_NONE) ink = verdict_color();
        if (control == app.summary) ink = app.c.dim;
        SetTextColor(dc, ink);
        SetBkColor(dc, app.c.card);
        return (LRESULT)app.card_brush;
    }
    case WM_CTLCOLOREDIT:
        SetTextColor((HDC)w, app.c.text);
        SetBkColor((HDC)w, app.c.field);
        return (LRESULT)app.field_brush;
    case WM_SETTINGCHANGE:
        if (l && wcscmp((const WCHAR *)l, L"ImmersiveColorSet") == 0 && app.list) apply_theme();
        break;
    case WM_TIMER:
        if (w == TIMER_COPIED) {
            KillTimer(window, TIMER_COPIED);
            SetWindowTextW(BUTTON(ID_COPY), T("Copy report"));
            InvalidateRect(BUTTON(ID_COPY), NULL, FALSE);
        } else if (w == TIMER_ANIMATION) {
            app.phase++;
            RECT strip = app.hero_card;
            strip.top = strip.bottom - px(8);
            InvalidateRect(window, &strip, FALSE);
        }
        return 0;
    case WM_SIZE:
        if (app.list) layout();
        return 0;
    case WM_GETMINMAXINFO: {
        MINMAXINFO *limits = (MINMAXINFO *)l;
        limits->ptMinTrackSize.x = px(1020);
        limits->ptMinTrackSize.y = px(680);
        return 0;
    }
    case WM_DPICHANGED: {
        app.dpi = HIWORD(w);
        make_fonts();
        RECT *r = (RECT *)l;
        SetWindowPos(window, NULL, r->left, r->top, r->right - r->left, r->bottom - r->top, SWP_NOZORDER | SWP_NOACTIVATE);
        return 0;
    }
    case WM_SETCURSOR:
        if (app.busy) {
            SetCursor(LoadCursorW(NULL, IDC_APPSTARTING));
            return TRUE;
        }
        break;
    case WM_COMMAND:
        switch (LOWORD(w)) {
        case ID_HOME: go_home(); break;
        case ID_ANALYSE: analyse_typed(); break;
        case ID_OPEN_FOLDER: choose(1); break;
        case ID_OPEN_FILE: choose(0); break;
        case ID_LANGUAGE: pick_language(); break;
        case ID_THEME: next_theme(); break;
        case ID_TILE_0:
        case ID_TILE_1:
        case ID_TILE_2:
        case ID_TILE_3: {
            int index = LOWORD(w) - ID_TILE_0;
            if (app.tiles_known) {
                app.tile = app.tile == index ? 0 : index;
                for (int i = 0; i < TILES; i++) InvalidateRect(BUTTON(ID_TILE_0 + i), NULL, FALSE);
                rebuild_view();
            }
            break;
        }
        case ID_PREVIEW: preview_repair(); break;
        case ID_APPLY: apply_repair(1); break;
        case ID_UNDO: undo_repair(1); break;
        case ID_SHOW: show_in_explorer(); break;
        case ID_COPY: copy_report(); break;
        case ID_PACKAGE: package_now(); break;
        case ID_WORLD: inspect_world(); break;
        case ID_GROUP:
            app.by_cause = !app.by_cause;
            set_group_label();
            count_tiles();
            rebuild_view();
            break;
        case ID_FILTER:
            if (HIWORD(w) == EN_CHANGE && app.list) rebuild_view();
            break;
        case ID_FOCUS_FILTER:
            SetFocus(app.filter);
            SendMessageW(app.filter, EM_SETSEL, 0, -1);
            break;
        case ID_FOCUS_PATH:
            SetFocus(app.path_box);
            SendMessageW(app.path_box, EM_SETSEL, 0, -1);
            break;
        case ID_PATH:
            if (HIWORD(w) == EN_CHANGE) update_buttons();
            break;
        case IDOK: /* Enter: where the focus is decides what it means */
            if (GetFocus() == app.list && app.home) {
                int row = ListView_GetNextItem(app.list, -1, LVNI_SELECTED);
                if (row >= 0 && (size_t)row < app.view_count) open_path(app.places[app.view[row]].path);
            } else if (GetFocus() == app.path_box) {
                analyse_typed();
            } else if (GetFocus() == app.filter && app.view_count) {
                /* from the filter to what it found */
                SetFocus(app.list);
                select_first_row();
            }
            break;
        case ID_CANCEL:
        case IDCANCEL: /* Esc */
            if (GetFocus() == app.filter && GetWindowTextLengthW(app.filter) > 0) {
                SetWindowTextW(app.filter, L"");
            } else if (app.busy && (app.running == JOB_REPORT || app.running == JOB_WORLD)) {
                InterlockedExchange(&app.cancel, 1);
                SetWindowTextW(app.header, T("Cancelling..."));
                EnableWindow(BUTTON(ID_CANCEL), FALSE);
            }
            break;
        }
        return 0;
    case WM_NOTIFY:
        on_notify((NMHDR *)l);
        return 0;
    case WM_DROPFILES: {
        HDROP drop = (HDROP)w;
        WCHAR path[32768];
        if (DragQueryFileW(drop, 0, path, 32768)) open_path(path);
        DragFinish(drop);
        return 0;
    }
    case WM_APP_PROGRESS: {
        WCHAR *stage = (WCHAR *)l;
        if (app.busy && (app.running == JOB_REPORT || app.running == JOB_WORLD)) {
            WCHAR a[16], b[16], line[512];
            app.step = LOWORD(w);
            app.steps_total = HIWORD(w);
            _snwprintf(a, 16, L"%u", app.step + 1);
            _snwprintf(b, 16, L"%u", app.steps_total);
            fill_in(line, 512, T("Step {0} of {1}: {2}"), 3, a, b, stage);
            SetWindowTextW(app.summary, line);
            RECT strip = app.hero_card;
            strip.top = strip.bottom - px(8);
            InvalidateRect(window, &strip, FALSE);
        }
        free(stage);
        return 0;
    }
    case WM_APP_DONE: {
        char *json = (char *)l;
        JobKind kind = (JobKind)w;
        app.busy = 0;
        KillTimer(window, TIMER_ANIMATION);
        SetCursor(LoadCursorW(NULL, IDC_ARROW));
        switch (kind) {
        case JOB_REPORT: show_analysis(json); break;
        case JOB_PLAN: show_plan(json); break;
        case JOB_APPLY: show_outcome(json, T("The repair gave no answer")); break;
        case JOB_UNDO: show_outcome(json, T("The undo gave no answer")); break;
        case JOB_PLACES: show_places(json); break;
        case JOB_PACKAGE: show_package(json); break;
        case JOB_WORLD: show_world(json); break;
        }
        if (json) app.engine.string_free(json);
        layout();
        update_buttons();
        InvalidateRect(BUTTON(ID_HOME), NULL, FALSE);
        /* the button that had the focus may be disabled while it worked */
        if (!GetFocus()) SetFocus(app.list);
        RECT hero = app.hero_card;
        InvalidateRect(window, &hero, FALSE);
        next_step();
        return 0;
    }
    case WM_CLOSE:
        /* a repair cut short would leave files half changed */
        if (app.busy && (app.running == JOB_APPLY || app.running == JOB_UNDO)) {
            MessageBoxW(window, T("A repair is running. It will finish in a moment."), L"Kebitx", MB_OK | MB_ICONINFORMATION);
            return 0;
        }
        break;
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(window, message, w, l);
}

static HWND child(const WCHAR *cls, const WCHAR *text, DWORD style, int id) {
    return CreateWindowExW(0, cls, text, WS_CHILD | WS_VISIBLE | style, 0, 0, 0, 0, app.window, (HMENU)(INT_PTR)id,
                           GetModuleHandleW(NULL), NULL);
}

static void add_button(int id) {
    BUTTON(id) = child(L"BUTTON", L"", BS_OWNERDRAW | WS_TABSTOP, id);
    SetWindowSubclass(BUTTON(id), on_button_message, (UINT_PTR)id, 0);
}

/* The order here is the order of the Tab key. */
static void build_window(void) {
    for (int id = ID_HOME; id <= ID_OPEN_FILE; id++) add_button(id);
    app.path_box = child(L"EDIT", L"", ES_AUTOHSCROLL | WS_TABSTOP, ID_PATH);
    SetWindowSubclass(app.path_box, on_box_message, 1, (DWORD_PTR) "Type or paste the path of a pack, a world, a mod or a mods folder");
    SHAutoComplete(app.path_box, SHACF_FILESYSTEM);
    add_button(ID_ANALYSE);
    app.header = child(L"STATIC", L"", SS_NOPREFIX | SS_ENDELLIPSIS, ID_HEADER);
    app.summary = child(L"STATIC", L"", SS_NOPREFIX, ID_SUMMARY);
    for (int id = ID_TILE_0; id <= ID_TILE_3; id++) add_button(id);
    app.filter = child(L"EDIT", L"", ES_AUTOHSCROLL | WS_TABSTOP, ID_FILTER);
    SetWindowSubclass(app.filter, on_box_message, 1, (DWORD_PTR) "Filter the list: a name, a file, a kind of problem (Ctrl+F)");
    add_button(ID_GROUP);
    app.count = child(L"STATIC", L"", SS_NOPREFIX | SS_RIGHT, ID_COUNT);
    app.list = child(WC_LISTVIEWW, L"", LVS_REPORT | LVS_OWNERDATA | LVS_OWNERDRAWFIXED | LVS_SINGLESEL | LVS_SHOWSELALWAYS | WS_TABSTOP, ID_LIST);
    SetWindowSubclass(app.list, on_list_message, 1, 0);
    ListView_SetExtendedListViewStyle(app.list, LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER);
    LVCOLUMNW column = {LVCF_TEXT | LVCF_WIDTH};
    column.cx = 100;
    column.pszText = L"";
    ListView_InsertColumn(app.list, 0, &column);
    ListView_InsertColumn(app.list, 1, &column);
    ListView_InsertColumn(app.list, 2, &column);
    app.detail = child(L"EDIT", L"", ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | WS_VSCROLL | WS_TABSTOP, ID_DETAIL);
    for (int id = ID_PREVIEW; id <= ID_CANCEL; id++) add_button(id);
    add_button(ID_LANGUAGE);
    add_button(ID_THEME);
    app.tips = CreateWindowExW(WS_EX_TOPMOST, TOOLTIPS_CLASSW, NULL, WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX, 0, 0, 0, 0, app.window, NULL,
                               GetModuleHandleW(NULL), NULL);
    SendMessageW(app.tips, TTM_SETMAXTIPWIDTH, 0, px(320));
}

/* The engine, next to the executable. */
static int load_engine(void) {
    WCHAR path[32768];
    DWORD n = GetModuleFileNameW(NULL, path, 32768);
    if (n == 0 || n >= 32768) return -1;
    WCHAR *slash = wcsrchr(path, L'\\');
    if (!slash || (size_t)(slash - path) + 16 >= 32768) return -1;
    wcscpy(slash + 1, L"kebitx_ffi.dll");
    HMODULE engine = LoadLibraryExW(path, NULL, LOAD_WITH_ALTERED_SEARCH_PATH);
    if (!engine) return -1;
    /* through void*: Windows gives every symbol the same pointer type */
    ReportFn report = (ReportFn)(void *)GetProcAddress(engine, "kebitx_report_with_progress");
    app.engine.plan = (PlanFn)(void *)GetProcAddress(engine, "kebitx_repair_plan");
    app.engine.apply = (ApplyFn)(void *)GetProcAddress(engine, "kebitx_repair_apply");
    app.engine.undo = (UndoFn)(void *)GetProcAddress(engine, "kebitx_repair_undo");
    app.engine.places = (PlacesFn)(void *)GetProcAddress(engine, "kebitx_places");
    app.engine.package = (ApplyFn)(void *)GetProcAddress(engine, "kebitx_package");
    app.engine.world = (WorldFn)(void *)GetProcAddress(engine, "kebitx_world");
    app.engine.string_free = (FreeFn)(void *)GetProcAddress(engine, "kebitx_string_free");
    app.engine.set_language = (SetLanguageFn)(void *)GetProcAddress(engine, "kebitx_set_language");
    app.engine.languages = (LanguagesFn)(void *)GetProcAddress(engine, "kebitx_languages");
    app.engine.tr = (TrFn)(void *)GetProcAddress(engine, "kebitx_tr");
    if (!report || !app.engine.places || !app.engine.package || !app.engine.world || !app.engine.plan || !app.engine.apply || !app.engine.undo || !app.engine.string_free ||
        !app.engine.set_language || !app.engine.languages || !app.engine.tr) {
        app.engine.tr = NULL;
        app.engine.languages = NULL;
        app.engine.set_language = NULL;
        return -1;
    }
    app.engine.report = report;
    return 0;
}

/* The language saved, or the system's if the engine has it, or English. */
static void choose_language(void) {
    char *forced = app.forced_language ? narrow(app.forced_language) : NULL;
    if (forced) {
        snprintf(app.language, sizeof(app.language), "%s", forced);
        free(forced);
    }
    if (!app.language[0]) {
        WCHAR name[LOCALE_NAME_MAX_LENGTH] = L"";
        if (GetUserDefaultLocaleName(name, LOCALE_NAME_MAX_LENGTH)) {
            WCHAR *dash = wcschr(name, L'-');
            if (dash) *dash = L'\0';
            char *code = narrow(name);
            if (code) {
                snprintf(app.language, sizeof(app.language), "%s", code);
                free(code);
            }
        }
    }
    if (!app.engine.set_language || !app.language[0] || !app.engine.set_language(app.language)) {
        strcpy(app.language, "en");
        if (app.engine.set_language) app.engine.set_language("en");
    }
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR line, int show) {
    (void)previous;
    (void)line;
    int argc = 0;
    WCHAR **argv = CommandLineToArgvW(GetCommandLineW(), &argc);
    const WCHAR *start_path = NULL;
    for (int i = 1; argv && i < argc; i++) {
        if (wcscmp(argv[i], L"--capture") == 0 && i + 1 < argc) app.capture_to = argv[++i];
        else if (wcscmp(argv[i], L"--steps") == 0 && i + 1 < argc) app.steps = argv[++i];
        else if (wcscmp(argv[i], L"--theme") == 0 && i + 1 < argc) app.forced_theme = argv[++i];
        else if (wcscmp(argv[i], L"--lang") == 0 && i + 1 < argc) app.forced_language = argv[++i];
        else if (wcscmp(argv[i], L"--package-to") == 0 && i + 1 < argc) app.package_to = argv[++i];
        else start_path = argv[i];
    }

    CoInitializeEx(NULL, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    INITCOMMONCONTROLSEX controls = {sizeof(controls), ICC_LISTVIEW_CLASSES | ICC_BAR_CLASSES};
    InitCommonControlsEx(&controls);

    strcpy(app.theme, "dark");
    app.by_cause = 1;
    if (!app.capture_to) load_settings();
    int engine = load_engine();
    choose_language();

    WNDCLASSEXW cls = {sizeof(cls)};
    cls.lpfnWndProc = on_message;
    cls.hInstance = instance;
    cls.hCursor = LoadCursorW(NULL, IDC_ARROW);
    cls.hIcon = LoadIconW(instance, MAKEINTRESOURCEW(1));
    cls.hIconSm = cls.hIcon;
    cls.lpszClassName = L"KebitxApp";
    RegisterClassExW(&cls);

    app.dpi = GetDpiForSystem();
    set_palette(1);
    app.window = CreateWindowExW(WS_EX_ACCEPTFILES, L"KebitxApp", L"Kebitx", WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, CW_USEDEFAULT, CW_USEDEFAULT,
                                 px(1240), px(840), NULL, NULL, instance, NULL);
    if (!app.window) return 1;
    app.dpi = GetDpiForWindow(app.window);
    build_window();
    make_fonts();
    set_labels();
    apply_theme();
    layout();
    update_buttons();

    if (engine != 0) show_failure(T("The engine is missing"), T("A current kebitx_ffi.dll must be next to this program."));

    /* a capture needs a window that is really drawn, away from the screen */
    if (app.capture_to) {
        if (engine != 0) return 2;
        SetWindowPos(app.window, NULL, -32000, -32000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        ShowWindow(app.window, SW_SHOWNOACTIVATE);
    } else {
        ShowWindow(app.window, show);
    }
    UpdateWindow(app.window);

    if (engine == 0) {
        if (start_path) open_path(start_path);
        else go_home();
    }
    SetFocus(app.list);

    ACCEL shortcuts[] = {
        {FVIRTKEY | FCONTROL, 'O', ID_OPEN_FOLDER},
        {FVIRTKEY | FCONTROL | FSHIFT, 'O', ID_OPEN_FILE},
        {FVIRTKEY | FCONTROL, 'L', ID_FOCUS_PATH},
        {FVIRTKEY | FCONTROL, 'F', ID_FOCUS_FILTER},
        {FVIRTKEY, VK_F5, ID_ANALYSE},
        {FVIRTKEY | FALT, VK_HOME, ID_HOME},
    };
    HACCEL keys = CreateAcceleratorTableW(shortcuts, sizeof(shortcuts) / sizeof(shortcuts[0]));

    MSG message;
    while (GetMessageW(&message, NULL, 0, 0) > 0) {
        if (!TranslateAcceleratorW(app.window, keys, &message) && !IsDialogMessageW(app.window, &message)) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    return (int)message.wParam;
}
