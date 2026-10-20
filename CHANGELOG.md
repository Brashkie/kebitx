# Changelog

All notable changes to Kebitx are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.1.0] - 2026-10-19

Java Edition becomes a target of its own (Phase 7, Level 1 complete). Kebitx analyses Java
resource packs, mods and whole `mods/` folders against the installed game, including each mod's
code and Mixins, and repairs a mods folder with the same guarantees as Bedrock.

### Added

- **`kebitx analyze` for Java Edition** resource packs and mods (Fabric, Quilt, Forge and
  NeoForge), from a folder, `.jar` or `.zip` read in place.
  - **Checked against:** the installed game version whose format the pack declares (format
    ranges included) or, for a mod, the newest stable version. The report says which version
    was used and which version the mod declares.
  - **Broken:**
    - missing model parents, textures and models;
    - unbound texture variables in used models;
    - animations that do not divide into frames;
    - `.png` files that are not PNGs.
  - **Information:** missing language keys, and textures in proportions other than the game's.
  - **What is proven:** only the `minecraft` namespace and a mod's own namespace. Other mods'
    references are counted as unchecked.
- **New crate `kebitx-java`** for Phase 7: the installed Java game (moved from
  `kebitx-migrate`, which now uses it), sources read in place, and the analysis.
- **The installed game finds the version for a pack's format range** (`min_format` to
  `max_format`, `supported_formats`).
- **A mod's code checked against the game:** a class-file reader of its own, written with the
  standard library only.
  - **What is checked:** every game class, field and method the mod names, and every `@Mixin`
    target. Members are looked up as the JVM does. The mod's bundled jars, read in memory, and
    what Mixins merge into the game's classes count.
  - **Not claimed:**
    - Forge and NeoForge patches, and dependencies the mod does not bundle (listed as
      information);
    - mods built for obfuscated versions (intermediary names), which are not compared.
- **What Mixins need of their targets:**
  - `@Shadow`, `@Overwrite`, `@Accessor` and `@Invoker` members;
  - the methods injectors go into (Mixin's and MixinExtras');
  - `@At` targets.

  An injection that matches nothing is broken when it is required (`require`, or the config's
  `injectors.defaultRequire`), and information otherwise.
- **`kebitx analyze <mods folder>`:** a whole `mods/` folder, as the loader sees it.
  - **Per mod:** each one is checked against the game and the others.
  - **Refusals:** duplicate ids, missing dependencies, wrong versions (`minecraft` included),
    and declared `breaks` / `incompatible`.
  - **Information:** `@Overwrite` conflicts and mixed loaders.
  - **The game:** the installed version every mod accepts is chosen.
- **`kebitx repair <mods folder>`:** `--fix duplicates` (the default) and `incompatible`.
  - **How it works:** it takes jars out of the folder with a backup, analyses again, and keeps
    the result only if it is strictly better.
  - **Undo:** `kebitx repair --undo` restores Java backups as well as Bedrock ones.
- **Version requirements as the loaders read them:** Fabric predicates (`~`, `^`, `>=`, `.x`,
  pre-releases) and Maven ranges (Forge, NeoForge).
- **The game version comes from the mod.** It is the one version the mod names
  (`~26.2`, `26.3.x`, `[1.21.1]`), or the one chosen with `--game <version>`.
- **`ZipArchive::from_bytes`:** archives read in memory (a `.jar` inside a `.jar`), never
  written to disk.

### Verified

- **No false positives:** the game's own assets, analysed as a pack, give no finding — Java
  26.2 (20 873 references) and 26.3 (23 517 references).
- **Faithful 32x 26.3:** nothing broken. One real off-size particle frame is reported as
  information.
- **Farmer's Delight 1.21.1:** 1591 references checked, 0 broken.
- **Java 26.x is not obfuscated:** the `.jar` keeps the real class names, which settles how the
  bytecode reader matches names.
- **The game's code against itself:** every reference resolves — 26.2 (340 963) and 26.3
  (350 837).
- **Lithium and Sodium for 26.3, on 26.3:** nothing missing. Sodium's calls into Fabric API are
  resolved through its bundled jars' Mixins.
- **Lithium for 26.2, run on 26.3:** 17 failures with the Mixin checks. A sample was confirmed
  with `javap` in both versions.

## [2.0.0] - 2026-10-16

First public release. Kebitx is a semantic engine for Minecraft Bedrock add-ons: it analyses,
repairs, compares and builds packs and worlds, and migrates Java Edition resource packs, data
packs and mods to Bedrock. It is written in Rust with the standard library only.

### Highlights

- **No false alarms.** A reference is reported as broken only when it is proven missing. If
  Kebitx cannot be sure, it says nothing rather than guess.
- **Knows what the game provides.** Kebitx reads the installed Bedrock game, including its
  `.brarchive` bundles, so a reference to a vanilla asset is never reported as broken.
- **Migrates from Java using the games' own files as proof.** Pixels, sound files, English
  strings and Mojang's own converted pairs decide what converts. Name similarity alone never
  does.
- **Fast.** A whole 29-pack world is analysed in about 0.5 s (it was 7.6 s before the
  performance work). The game catalog is cached on disk.
- **Runs on every major platform.** Windows, Linux, macOS and Android, plus a C ABI for other
  languages.

### Added

#### Analysis (`kebitx analyze`)

- **References.** The textures, geometry, animations, animation controllers, render controllers
  and materials each client entity uses. Each one is resolved against the union of a world's
  resource packs, the way the game resolves it.
