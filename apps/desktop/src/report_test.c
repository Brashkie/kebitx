/* The report reader, on what the engine writes and on what it must survive. */
#include "report.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures;

#define CHECK(cond)                                                \
    do {                                                           \
        if (!(cond)) {                                             \
            fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #cond); \
            failures++;                                            \
        }                                                          \
    } while (0)

static void reads_a_report(void) {
    Report r;
    const char *json =
        "{ \"path\": \"C:\\\\mods\", \"engine\": \"2.5.0\", \"kind\": \"Fabric mod: a\","
        " \"extra\": {\"nested\": [1, true, null, \"]\"]}, \"broken\": 2,"
        " \"summary\": [\"Game: Java 26.3\", \"Classes: 7\"],"
        " \"notes\": [\"a note\"], \"problems\": [\"first\", \"second \\\"quoted\\\"\"] }";
    CHECK(report_parse(json, &r) == 0);
    CHECK(strcmp(r.path, "C:\\mods") == 0);
    CHECK(strcmp(r.kind, "Fabric mod: a") == 0);
    CHECK(r.broken == 2);
    CHECK(strcmp(r.summary, "Game: Java 26.3 | Classes: 7") == 0);
    CHECK(r.row_count == 3);
    /* problems come first, whatever the order of the keys */
    CHECK(r.rows[0].is_problem && strcmp(r.rows[0].text, "first") == 0);
    CHECK(r.rows[1].is_problem && strcmp(r.rows[1].text, "second \"quoted\"") == 0);
    CHECK(!r.rows[2].is_problem && strcmp(r.rows[2].text, "a note") == 0);
    report_free(&r);
}

static void reads_unicode(void) {
    Report r;
    /* cuerpo_ñ directly and as an escape, and an emoji as a surrogate pair */
    const char *json = "{\"kind\":\"cuerpo_\xC3\xB1 \\u00f1 \\ud83d\\ude00 \\ud83d!\"}";
    CHECK(report_parse(json, &r) == 0);
    CHECK(strcmp(r.kind, "cuerpo_\xC3\xB1 \xC3\xB1 \xF0\x9F\x98\x80 \xEF\xBF\xBD!") == 0);
    report_free(&r);
}

static void an_empty_report_is_a_report(void) {
    Report r;
    CHECK(report_parse("{\"kind\":\"x\",\"summary\":[],\"problems\":[],\"notes\":[]}", &r) == 0);
    CHECK(r.row_count == 0 && r.summary == NULL && r.broken == 0);
    report_free(&r);
}

static void reads_a_repair(void) {
    Report r;
    const char *preview =
        "{\"kind\":\"Repair preview\",\"broken\":1,\"summary\":[\"s\"],\"problems\":[\"Edit a\"],"
        "\"notes\":[],\"fingerprint\":\"abcd1234-1\"}";
    CHECK(report_parse(preview, &r) == 0);
    CHECK(strcmp(r.fingerprint, "abcd1234-1") == 0);
    CHECK(r.backup == NULL && !r.kept && !r.cancelled);
    CHECK(r.row_count == 1 && r.rows[0].is_problem);
    report_free(&r);

    CHECK(report_parse("{\"kind\":\"Repair kept\",\"kept\":true,\"backup\":\"C:\\\\b\"}", &r) == 0);
    CHECK(r.kept && strcmp(r.backup, "C:\\b") == 0);
    report_free(&r);

    CHECK(report_parse("{\"kind\":\"Repair rolled back\",\"kept\":false}", &r) == 0);
    CHECK(!r.kept && r.backup == NULL);
    report_free(&r);

    CHECK(report_parse("{\"kind\":\"Cancelled\",\"cancelled\":true}", &r) == 0);
    CHECK(r.cancelled);
    report_free(&r);

    CHECK(report_parse("{\"kind\":\"Nothing to analyse\",\"broken\":0,\"nothing\":true}", &r) == 0);
    CHECK(r.nothing && r.broken == 0);
    report_free(&r);

    CHECK(report_parse("{\"kind\":\"Bedrock pack\",\"broken\":0}", &r) == 0);
    CHECK(!r.nothing);
    report_free(&r);
}

/* A place: what, name and path, kept whole for the window to cut. */
static void reads_places(void) {
    Report r;
    CHECK(report_parse("{\"kind\":\"Places\",\"notes\":[\"World\\tMi mundo\\tC:\\\\w\"]}", &r) == 0);
    CHECK(r.row_count == 1 && strcmp(r.rows[0].text, "World\tMi mundo\tC:\\w") == 0);
    report_free(&r);

    /* what does nothing is neither a problem nor a plain note */
    CHECK(report_parse("{\"kind\":\"k\",\"dead\":[\"d\"],\"notes\":[\"n\"],\"problems\":[\"p\"]}", &r) == 0);
    CHECK(r.row_count == 3 && r.rows[0].is_problem && !r.rows[0].is_dead);
    CHECK(!r.rows[1].is_problem && r.rows[1].is_dead && strcmp(r.rows[1].text, "d") == 0);
    CHECK(!r.rows[2].is_problem && !r.rows[2].is_dead);
    report_free(&r);
}

