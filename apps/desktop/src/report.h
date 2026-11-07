/* What the app shows: the engine's JSON report, read with no system code, so
 * every platform's window shows the same thing and the tests run anywhere. */
#ifndef KEBITX_APP_REPORT_H
#define KEBITX_APP_REPORT_H

#include <stddef.h>

typedef struct {
    int is_problem;
    int is_dead; /* names what does not exist: that part does nothing */
    char *text;
} Row;

typedef struct {
    char *kind;
    char *path;
    char *summary; /* the engine's summary lines, joined with " | " */
    long broken;
    Row *rows; /* problems first, then the rest */
    size_t row_count;
    Row *causes; /* one per thing to fix: count\twhat\tfix\tplaces */
    size_t cause_count;
    char *fingerprint; /* a repair's preview: what applying it must match */
    char *backup;      /* a repair kept: where to undo it from */
    char *package;     /* the extension this packs into, if it does */
    char *details;     /* more of a world, one per line */
    char *package_file; /* a package made */
    int kept;          /* a repair: it stayed */
    int cancelled;     /* an analysis stopped before its end */
    int nothing;       /* no pack, world or mods here: no verdict on anything */
    int inspect;       /* a world whose saved data can be read */
    int world_view;    /* the notes are what a world has saved */
} Report;

/* Reads a report. Returns 0 on success, or -1 when the text is not a report
 * (out of memory included); then there is nothing to free. */
int report_parse(const char *json, Report *out);

void report_free(Report *r);

#endif
