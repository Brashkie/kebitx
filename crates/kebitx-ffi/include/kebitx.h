/* kebitx — analysis of Minecraft packs, worlds and mods, as a C ABI.
 *
 * Link against kebitx_ffi (kebitx_ffi.dll / libkebitx_ffi.so / .dylib), or
 * load it at run time. Strings are UTF-8 and NUL-terminated.
 */
#ifndef KEBITX_H
#define KEBITX_H

#ifdef __cplusplus
extern "C" {
#endif

/* The engine version. Static: never free it. */
const char *kebitx_version(void);

/* Analyse a pack or a world folder. Returns a JSON report:
 *
 *   { "path", "engine", "game_version" (null without the game installed),
 *     "mode": "pack" | "world", "packs", "entities", "broken",
 *     "case_mismatches", "compat_errors", "unused_assets",
 *     "issues": [ { "pack", "kind", "owner", "reference", "confidence", "fix" } ] }
 *
 * Returns NULL if path is NULL, not UTF-8, not a folder, or the analysis
 * failed. Free the result with kebitx_string_free. */
char *kebitx_analyze(const char *path);

/* Analyse whatever path is, the way `kebitx analyze` decides: a Java mods folder,
 * a Java pack or mod (folder, .jar or .zip), or a Bedrock pack or world. Returns
 * a JSON report with the lines people read:
 *
 *   { "path", "engine", "kind", "broken",
 *     "summary": [ line ], "problems": [ line ], "notes": [ line ],
 *     "dead": [ line ] }
 *
 * Bedrock reports also group them: "problem_causes" and "dead_causes" have one
 * line per thing to fix, as "count\twhat\tfix\tplaces", the places one per line.
 *
 * "broken" counts what is proven broken; "notes" is information only; "dead",
 * only for Bedrock, names what does not exist where that part just does nothing. With
 * "nothing": true, nothing could be checked (not a pack, a world or a mods folder;
 * a world with no packs; a behavior pack that names nothing): "summary" says why,
 * and it is no verdict on anything. Returns
 * NULL as kebitx_analyze does. Free the result with kebitx_string_free. It can
 * take seconds: call it away from a user interface's thread. */
char *kebitx_report(const char *path);

/* Told what comes next, how many steps are done and of how many. A value other
 * than 0 cancels. Called on the thread that called kebitx_report_with_progress. */
typedef int (*kebitx_progress)(const char *stage, unsigned done, unsigned total, void *user);

/* As kebitx_report, telling progress (which may be NULL) what it is doing. A
 * cancelled analysis returns a report with "cancelled": true. It stops between
 * steps: a mod in a mods folder, or a stage elsewhere. */
char *kebitx_report_with_progress(const char *path, kebitx_progress progress, void *user);

/* The preview of a repair: the safe fixes only (wrong case in a Bedrock pack or
 * world, duplicate jars in a Java mods folder). Writes nothing. A report whose
 * "problems" are the changes, with the "fingerprint" to apply them. */
char *kebitx_repair_plan(const char *path);

/* Applies the repair if it still is what the preview with this fingerprint
 * showed. Every file is backed up first, and the result stays only if it is
 * strictly better. A report with "kept" and, when kept, "backup". Without "kept",
 * nothing was changed. Do not interrupt it: it cannot be cancelled. */
char *kebitx_repair_apply(const char *path, const char *fingerprint);

/* Puts back what a kept repair changed, from the "backup" it reported. */
char *kebitx_repair_undo(const char *backup);

/* What can be given a verdict on this computer, so nobody has to know where the
 * game keeps it: a report with one note per place, as "what\tname\tpath". A what
 * of "Shortcut" is a game folder to browse from, not to analyse. */
char *kebitx_places(void);

/* What a Bedrock world has saved: a report with "world_view": true, whose notes are
 * "what\tname\thow many" (Block, Entity or Block entity), most first. Reads, and
 * never writes. "details" has more, one per line. A report's "inspect" says a world
 * can be read so. */
char *kebitx_world(const char *path, kebitx_progress progress, void *user);

/* Puts a pack or a world in one file for the game: a report, with "package_file"
 * when it worked. A report's "package" ("mcpack" or "mcworld") says the extension. */
char *kebitx_package(const char *path, const char *out);

/* The language of what is said from now on, for the whole process, by its code:
 * "es", "pt-BR", ... Returns 1, or 0 with nothing changed if there is no such
 * language. English until it is chosen. */
int kebitx_set_language(const char *code);

/* Every language, one per line: its code, a tab and its name for itself. Static:
 * never free it. */
const char *kebitx_languages(void);

/* A text of the list (crates/kebitx-core/lang/en.txt) in the language in use, with
 * {0}, {1} where values go; text itself when the list does not have it. Never free
 * the answer. */
const char *kebitx_tr(const char *text);

/* Free a string returned by any function here. NULL is ignored. */
void kebitx_string_free(char *s);

#ifdef __cplusplus
}
#endif

#endif /* KEBITX_H */
