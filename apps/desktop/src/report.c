/* A small JSON reader that keeps only what a report needs, walking the text
 * once. Nothing is trusted: every read is bounded by the end of the string,
 * and nesting is limited, so a bad report is an error and never a crash. */
#include "report.h"

#include <stdlib.h>
#include <string.h>

#define MAX_DEPTH 32

typedef struct {
    const char *at;
} Reader;

static void skip_space(Reader *r) {
    while (*r->at == ' ' || *r->at == '\t' || *r->at == '\n' || *r->at == '\r') r->at++;
}

static int hex4(const char *s, unsigned *out) {
    unsigned v = 0;
    for (int i = 0; i < 4; i++) {
        char c = s[i];
        v <<= 4;
        if (c >= '0' && c <= '9') v |= (unsigned)(c - '0');
        else if (c >= 'a' && c <= 'f') v |= (unsigned)(c - 'a' + 10);
        else if (c >= 'A' && c <= 'F') v |= (unsigned)(c - 'A' + 10);
        else return -1; /* a NUL lands here too: nothing past it is read */
    }
    *out = v;
    return 0;
}

static size_t put_utf8(char *to, unsigned c) {
    if (c < 0x80) {
        to[0] = (char)c;
        return 1;
    }
    if (c < 0x800) {
        to[0] = (char)(0xC0 | (c >> 6));
        to[1] = (char)(0x80 | (c & 0x3F));
        return 2;
    }
    if (c < 0x10000) {
        to[0] = (char)(0xE0 | (c >> 12));
        to[1] = (char)(0x80 | ((c >> 6) & 0x3F));
        to[2] = (char)(0x80 | (c & 0x3F));
        return 3;
    }
    to[0] = (char)(0xF0 | (c >> 18));
    to[1] = (char)(0x80 | ((c >> 12) & 0x3F));
    to[2] = (char)(0x80 | ((c >> 6) & 0x3F));
    to[3] = (char)(0x80 | (c & 0x3F));
    return 4;
}

/* A string, at its opening quote. With `keep`, returns it unescaped in new
 * memory; without, returns a non-NULL marker. NULL when it is malformed. */
static char *read_string(Reader *r, int keep) {
    static char skipped;
    if (*r->at != '"') return NULL;
    const char *start = ++r->at;
    const char *p = start;
    while (*p != '"') {
        if (*p == '\0') return NULL;
        if (*p == '\\') {
            p++;
            if (*p == '\0') return NULL;
        }
        p++;
    }
    const char *end = p;
    r->at = end + 1;
    if (!keep) return &skipped;
    /* unescaped text is never longer than its source */
    char *out = malloc((size_t)(end - start) + 1);
    if (!out) return NULL;
    size_t n = 0;
    for (p = start; p < end; p++) {
        if (*p != '\\') {
            out[n++] = *p;
            continue;
        }
        p++;
        switch (*p) {
        case 'n': out[n++] = '\n'; break;
        case 't': out[n++] = '\t'; break;
        case 'r': out[n++] = '\r'; break;
        case 'b': out[n++] = '\b'; break;
        case 'f': out[n++] = '\f'; break;
        case 'u': {
            unsigned c;
            if (end - p < 5 || hex4(p + 1, &c) != 0) {
                free(out);
                return NULL;
            }
            p += 4;
            /* a surrogate pair is one character */
            if (c >= 0xD800 && c < 0xDC00 && end - p >= 7 && p[1] == '\\' && p[2] == 'u') {
                unsigned low;
                if (hex4(p + 3, &low) == 0 && low >= 0xDC00 && low < 0xE000) {
                    c = 0x10000 + ((c - 0xD800) << 10) + (low - 0xDC00);
                    p += 6;
                }
            }
            /* a lone surrogate is not text: the replacement character */
            if (c >= 0xD800 && c < 0xE000) c = 0xFFFD;
            n += put_utf8(out + n, c);
            break;
        }
        default: out[n++] = *p; break; /* \" \\ \/ */
        }
    }
    out[n] = '\0';
    return out;
}