- **Render-controller slots** (`Texture.*`, `Geometry.*`, `Material.*`), checked for each entity
  that uses the controller.
- **Texture atlases** (`terrain_texture.json`, `item_texture.json`), in every shape: plain, list,
  tinted, `variations` and per face.
- **Empty texture sets** and **case mismatches**. A case mismatch works on Windows and breaks on
  Android and iOS.
- **Compatibility between packs.** Duplicate UUIDs, a `min_engine_version` above the installed
  game, missing dependencies, pack lists that point to packs that are not there, and silent
  overrides.
- **Script modules.** Invalid versions, duplicate modules, and beta APIs in a world where they
  are turned off.
- **Unused content.** Unused assets, with their size, and unused definitions.
- **Identical files**, with the space the extra copies take.
- **Graphics.** UVs outside the texture, stretched textures, animations that move nothing,
  missing materials, broken bone hierarchies, degenerate cubes and extreme scales.
- **Optimization for phones.** Oversized textures and icons, a file whose content does not match
  its extension, and the heaviest models.
- **Explanations.** Every finding comes with a confidence level and a possible fix.

#### Repair, compare and build

- **`kebitx repair`** makes case and unused-file fixes:
  - preview by default; nothing is written without `--apply`;
  - a backup is made outside the world first;
  - edits change only the exact text that needs fixing;
  - the world is analysed again afterwards, and the repair is rolled back automatically unless
    the result is strictly better;
  - `--undo` restores a backup.
- **`kebitx compare`** shows what changed between two versions: new problems, fixed problems,
  and definitions that are gone. A problem counts as fixed only while its pack still exists.
  A pack that disappears is a regression; packs are matched by their manifest UUID.
- **`kebitx build`** turns a project into an add-on:
  - filters run on a scratch copy, so the source is never touched;
  - the analysis runs as a gate before export;
  - export targets: the game's development folders, `.mcpack`, `.mcaddon` or `.mcworld`;
  - `--watch` rebuilds when the sources change.

#### Java to Bedrock migration (`kebitx migrate`)

- **Textures** convert:
  - when Bedrock has the same path;
  - through a rename table that the game's own atlases confirm;
  - when the pixels are identical, as a whole picture, a cut or a cell of a sheet;
  - when the drawn shape proves they share a layout.
- **Flipbooks.** Animated textures become flipbook entries, tick for tick.
- **Sounds** are mapped when the Java and Bedrock events play exactly the same files.
- **Languages** carry over when a Java key and a Bedrock key have identical English text.
- **Block and item models** become Bedrock geometry, with culling rules. The conversion rules are
  taken from Mojang's own converted pair, `straw_bed_foot`.
- **Recipes and loot tables** are compared by meaning:
  - 861 of the 1275 vanilla recipes in Java 26.3 convert to exactly Bedrock's own;
  - 84 of the 192 loot tables that both games have mean the same;
  - unknown tags are expanded into their items.
- **OptiFine CEM models.** A part is matched to a bone only when both paint exactly the same
  pixels.
- **Mods (`.jar`)** from Fabric, Quilt, Forge and NeoForge:
  - flat items become custom items;
  - blocks become custom blocks with geometry, materials and loot;
  - blocks with states are drafted as permutations, using the rules of Mojang's
    `shelf_mushroom`;
  - 3D items are drafted as attachables;
  - the mod's code goes on a task list for a person to reimplement.
- **Functions.** Every line is checked against the 115 commands Bedrock has.
- **Output.** Converted content goes to a resource pack and a behavior pack with stable UUIDs.
  Drafts and task lists are kept apart for review. The result is analysed again before the
  command finishes.
- **`kebitx catalog`** shows what the installed game provides, as Kebitx sees it.

#### Platform and integration

- **Finds the game on any system.**
  - Windows: the GDK install or the UWP package, whichever is newer.
  - Anywhere: `KEBITX_BEDROCK_DIR` can point to a client, a Bedrock Dedicated Server or
    Mojang's published samples.
- **Per-platform paths** for the cache, `.minecraft` and `com.mojang`.
- **Built-in codecs**, standard library only: DEFLATE/zlib, PNG (decode and encode), TGA and ZIP.
- **C ABI** (`kebitx-ffi`, `include/kebitx.h`), with a reference consumer written in Zig.
- **`kebitx --version`** and **`kebitx --help`**.

#### Project

- **CI on Windows, Linux and macOS.** It runs formatting, clippy with warnings as errors, every
  test, a short fuzzing pass and documentation checks. It also checks the build for musl,
  Android, Intel macOS and Windows GNU, and runs the Zig consumer.
- **Release workflow.** It produces binaries for Windows, Linux (glibc and static musl, x64 and
  ARM64), Android and macOS (Apple Silicon and Intel), with SHA256 checksums.
- **Verified with Chunker.** A real 28-pack world was converted Bedrock → Java → Bedrock with
  Chunker: Kebitx detects that the packs are lost and analyses the converted world like any
  other.

### Known limits

- Kebitx cannot guarantee that a visual defect disappears in game. Some defects depend on the
  renderer, the game version or Molang that is only evaluated at runtime.
- Mod code (`.class`) is never converted; it must be reimplemented.
- Java shaders have no general Bedrock equivalent.
- The tests were run on Windows by the author. Linux and macOS are covered by CI.

[Unreleased]: https://github.com/Brashkie/kebitx/compare/v2.1.0...HEAD
[2.1.0]: https://github.com/Brashkie/kebitx/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/Brashkie/kebitx/releases/tag/v2.0.0
