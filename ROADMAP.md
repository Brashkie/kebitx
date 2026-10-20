# Kebitx roadmap

Seven phases, plus tracks that run alongside them: platforms, building add-ons and security. Each one
keeps the same rule as today: **nothing is declared broken without enough evidence**, and every
new capability arrives with fixtures that reproduce a real case.

Items marked `[x]` are done and covered by tests. Concrete numbers come from a real reference
world (29 packs, 13 resource packs) that the engine is validated against.

---

## Phase 1 — Complete the current core

Finish the indexes and passes the engine already promises, so the core can be trusted before
anything is built on top of it.

- [x] Vanilla catalog (`External`), including the `.brarchive` bundles used since 1.26.51
- [x] Texture atlases in every shape (`variations`, tints, per-face)
- [x] Empty texture sets reported as their own defect
- [x] Case mismatches (work on Windows, fail on Android/iOS)
- [x] World mode: resolve every pack against the union of all of them
- [x] Trailing commas accepted; unreadable files reported instead of skipped
- [x] Animation index, and the references from client entities to it
  — the reference world has 125 references to animations that do not exist on the client
  (10 of them name a controller only a behavior pack defines, which the client never sees)
- [x] Animation-controller index (map values and the legacy `animation_controllers` array)
- [x] Render-controller index: references by name, in string and object form
- [x] Render-controller internals: every `Texture.` / `Geometry.` / `Material.` slot checked per
  (entity, controller) pair. Molang-indexed arrays are `BoundedOpaque` unless every candidate is
  missing; expressions and per-bone materials are opaque; when packs define one controller
  differently, only what is missing under all definitions is reported. The reference world:
  2439 slots, 0 missing — a first draft reported 288, all false positives from per-bone
  materials on bones only a dead variant shows
- [x] Vanilla render-controller bodies in the catalog, so entities that use the game's
  controllers get their slots checked too (381 more slots on the reference world, 0 missing)
- [x] The catalog reads the game's resource packs only: the client never sees behavior packs
- [x] Orphan pass (`unused_asset`) for textures and sounds — when in doubt, do not report. A file
  is used if any text file of any pack names it (raw scan of quoted strings, so a reference is
  over-counted, never lost), if a path built at runtime could produce it (UI bindings, script
  templates), if a texture set names it, if it replaces a game file, or if the engine reads its
  path by convention (custom sky cubemap). Preload lists and `contents.json` are not uses. The
  reference world: 5099 files, 348 unused (4.1 MB) — among them 88 bullet sounds no sound
  definition plays, and leftovers in the world's own packs (`piano3..7`, `moon.png`)
- [x] Shared-bytes pass (`duplicate_resource`): informational only, never auto-merge; identity
  proved byte for byte. The reference world: 166 groups, 0.92 MB in extra copies
- [x] Orphans for definitions (`unused_definition`): geometry, animations, animation controllers
  and render controllers that no file outside their own definition folder names (functions
  included: `/playanimation` plays animations no entity names), that no model inherits from,
  and that the game does not use. Tokens must start at a boundary, or `animation.x` inside
  `controller.animation.x` would hide the orphan. The reference world: 3209 definitions, 1245
  unused — 1038 of them shipped unused by one addon, confirmed by an independent text search
- [x] Compatibility checks across packs, behavior packs included, in both world layouts (packs
  as subfolders, or under `behavior_packs/` + `resource_packs/`):
  - errors: duplicate UUIDs in manifests; `min_engine_version` above the installed game;
    dependencies on packs not in the world; `world_*_packs.json` activating missing packs
  - warnings, only when the versions really differ (compared structurally: formatting, comments
    and key order do not count): files the game does not merge (`loot_tables/`, `trading/`,
    `ui/`, `materials/`, `biomes_client.json`, `sounds.json`) in several packs; one identifier
    defined differently (entities, items, blocks, client entities, geometry, animations,
    controllers); packs present but not activated
  - the reference world: 0 errors, 20 warnings — among them items whose file reuses another
    item's identifier (`refresco_lata_fantastic.json` declares `cerveza_lata_linex`), so one
    of the two never exists in game
- [x] Script-module dependencies, as far as the files prove: invalid versions, one module
  required twice with different versions, and beta modules against the world's Beta APIs
  experiment (read from `level.dat` with a hardened NBT reader; without `level.dat` it is a
  warning). Plus an inventory of which pack uses which module version