/* Skips any value. Returns 0, or -1 when it is malformed. */
static int skip_value(Reader *r, int depth) {
    if (depth > MAX_DEPTH) return -1;
    skip_space(r);
    char c = *r->at;
    if (c == '"') return read_string(r, 0) ? 0 : -1;
    if (c == '{' || c == '[') {
        char close = c == '{' ? '}' : ']';
        r->at++;
        skip_space(r);
        if (*r->at == close) {
            r->at++;
            return 0;
        }
        for (;;) {
            if (c == '{') {
                skip_space(r);
                if (!read_string(r, 0)) return -1;
                skip_space(r);
                if (*r->at != ':') return -1;
                r->at++;
            }
            if (skip_value(r, depth + 1) != 0) return -1;
            skip_space(r);
            if (*r->at == ',') {
                r->at++;
                continue;
            }
            if (*r->at == close) {
                r->at++;
                return 0;
            }
            return -1;
        }
    }
    /* a number, true, false or null: its characters up to a delimiter */
    const char *start = r->at;
    while (*r->at && !strchr(",]} \t\r\n", *r->at)) r->at++;
    return r->at > start ? 0 : -1;
}

/* Appends one row; takes `text`. */
static int add_row(Report *rep, int is_problem, int is_dead, char *text) {
    Row *rows = realloc(rep->rows, (rep->row_count + 1) * sizeof(Row));
    if (!rows) {
        free(text);
        return -1;
    }
    rep->rows = rows;
    rep->rows[rep->row_count].is_problem = is_problem;
    rep->rows[rep->row_count].is_dead = is_dead;
    rep->rows[rep->row_count].text = text;
    rep->row_count++;
    return 0;
}

/* Appends one cause; takes `text`. */
static int add_cause(Report *rep, int is_problem, char *text) {
    Row *rows = realloc(rep->causes, (rep->cause_count + 1) * sizeof(Row));
    if (!rows) {
        free(text);
        return -1;
    }
    rep->causes = rows;
    rep->causes[rep->cause_count].is_problem = is_problem;
    rep->causes[rep->cause_count].is_dead = !is_problem;
    rep->causes[rep->cause_count].text = text;
    rep->cause_count++;
    return 0;
}

/* Appends a line to the summary; takes `line`. */
static int add_summary(Report *rep, char *line) {
    const char *sep = " | ";
    size_t have = rep->summary ? strlen(rep->summary) : 0;
    size_t need = have + strlen(sep) + strlen(line) + 1;
    char *s = realloc(rep->summary, need);
    if (!s) {
        free(line);
        return -1;
    }
    if (have) {
        strcpy(s + have, sep);
        strcpy(s + have + strlen(sep), line);
    } else {
        strcpy(s, line);
    }
    rep->summary = s;
    free(line);
    return 0;
}

/* A list of strings: each one goes to `add`, or to the rows. */
/* to_summary: 1, the summary; 2, causes; 0, the rows */
static int read_lines(Reader *r, Report *rep, int to_summary, int is_problem, int is_dead) {
    skip_space(r);
    if (*r->at != '[') return -1;
    r->at++;
    skip_space(r);
    if (*r->at == ']') {
        r->at++;
        return 0;
    }
    for (;;) {
        skip_space(r);
        char *line = read_string(r, 1);
        if (!line) return -1;
        int rc = to_summary == 1 ? add_summary(rep, line) : to_summary == 2 ? add_cause(rep, is_problem, line) : add_row(rep, is_problem, is_dead, line);
        if (rc != 0) return -1;
        skip_space(r);
        if (*r->at == ',') {
            r->at++;
            continue;
        }
        if (*r->at == ']') {
            r->at++;
            return 0;
        }
        return -1;
    }
}

/* Problems first: the engine's keys may come in any order. */
static void problems_first(Row *rows, size_t count) {
    size_t next = 0;
    for (size_t i = 0; i < count; i++) {
        if (rows[i].is_problem) {
            Row row = rows[i];
            memmove(&rows[next + 1], &rows[next], (i - next) * sizeof(Row));
            rows[next++] = row;
        }
    }
}

