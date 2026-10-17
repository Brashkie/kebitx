/* kebitx — semantic analysis of Minecraft Bedrock packs and worlds, as a C ABI.
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

/* Free a string returned by kebitx_analyze. NULL is ignored. */
void kebitx_string_free(char *s);

#ifdef __cplusplus
}
#endif

#endif /* KEBITX_H */