- Not provable, so not checked: whether the INSTALLED game supports a given stable module
  version. The game ships no list of them and its executable is not readable; a hand-kept
  table per game version would go stale with every update. The inventory shows the versions
  instead, for the reader to compare
- [ ] Later — evidence for the stable-version question, in this order:
  1. read the game's own `ContentLog__*.txt` (written when *Content Log File* is on in the
     creator settings): the installed game reporting its own script errors, no table needed
  2. a module ↔ game version table GENERATED from the npm metadata Mojang publishes (beta
     versions carry their target game version), stored as a dated data file with its source.
     Verdict only when the installed game is inside the table's range; a newer game gets "table
     out of date", never a false error. A user-supplied file can override it

**Exit criterion:** on the reference world, every finding is a real defect and the defects
fixed by hand earlier are all found again. Zero false positives.

## Phase 2 — Graphics diagnostics

Explain *why* a model or texture looks wrong.

- [x] UVs outside the declared canvas, box and per-face. Box UV is checked face by face, and
  faces with zero area (the sides of a flat plane) are skipped: they never render. Box overflow
  must exceed one texel (fractional sizes round loosely). The reference world: 1385 models,
  46942 cubes, 113 models with UVs outside — measuring the whole box layout instead of its
  real faces had given 167
- [x] Texture proportions versus the declared `texture_width` / `texture_height` (stretched
  textures). Pairs come only from render controllers that name both directly; controllers with
  `uv_anim` are not paired (a 75-frame flipbook strip is 75 times taller on purpose — three
  portals were false positives before this rule). The reference world: 402 pairs, 1 stretched
  (a 1500×1200 blood decal on a 64×64 canvas). Image sizes are read from PNG/TGA headers only
- [ ] Attachables (same render path as entities) in the texture/model pairing
- [x] Degenerate models: bones whose `parent` no bone has (inherited bones of old-format models
  count), repeated bone names, cubes turned inside out (negative size, or an inflate that shrinks
  a side past zero), cubes of size 0. The reference world: 3 detached bones, 1 model with `head`
  and `body` twice, 20 models with inverted cubes (some are deliberate — a sight's reticle — so
  medium confidence), 17 with empty cubes (information)
- [x] Extreme animation scales: a bone scaled to 0 (it vanishes while the animation plays) or to
  20× or more — constant values and keyframes; Molang is not evaluated. Information: often
  intended. The reference world: 153
- Rotations are deliberately NOT reported: every angle is valid and large values are intended
  spins; nothing about a rotation value is provably wrong
- [x] Animations that move nothing: none of their bones exists in any of the entity's models
  (the union of all its models, old-format parents included, and of every definition of a model
  defined in several packs). Animations that miss only SOME bones are counted, not reported —
  shared animations like `look_at_target` move arms a buffalo does not have, by design. Entities
  with a model outside the packs are skipped. The reference world: 1236 entity animations, 49
  that move nothing — a penguin whose head bone is `cabeza` so it never looks at anything, 16
  backpacks whose model has `bone` where the animation moves `backpack`, corpses without the
  blood bones. A first draft reported 60 bullets as well: their model is defined three times
  and only the first definition had been read
- [x] Materials referenced by entities that no pack or the game defines. The game's come from
  its `materials/*.material` files (JSON; `name:parent` keys) and from what its own entities
  use; a pack's from its `materials/*.material`. Case is ignored. Without the game catalog
  nothing is claimed — almost every material is the game's. The reference world: 896 material
  references, 2 missing (`custom_animated` on two XaierCities NPCs; the original addon does
  not define it either)
- [x] Shader packs that only work with a modified client (`*.material.bin`, subpacks included):
  said so, and pointed to Vibrant Visuals; Vibrant Visuals content (`atmospherics/`,
  `lighting/`, `pbr`, ...) noted as needing a capable device. Checked on a real shader pack
  (Newb: 22 compiled files)
- [x] Every finding carries file, location, cause, **confidence** and a possible fix (`explain`):
  confidence is about the consequence — high when the files alone decide what the game does,
  medium when it depends on how the world is played, low when it may well be intended

## Phase 3 — Repair

Go from diagnosis to a repair that can be checked.

`kebitx-repair`, and `kebitx repair <path> [--fix case,unused] [--apply]`.

