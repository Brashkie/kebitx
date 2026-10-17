# Changelog

All notable changes to Kebitx are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/Brashkie/kebitx/compare/v2.0.0...HEAD
[2.0.0]: https://github.com/Brashkie/kebitx/releases/tag/v2.0.0