static void reads_causes_and_packages(void) {
    Report r;
    const char *json =
        "{\"kind\":\"k\",\"package\":\"mcpack\",\"problem_causes\":[\"2\\tw\\tf\\ta\\nb\"],"
        "\"dead_causes\":[\"1\\tx\\ty\\tz\"],\"problems\":[\"a\",\"b\"]}";
    CHECK(report_parse(json, &r) == 0);
    report_free(&r);
    /* whichever key comes first, problems lead */
    const char *dead_first = "{\"kind\":\"k\",\"dead_causes\":[\"1\\tx\\ty\\tz\"],\"problem_causes\":[\"1\\tw\\tf\\ta\"]}";
    CHECK(report_parse(dead_first, &r) == 0);
    CHECK(r.cause_count == 2 && r.causes[0].is_problem && r.causes[1].is_dead);
    report_free(&r);
    CHECK(report_parse(json, &r) == 0);
    CHECK(strcmp(r.package, "mcpack") == 0 && r.package_file == NULL);
    CHECK(r.row_count == 2 && r.cause_count == 2);
    CHECK(r.causes[0].is_problem && !r.causes[0].is_dead && strcmp(r.causes[0].text, "2\tw\tf\ta\nb") == 0);
    CHECK(!r.causes[1].is_problem && r.causes[1].is_dead);
    report_free(&r);
    CHECK(report_parse("{\"kind\":\"k\",\"package_file\":\"C:\\\\a.mcpack\"}", &r) == 0);
    CHECK(strcmp(r.package_file, "C:\\a.mcpack") == 0);
    report_free(&r);
}

static void reads_a_world(void) {
    Report r;
    CHECK(report_parse("{\"kind\":\"k\",\"inspect\":true,\"nothing\":true}", &r) == 0);
    CHECK(r.inspect && r.nothing && !r.world_view);
    report_free(&r);
    CHECK(report_parse("{\"kind\":\"k\",\"world_view\":true,\"notes\":[\"Block\\tminecraft:stone\\t5\"]}", &r) == 0);
    CHECK(r.world_view && !r.inspect && r.row_count == 1 && r.details == NULL);
    report_free(&r);
    CHECK(report_parse("{\"kind\":\"k\",\"details\":\"a\nb\"}", &r) == 0);
    CHECK(strcmp(r.details, "a\nb") == 0);
    report_free(&r);
}

static void bad_text_is_an_error_never_a_crash(void) {
    const char *bad[] = {
        NULL,
        "",
        "null",
        "[]",
        "{",
        "{}", /* no kind */
        "{\"kind\"",
        "{\"kind\":",
        "{\"kind\":\"x",
        "{\"kind\":\"x\\",
        "{\"kind\":\"\\u12",
        "{\"kind\":3}",
        "{\"kind\":\"x\",\"problems\":[\"a\"",
        "{\"kind\":\"x\",\"problems\":[\"a\",",
        "{\"kind\":\"x\",\"problems\":3}",
        "{\"kind\":\"x\",\"summary\":[1]}",
        "{\"kind\":\"x\" \"broken\":1}",
    };
    for (size_t i = 0; i < sizeof(bad) / sizeof(bad[0]); i++) {
        Report r;
        CHECK(report_parse(bad[i], &r) == -1);
    }
    /* nesting far past the limit */
    size_t n = 200000;
    char *deep = malloc(n + 32);
    CHECK(deep != NULL);
    if (deep) {
        Report r;
        strcpy(deep, "{\"kind\":\"x\",\"a\":");
        size_t at = strlen(deep);
        memset(deep + at, '[', n);
        deep[at + n] = '\0';
        CHECK(report_parse(deep, &r) == -1);
        free(deep);
    }
    /* every prefix of a good report, at every cut */
    const char *good =
        "{\"kind\":\"k\",\"path\":\"p\\u00f1\",\"broken\":1,\"summary\":[\"s\"],"
        "\"problems\":[\"a\\n\"],\"notes\":[\"b\"],\"other\":{\"x\":[1,2]}}";
    size_t len = strlen(good);
    char *cut = malloc(len + 1);
    CHECK(cut != NULL);
    for (size_t i = 0; cut && i < len; i++) {
        Report r;
        memcpy(cut, good, i);
        cut[i] = '\0';
        CHECK(report_parse(cut, &r) == -1);
    }
    free(cut);
}

int main(void) {
    reads_a_report();
    reads_unicode();
    an_empty_report_is_a_report();
    reads_a_repair();
    reads_places();
    reads_causes_and_packages();
    reads_a_world();
    bad_text_is_an_error_never_a_crash();
    if (failures) {
        fprintf(stderr, "%d check(s) failed\n", failures);
        return 1;
    }
    puts("report reader: all checks passed");
    return 0;
}