- [x] Preview of every change before writing anything: without `--apply` nothing is written
- [x] Backup of every file touched, with an index, next to the world (never inside it, so it is
  never packaged); `kebitx repair --undo <backup>` restores it
- [x] Changes made **in place**: one exact quoted literal replaced in the text, never a
  re-serialized file — comments, order, tints, weights, variations and unknown keys stay byte
  for byte. A file changed since the plan is refused
- [x] A second analysis after the repair; if the result is not strictly better (nothing worse,
  something better), every change is rolled back
- [x] Deletions only for things proven unused, and only with `--fix unused` (which refuses to
  run without the game catalog)
- First repairs: references whose case differs from the file (`--fix case`, the default) and
  unused textures and sounds (`--fix unused`)
- Validated on the reference world:
  - on the pre-fix copy, `--fix case` found exactly the 6 `desert_N` / `Desert_N.png` defects
    once hunted by hand; after the repair `diff -r` shows 6 changed lines in one file and
    nothing else; `--undo` gives back an identical world
  - on the final world, deleting the 349 "unused" files made ONE reference break, so the repair
    rolled itself back and left the world identical. The cause was a real bug in the orphan
    pass: the atlas names `"textures/items/food/raw_burger "` and the file is
    `raw_burger .png` — the space was trimmed on one side only. Fixed and covered by a test;
    348 files (4.1 MB) now delete with nothing broken, and `--undo` restores them all
- [ ] More repairs, one at a time and each with its fixture: empty atlas entries, a
  render-controller slot the entity names in another case, duplicate identifiers created by
  copy-paste (needs a new identifier and every reference to it updated)

Why the strictness: on the reference world, 86 blocks turned into pink checkerboards not because
of an addon but because of an automatic "fix" that rebuilt texture entries from what it had read
— and it could not read `variations`. Repair is the most dangerous thing this tool will ever do.

## Phase 4 — Java → Bedrock migrator

A separate crate on top of `kebitx-core`. It does not compete with world converters (Chunker);
it migrates mods, resource packs, models and addons.

- [x] Compatibility report first: what converts, what adapts, what must be rebuilt (`kebitx migrate`)
- [x] Textures: by path, by a name-table rename the game confirms (file exists, its atlas shows
  it), or by a proven name (same kind, same subject, unclaimed, same proportions)
- [x] Animated textures → flipbooks, tick for tick (per-frame times, interpolation)
- [x] `sounds.json` → `sound_definitions.json`: events mapped only by identical file sets,
  checked against the installed Java game and the installed Bedrock game
- [x] Textures proven by pixels against both games: whole pictures at other paths, cuts to
  Bedrock's layout (zombie, husk), sprites pasted into Bedrock's sheets (particles)
- [x] Language files: keys proven by identical English (unique, or unique within a key family)
- [x] Textures redrawn by Bedrock on the same layout: proven by the shape of what is drawn
  (banner patterns, villager biomes, armour); material maps never matched
- [x] Models, blockstates and every other data file compared with the Java game's own:
  unchanged needs nothing, changed or new says what Bedrock needs instead
- [x] Changed models → Bedrock geometry with the rules of Mojang's own conversion (reproduced
  from the games by a test); custom blocks and attachables drafted for a person to finish
- [ ] Still in resource packs, with the evidence: fox, sheep, saddles (redrawn on other layouts,
  Java's layout is in its code), 401 GUI sprites (no picture, shape or cell in Bedrock)
- [x] OptiFine entity models: parts proven to be Bedrock bones by the pixels their boxes paint;
  positions and animations reported, not converted (no pair in the games to check them)
- [x] Data-pack recipes → behavior-pack recipes, held to both games' own (861 of 1275 Java
  vanilla recipes reproduce Bedrock's exactly); changed vanilla recipes replace Bedrock's by
  identifier; names renamed only where the games' recipes prove them
- [x] Loot tables held to both games' own (84 of 192 at the same path mean the same; readings
  of `survives_explosion` and smelting taken from Mojang's tables); tags spelled out in the recipes
  that use them
- [x] Mods (`.jar`): flat items and stateless blocks → Bedrock custom items and blocks, with
  components Bedrock's own definitions use, geometry and culling from Mojang's rules, loot,
  recipes and names linked; blocks with states and 3D items to the task list
- [x] Forge / Fabric code: a task list for reimplementation — never claimed as automatic
- [x] Functions: every line checked against the commands Bedrock has (no translation: no pair
  in the games to prove one); advancements and world generation reported by kind
- [x] Mods: block states → permutations drafted with the rules of Bedrock's own `shelf_mushroom`
  (facing → cardinal direction, `y` → rotation, 8 of 8); 3D items → attachables drafted
- [x] Every migrated project is analysed again by the core before it is called done
- [x] Worlds converted by Chunker: analysed as any world. Checked with a round trip of the
  reference world's 28 packs on real chunks (Bedrock → Java 26.3 → Bedrock 1.26.50, Chunker
  1.21.0): Chunker drops every pack and both pack lists. `compare` used to call that "123
  problems fixed, OK"; a problem now counts as fixed only while its pack is there (packs by
  manifest UUID), so the result is a regression: 28 packs gone, 123 lost, 0 fixed. With the
  packs put back, `analyze` is identical to the original's, line for line