int report_parse(const char *json, Report *out) {
    memset(out, 0, sizeof(*out));
    if (!json) return -1;
    Reader r = {json};
    int have_kind = 0;
    skip_space(&r);
    if (*r.at != '{') return -1;
    r.at++;
    skip_space(&r);
    while (*r.at != '}') {
        char *key = read_string(&r, 1);
        if (!key) goto bad;
        skip_space(&r);
        if (*r.at != ':') {
            free(key);
            goto bad;
        }
        r.at++;
        skip_space(&r);
        int rc = 0;
        char **slot = strcmp(key, "kind") == 0          ? &out->kind
                      : strcmp(key, "path") == 0        ? &out->path
                      : strcmp(key, "fingerprint") == 0 ? &out->fingerprint
                      : strcmp(key, "backup") == 0      ? &out->backup
                      : strcmp(key, "package") == 0     ? &out->package
                      : strcmp(key, "details") == 0     ? &out->details
                      : strcmp(key, "package_file") == 0 ? &out->package_file
                                                        : NULL;
        if (slot) {
            char *v = read_string(&r, 1);
            if (!v) rc = -1;
            free(*slot);
            *slot = v;
            have_kind |= slot == &out->kind;
        } else if (strcmp(key, "kept") == 0) {
            out->kept = strncmp(r.at, "true", 4) == 0;
            rc = skip_value(&r, 0);
        } else if (strcmp(key, "cancelled") == 0) {
            out->cancelled = strncmp(r.at, "true", 4) == 0;
            rc = skip_value(&r, 0);
        } else if (strcmp(key, "inspect") == 0) {
            out->inspect = strncmp(r.at, "true", 4) == 0;
            rc = skip_value(&r, 0);
        } else if (strcmp(key, "world_view") == 0) {
            out->world_view = strncmp(r.at, "true", 4) == 0;
            rc = skip_value(&r, 0);
        } else if (strcmp(key, "nothing") == 0) {
            out->nothing = strncmp(r.at, "true", 4) == 0;
            rc = skip_value(&r, 0);
        } else if (strcmp(key, "broken") == 0) {
            out->broken = strtol(r.at, NULL, 10);
            rc = skip_value(&r, 0);
        } else if (strcmp(key, "summary") == 0) {
            rc = read_lines(&r, out, 1, 0, 0);
        } else if (strcmp(key, "problems") == 0) {
            rc = read_lines(&r, out, 0, 1, 0);
        } else if (strcmp(key, "notes") == 0) {
            rc = read_lines(&r, out, 0, 0, 0);
        } else if (strcmp(key, "problem_causes") == 0) {
            rc = read_lines(&r, out, 2, 1, 0);
        } else if (strcmp(key, "dead_causes") == 0) {
            rc = read_lines(&r, out, 2, 0, 1);
        } else if (strcmp(key, "dead") == 0) {
            rc = read_lines(&r, out, 0, 0, 1);
        } else {
            rc = skip_value(&r, 0);
        }
        free(key);
        if (rc != 0) goto bad;
        skip_space(&r);
        if (*r.at == ',') {
            r.at++;
            skip_space(&r);
        } else if (*r.at != '}') {
            goto bad;
        }
    }
    if (!have_kind) goto bad;
    problems_first(out->rows, out->row_count);
    problems_first(out->causes, out->cause_count);
    return 0;
bad:
    report_free(out);
    return -1;
}

void report_free(Report *r) {
    for (size_t i = 0; i < r->row_count; i++) free(r->rows[i].text);
    free(r->rows);
    for (size_t i = 0; i < r->cause_count; i++) free(r->causes[i].text);
    free(r->causes);
    free(r->package);
    free(r->details);
    free(r->package_file);
    free(r->kind);
    free(r->path);
    free(r->summary);
    free(r->fingerprint);
    free(r->backup);
    memset(r, 0, sizeof(*r));
}