Converting whole Java worlds stays with Chunker (open source, years of work on block-state
mapping and the LevelDB format). A converter of our own is a long-term idea only, after this
phase, as its own crate; the NBT reader already written would carry over (Java uses the same
format, big-endian).

Language split for this phase: Rust analyses the source and orchestrates the output; Zig and C/C++
join only where a binary format or an existing library makes them clearly better. If Rust can do
a conversion correctly, it stays in Rust.

## Phase 5 — Optimization and visual testing

With mobile devices as the reference target.

`optimize`: information, measured from the files, with what each costs on a phone.

- [x] Oversized pack icons (over 256 px a side or 200 KB) and textures of 2048×2048 or more
  (decoded size shown; the sum of every texture's decoded size too). The reference world: 10
  icons still at 480–866 px, and our own `weather.png` at 2048×2048 — 16 MB of video memory
  for rain
- [x] Images whose content is another format than their extension: 20 JPEGs named `.png` in
  AdhaCraft, and a PNG named `.tga` in AplokGuns. Image formats are now told by content
  everywhere (PNG, JPEG, TGA), which also let the stretched-texture check read those files
- [x] Byte-identical duplicates across packs (the shared-bytes pass of Phase 1)
- [x] Geometry metrics: cubes and bones per model, the heaviest first (500+ cubes reported):
  `geometry.imperialship`, 1879 cubes in one bone
- Not done, on purpose: non-power-of-two textures (not a provable problem on today's mobile
  GPUs) and unused alpha channels (needs decoding every image; maybe later, measured first)
- [x] Regression checks between two versions of a project: `kebitx compare <before> <after>`
  lists problems that are NEW (the regression; exit 1), problems fixed, and what is no longer
  defined (textures, models, animations, controllers, entities other packs may still use).
  Problems are compared by content; paths inside them are made relative, so a copy in another
  folder compares equal. The reference world's own pipeline, original against final: 0 new
  problems, 639 fixed; the 1131 definitions gone are those of the packs it removed on purpose
- Compatibility per game version: what the files prove is done (`min_engine_version` against
  the installed game, script modules); checking several game versions needs several installs,
  which the engine cannot get by itself
- Screenshot comparison: needs the game driven automatically, which the unmodified client does
  not allow; left out rather than faked

## Phase 6 — Ecosystem

- [x] A C ABI (`kebitx-ffi`, header `crates/kebitx-ffi/include/kebitx.h`): `kebitx_version`,
  `kebitx_analyze` (a JSON report with counts and every issue explained), `kebitx_string_free`.
  The only crate with `unsafe`, confined to the C-string frontier, each block with its safety
  condition; a Rust panic never crosses into C (it answers NULL). Tested by calling it exactly
  as C does
- [x] Reference consumer of the C ABI written in Zig (`native/zig/kebitx-check`): loads the
  library at run time, so the C ABI is the whole contract; built `ReleaseSafe`. On the reference
  world it reports the same numbers as the Rust CLI
- [x] Zig as the cross-compilation linker (`native/zig/cross`): the CLI builds for Linux x86-64
  and ARM64 from Windows. Checked as valid ELF executables; not run here (no Linux on this
  machine)
- [x] Android: a fully static ARM64 build (`aarch64-unknown-linux-musl`, zig cc as linker with
  `-C link-self-contained=no` so Rust's and Zig's start files do not collide), 1.6 MB, the kind
  of binary Termux or `adb shell` run. Built and checked as a static ARM64 executable; not yet
  run on a phone
- [ ] Optional desktop interface
- [ ] Integration with modeling tools (Blockbench)
- [ ] An extensible system of rules and adapters
- [x] The analysis as a standalone step other build tools can call at the end of their
  pipelines: `kebitx analyze` (0 clean, 1 broken, 2 usage) and `kebitx compare` (1 on a
  regression), or the C ABI for tools that want the report as JSON

---

## Phase 7 — Java Edition as a first-class target

Until now Java is where migrations come from. This phase makes it a platform kebitx analyses,
repairs and optimises for its own sake, with the same rule: verdicts proven against the
installed game (its `.jar`, its assets), never guessed. Levels, from least to most risk:

**Level 1 — static analysis (no code runs)**
- [x] `kebitx analyze` for Java resource packs and mods (`kebitx-java`). It reads folders,
  `.jar` and `.zip` in place, and checks against the pack, the mod and the game version whose
  format the pack declares (format ranges included). Broken:
  - models whose parent does not exist;
  - textures that do not exist;
  - blockstates and item definitions naming missing models;
  - unbound `#variables` in used models;
  - animations that do not divide into frames;
  - `.png` files that are not PNGs (the game's own `PngInfo.validateHeader`).

  Information: missing language keys, and proportions other than the game's.

  Only `minecraft` and a mod's own namespace are proven; other mods' references are counted
  as unchecked. The game's own assets give 0 findings (26.2: 20 873 references; 26.3:
  23 517). On real content:
  - Faithful 32x 26.3: 0 broken, and one real off-size frame (`noxious_gas_08`, 32×30);
  - Farmer's Delight: 1591 references, 0 broken.
- [x] A class-file (bytecode) reader in Rust (`kebitx_java::class`, std only, fuzzed by
  truncation and byte changes). It checks the game classes, fields and methods each mod names,
  and its `@Mixin` targets, against the installed version: "this mod calls a method that does
  not exist in 26.3".
  - **Names:** since 26.x the `.jar` is not obfuscated, so names match as they are. Mods built
    for obfuscated versions (intermediary names) are detected and not compared.
  - **Lookup:** as the JVM does. Missing only when the whole chain is known.
  - **Environment:** the mod's bundled jars, read in memory, and what Mixins merge into their
    targets.
  - **Not claimed:** Forge/NeoForge patches and unbundled dependencies.
  - **Proofs:**
    - the game against itself: 26.2, 340 963 references; 26.3, 350 837; all resolve;
    - Lithium and Sodium for 26.3: 0 missing;
    - Lithium for 26.2 run on 26.3: 6 failures, each confirmed in the `.jar`s (the
      `RedStoneWireBlock` → `RedstoneWireBlock` rename among them)
- [x] What Mixins need of their targets (`kebitx_java::mixin`):
  - `@Shadow` and `@Overwrite`, with prefix and aliases;
  - `@Accessor` and `@Invoker` with explicit names;
  - the method of every injector (Mixin's and MixinExtras'), never an inherited one;
  - every `@At(target)`.

  A required injection (`require`, else the config's `defaultRequire`) that matches nothing is
  broken; with `require = 0` it is information.
  - **Lithium for 26.2 on 26.3:** 17 failures; a sample was confirmed with `javap` in both
    versions (`swingTime`, `spawners`, `tickChunks`, `serverTick`).
  - **Lithium and Sodium for 26.3:** nothing broken. Sodium's one `require = 0` injection is
    information.
- [x] Mod sets (`kebitx_java::modset`): a `mods/` folder analysed as the loader sees it.
  - **The game:** the installed version every mod accepts.
  - **Each mod:** checked against the game and the others; their classes and Mixins count.
  - **Refusals:** duplicate ids, missing dependencies (bundled jars provide), wrong versions
    (Fabric predicates and Maven ranges, `minecraft` included), Fabric `breaks` and NeoForge
    `incompatible`.
  - **Information:** `@Overwrite` conflicts and mixed loaders.
  - **Real sets for 26.3:** Fabric API + Lithium + Mod Menu + Sodium are clean. A bad set gets
    exactly its three refusals.
  - **Fixture test:** one problem of each kind, and nothing else.
- [x] Repairs as for Bedrock (`kebitx_java::repair`): preview, backup next to the folder (the
  same format, so `kebitx repair --undo` restores it), re-analysis, kept only if strictly
  better.
  - **Fixes:** `duplicates` keeps the highest version the game accepts; `incompatible` takes
    out mods that reject the game's version.
  - **Real set:** 20 problems → 1 → 0, and `--undo` restores every jar byte for byte.
  - **Rollback:** a removal that leaves another mod without the version it needs is undone on
    its own (test).

**Level 2 — measuring a running game (with the JVM that is already there)**
- [ ] Read profiles the player records: Java Flight Recorder (`.jfr`) and spark reports — hot
  methods, allocation, GC pauses, tick time — attributed to the mod whose classes they are in
- [ ] JVM options: advice only, and only from a measurement (a flag that helps peak speed can
  hurt start-up or memory); never changed blindly

**Level 3 — native work where it is measurable**
- [ ] Offline work on a mod's assets (textures, models) in Rust or Zig, as for Bedrock
- [ ] Native acceleration inside the game (JNI): only for an isolated, measured hot spot, where
  the gain outweighs crossing the boundary

**Research, not a commitment — a runtime of its own with a JIT (`kebitx-jit`)**

Replacing the JVM means implementing what Minecraft and its mods expect of Java: class loading
and reflection, `invokedynamic`, Mixin rewriting bytecode as classes load, threads, a garbage
collector, the native libraries (LWJGL). HotSpot's and Graal's JITs are among the best that
exist, and Java Edition's time goes mostly to rendering, chunk meshing, entity ticks and
allocation — what Sodium, Lithium or FerriteCore improve from inside Java. Taken up only if:
- a small Java program runs under it correctly, then the game with a real loader and real mods;
- measured FPS, tick time and memory beat HotSpot on the same machine;
- every behaviour it changes can be checked. Until then it is not on the plan.

## Platforms

- [x] Windows, Linux, macOS, Android: what is Windows-only (registry, package manager) is
  behind `cfg(windows)`; clippy clean, warnings as errors, for every target; Linux binaries link
  with `zig cc`; CI runs the tests on Windows, Linux and macOS
- [x] Bedrock's own files from any layout: the installed client (GDK or UWP, newest version,
  the GDK's read from `MicrosoftGame.Config`), a Bedrock Dedicated Server, Mojang's samples, or
  `KEBITX_BEDROCK_DIR` — a server's layout gives a catalog identical to the installed one
- [x] Caches in each system's own folder; `com.mojang` on Android or by `KEBITX_COM_MOJANG`
- [x] Run the tests on Linux and macOS: CI runs every test on Windows, Linux and macOS, green
  since 2.0.0 (the one test that assumed an installed game now reads the game version the
  report gives)
- [ ] Run the binaries on a phone (the Android build is published, not yet run on a device)

## Build — compiling add-ons (`kebitx build`)

Kebitx builds add-ons too: a project declares its packs and a list of
filters; the build copies the packs to a scratch folder, runs the filters in order, checks the
result with the analysis engine, and exports it. The source is never modified.

- [x] Project file `kebitx.json`: packs, filters, export target; a project name that could
  escape its folder (`..`, separators) is refused
- [x] Filters: external programs run on the scratch copy (any language), in order, with
  `KEBITX_WORK` / `KEBITX_BP` / `KEBITX_RP` / `KEBITX_PROJECT` / `KEBITX_NAME`; a failing filter
  stops the build. `kebitx-filter-example` documents the contract and is what the tests run
- [x] Built-in check: the analysis runs on the built packs (references, case, compatibility);
  errors stop the export, `--no-check` skips it
- [x] Export targets: a local `build/` folder; the game's `development_*_packs` (the new
  `%APPDATA%\Minecraft Bedrock\Users\Shared\games\com.mojang` first, then the old UWP location,
  or `export.com_mojang`); a `.mcaddon` archive (stored ZIP, fixed dates: reproducible; opened
  by Windows' own extractor in the check)
- [x] Safety: an export only replaces a folder the build itself created (marker file); anything
  else at the target is refused, never deleted. Symbolic links are not followed
- [x] `.mcpack` per pack (files at the archive's root), and a `.mcworld` target: the world
  template in `export.world`, the packs inside, and `world_*_packs.json` activating them (other
  packs in the lists kept, an older entry of ours replaced, a stale copy in the template dropped)
- [x] Watch mode (`--watch`): polling, std only; rebuilds once the sources have changed and then
  settled, so a burst of saves is one build; build outputs never trigger a rebuild
- [ ] The world pipeline of the reference project (`za`) expressed as filters

## Performance

Measure first, with criterion (`crates/kebitx-core/benches/engine.rs`) and `--timings`; keep a
change only if the report stays byte-for-byte the same.

- [x] criterion benchmarks: JSON parsing, the game catalog, and every world stage
  (`KEBITX_BENCH_WORLD` points them at a real world)
- [x] `--timings` per stage; `KEBITX_THREADS` to compare with sequential
- [x] Folder walking without a system call per entry (the largest single cost)
- [x] `par_map`: every file-heavy stage on every core, results in input order
- [x] Game install found through the registry instead of starting PowerShell
- [x] JSON: sorted-vector objects, string runs copied whole, exact fast path for numbers
- [x] Game bundles: only the index is read on open; contents on demand
- [x] Stages run concurrently in the CLI
- [x] Read every file once per run (`snapshot`, enabled only by `kebitx analyze`)
- [x] The game catalog cached on disk, keyed by install folder, game version and the packs'
  modification time; the installed game's real catalog round-trips through it exactly (a test);
  damaged or foreign files are rejected whole. 233 ms → 12 ms
- Result on the reference world: 7.6 s → 0.47 s for a full `kebitx analyze`, report unchanged

## Security — hardening against hostile input

Kebitx reads files nobody vetted: add-ons downloaded from anywhere, and the game's own bundles.
A malformed file must at worst be reported as unreadable — never crash the analysis, never touch
memory it should not. This runs alongside every phase, not after them.

- [x] `#![forbid(unsafe_code)]` in the library and the binary
- [x] JSON nesting limited to `MAX_DEPTH` (128): a file of `[[[[…` is an `Err`, not a stack
  overflow
- [x] Unterminated block comments cannot move the JSON cursor past the end of the input
- [x] `.brarchive`: every count, offset and size checked with overflow-checked arithmetic before
  it indexes anything; hostile bundles covered by tests
- [x] `cargo clippy --all-targets -- -D warnings` clean
- [x] Mutation fuzzing on stable Rust, std only (`tests/fuzz_smoke.rs`): every reader of
  untrusted input — JSON, models, entities, atlases, render controllers, manifests, materials,
  `.brarchive`, `level.dat`, PNG/JPEG/TGA headers — gets damaged versions of the real fixtures;
  nothing may panic, and parsed JSON written back must parse to the same value. Deterministic;
  `KEBITX_FUZZ_ITERS` for long sessions. A session of 1.3 million text and 300 000 binary
  mutations found nothing
- [ ] Coverage-guided fuzzing (`cargo-fuzz`, needs nightly and works best on Linux), with any
  crash kept as a regression fixture
- [x] `level.dat` NBT reader: lengths checked against the remaining bytes before allocating,
  nesting bounded; truncated, hostile and deeply nested input covered by tests
- [ ] Kani proofs for the `.brarchive` reader: no panic, no out-of-bounds read, for any input
- [x] Limits on input size: text files over 64 MiB (the largest real one is 3.8 MB), assets over
  256 MiB and bundle entries over 64 MiB are refused before reading, and the limit is enforced
  while reading; the catalog cache is capped too. Every whole-file read goes through
  `filesystem::read_limited`
- [x] The C ABI crate (`kebitx-ffi`) is the only place `unsafe` may exist, each block documented
  and covered by tests
- [x] Zig code built in `ReleaseSafe`, so bounds and overflow checks stay on in release
- [x] CI (`.github/workflows/ci.yml`): clippy, every test and a short fuzzing pass on Windows and
  Linux, and the Zig consumer analysing a fixture through the C ABI. Written, not yet run: the
  project is not in a repository yet

**On adding more languages for safety:** a third language (for example Ada/SPARK) cannot verify
Rust or Zig code — it only verifies itself — and every language boundary is a new place for memory
errors. Safety comes from the tools of the languages already in use: the compiler, clippy,
fuzzing and Kani.

---

## Cross-cutting rules

**Native code needs a benchmark.** A module moves to Zig or C/C++ only when a measurement shows a
hot path worth it, behind a defined C ABI. Zig is not automatically faster than Rust or C++.

**Zig as a toolchain** is useful from day one regardless: cross-compiling kebitx for Linux, macOS
and Android from Windows.

**Split crates as areas become real**, not before. The planned layout in the README is a target.

**No Python.** Prototypes and verification tools are written in Rust (or Zig where it fits).
