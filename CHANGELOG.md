# Changelog

All notable changes to Kebitx are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.6.0] - 2026-11-06

kebitx now holds no third-party code, and has a first desktop app. libdeflate and every crate are
gone: DEFLATE is kebitx's own decoder, in safe Rust. The Windows archive comes with an app in C, with
no toolkit and in seven languages: it finds what is on your computer, reports it grouped by what to
fix, repairs with a preview and an undo, and makes a pack or a world one file for the game. Behavior
packs are checked now, not only resource packs, and a world is read as the game loads it. Bedrock
worlds can be inspected with kebitx's own reader of the game's database, the first step to converting
worlds with no other tool. Against 2.5.0's binaries, which used libdeflate, speed is about the same.

### Changed

- **No third-party code.** `Cargo.lock` went from 82 packages to kebitx's own 9, and `native/c`
  is gone. kebitx holds only its own code.
- **DEFLATE and zlib are kebitx's own decoder** in every build, with no `unsafe`: 187 → 313 MB/s on
  the game's class files, with the CRC-32 check, against kebitx's previous decoder. Against the
  libdeflate build that 2.5.0 shipped: `analyze` on Sodium about the same (152 and 155 ms), a mods
  folder 4 % slower at most (424 and 407 ms), and `migrate` on Faithful 32x 4 % slower (3.21 and
  3.09 s).
- **The prebuilt CLI is pure Rust again**, and building it needs no Zig. The Zig PNG kernel is
  faster in isolation, but on whole commands it takes the same time. It stays behind
  `--features native`, and `kebitx --version` says when it is built in.
- **Nothing to install on Windows:** the CLI and the C ABI library carry the C runtime inside, and
  no longer ask for `VCRUNTIME140.dll`.
- **`kebitx analyze` on a folder that holds no pack, world or mods is now an error** (exit code 2). It
  used to print "OK": a script pointed at the wrong folder must not pass.
- **`kebitx analyze` and the gate of `kebitx build` also check behavior packs.** A pack that passed
  before can now fail on what they prove broken.
- **The packs a world's lists activate are found in the game's folders**, so they no longer read as
  packs the world does not have.

### Added

- **A desktop app for Windows, in C on Win32** (`apps/desktop`, and in the Windows archive): no toolkit
  and no third-party library. Its first screen lists what can be checked on this computer, to open
  with a double-click; there is also a path box with autocomplete, Windows' own file dialog with the
  game's folders in its side bar, drag and drop, and keyboard shortcuts. Preview a repair, apply it
  and undo it. Analyses show their progress and can be cancelled. What was opened lately comes first,
  a filter and clickable tiles narrow any list, the verdict has its colour, and the next step stands
  out. It has a dark look of its own, a light one, or follows the system. The report can be copied, and
  its folder opened in Explorer.
- **Worlds are read with own code:** `kebitx world`, **Inspect world** in the app and `kebitx_world`
  tell what a Bedrock world has saved: chunks by dimension, how many of each block, entity and block
  entity, its players and what its size goes on. The new crate `kebitx-world` reads the game's LevelDB
  with no library, verifies its checksums, and never writes. Worlds with saved data and no packs now
  appear on the app's first screen. Tested on one real world, in two versions of the game.
- **Findings grouped by what to fix** (app and C ABI): thirty places naming one missing thing are one
  row, what touches most first, with every place in its details. One click shows them all again.
- **Package from the app:** a pack becomes a `.mcpack`, a world a `.mcworld`. It says first if there are
  problems, and that a world's packs from the game's folders do not go inside. `kebitx_package`.
- **The logo** in the app, its icon and the README, and the window takes its violet.
- **Seven languages** (app and C ABI): English, Spanish, Portuguese, French, German, Russian and Chinese,
  findings included, from one catalogue in `crates/kebitx-core/lang` that every system's app shares.
  The CLI stays in English. The translations have had no native review.
- **A simpler app for Linux, not in the releases yet** (`apps/desktop/src/x11.c`): no library but libc,
  speaking the X11 protocol itself, with its own folder browser. It has neither the new look nor the
  languages, and it only runs under Xvfb in CI so far.
- **Behavior packs are checked**, alone or as a world uses them: entities, items, blocks, recipes,
  loot and trade tables, functions, spawn rules, feature rules, animations and scripts. Before, one
  read as "OK" with nothing looked at. Two levels, because the game's own files name groups, events
  and tables that do not exist, and run:
  - `MISSING` is broken: a script, a script module, a function, a server animation, a recipe symbol.
  - `DEAD` is reported and not counted: that part just does nothing.
  - Whatever cannot be known from here stays unverified, and `minecraft:` names are never judged.
  - A test runs the checker over the game's own files: none may read as broken.
  - `kebitx build`'s gate counts these too.
- **A world in the game's folders is read as the game loads it:** the packs in its folder, and those
  its lists activate from the game's folders, with what they depend on. A repair still only writes to
  the packs in its own folder.
- **No verdict when nothing was checked (C ABI and app):** a folder that holds no pack, a world with
  no packs, and a behavior pack that names nothing answer `"nothing": true` and say
  why, and no longer read as "OK".
- **The C ABI:**
  - `kebitx_report`: anything `kebitx analyze` reads, Bedrock or Java, with the lines the CLI prints.
  - `kebitx_report_with_progress`: the same, telling its steps, and it can be cancelled between them.
  - `kebitx_places`: what can be checked on this computer.
  - `kebitx_world`: what a Bedrock world has saved, with its steps, and it can be cancelled.
  - `kebitx_package`: a pack or a world in one file for the game.
  - `kebitx_set_language`, `kebitx_languages` and `kebitx_tr`: reports and texts in another language.
  - `kebitx_repair_plan`, `kebitx_repair_apply` and `kebitx_repair_undo`, with the safe fixes. Applying
    takes the preview's fingerprint, and refuses if the folder changed since.
- **Fast loop against careful path:** tests that both give the same bytes, the same input used and
  the same error, on 150 streams from another compressor, on damaged streams, at output limits
  and on 20 000 random inputs.

### Known limits

- The translations were written with no native review, and the reasons inside a repair's preview lines
  are still in English.
- The app's Linux version has not run on a real desktop.
- The app was not tried on Windows 10 or on high-DPI screens.

### Removed

- **libdeflate (C)**, vendored in `native/c/third_party`, and `native/c/bench-inflate`. On real
  commands kebitx's decoder is within 4 %.
- **criterion and its 74 crates:** `cargo bench` now runs a small harness in `benches/engine.rs`.
- **`zig build bench-inflate`** and the Zig DEFLATE benchmark.

## [2.5.0] - 2026-11-05

Analysing a mod takes about half the time, because the game's classes are now read only when
asked for. The prebuilt CLI ships with the native kernels, and CI builds every release target
with them first. Metadata reading is now one module, which fixed several small errors in how
mods are read. Reports on the real mods tested are unchanged.

### Changed

- **The game's class index is lazy.** It lists the `.jar`'s names and reads a class the first
  time something asks about it, because a mod names a small part of the game.
  `kebitx analyze` on one mod: 316 → 165 ms. On a mods folder (4 mods): 635 → 477 ms.
- **The release's CLI ships with the native kernels**, with Zig building them for each target.
  The binary grows by 28 KB on Windows, and `kebitx --version` says when they are built in. The
  C ABI library stays pure Rust. Its macOS builds move to `macos-15`.
- **`zig build bench-inflate`** in `native/zig/kebitx-native` replaces the separate
  `native/zig/bench-inflate` project.

### Added

- **CI:** a `release-targets` job builds the CLI with the native kernels for all 7 release
  targets, on the release's own runners. Where the binary can run, it also runs the differential
  tests, so a problem shows up before a tag is pushed.
- **`every_bedrock_texture_decodes`:** a test that every texture of the installed Bedrock game
  with a PNG signature decodes.

### Fixed

- **`mods.toml` values:** single-quoted values are read, and multi-line strings are skipped, so
  a `description` line that looks like a table or a key no longer breaks the metadata.
- **False "missing dependency" reports:** every `[[mods]]` of a jar now counts as provided, not
  only the first. So does Quilt's `provides`.
- **Jars for several loaders:** when the first metadata file is malformed, the next one that can
  be read is used.
- **`kebitx migrate`:** a comment after `displayName` no longer ends up in the mod's name.
- **An unreadable class of the game:** it now counts as existing, and what it declares is
  unknown. It was reported as a missing class.

### Internal

- **`kebitx_java::meta`** is the one reader of mod metadata, replacing three. `Source::each_bundled`
  is the one walk over bundled jars, replacing two.
- **`ModFolder`** opens a folder once, so the CLI no longer reads every mod's metadata twice.
- **The Java commands** moved out of `main.rs` into `crates/kebitx-cli/src/java.rs`.
- **Removed:** profiling tests that have served their purpose, and other leftovers.

## [2.3.0] - 2026-11-05

C and Zig join the engine, where measurement shows they pay, as optional native kernels for
decoding. Each one is checked byte for byte against the Rust it replaces, and without them
kebitx stays pure Rust. `kebitx migrate` is 2× faster. The PNG decoder now reads every valid
texture of the game; 41 were refused before.

### Added

- **Optional native kernels:** `--features native`, which needs Zig 0.14.
  - **libdeflate (C)** for DEFLATE and zlib: 597 MB/s against 187 MB/s for the Rust decoder.
  - **A Zig `@Vector` kernel** for PNG unfiltering: 1 228 MB/s on baseline x86-64, against
    651 MB/s.
  - **How they are built:** Zig builds both into one static library for Cargo's target
    (`native/zig/kebitx-native`), and `crates/kebitx-native` wraps it. Without the feature,
    kebitx is pure Rust and needs no Zig.
- **Differential tests:** `crates/kebitx-core/tests/native.rs`, with 150 DEFLATE/zlib streams
  from another compressor (`fixtures/deflate/streams.bin`, generated by `zig build gen-streams`).
  Native and Rust give identical bytes, including on damaged input.
- **CI:** a `native` job on Windows, Linux and macOS. It runs the Zig tests, clippy and every
  test with the native kernels, and checks that the fixture is what its generator writes.
- **libdeflate 1.26**, decompression side, vendored unmodified under
  `native/c/third_party/libdeflate`, with its MIT license, version and sha256.
- **Profiles for `kebitx migrate`:** `crates/kebitx-migrate/tests/profile.rs` (ignored tests).
- **`ZipArchive::size`:** the uncompressed size an archive declares for an entry, to check
  limits before reading.

### Changed

- **`kebitx migrate` is 2× faster, with identical reports:** Faithful 32x went from 6.9 s to
  3.5 s.
  - **Archives:** extracted on every core, after every name and the total size are checked.
  - **The game's textures:** each file is decoded on any core (bundles held one core): 1.47 →
    0.62 s.
  - **Picture matching:** regions are hashed without cropping, there is one index slot per cell
    size, and files are judged in parallel: 2.15 → 0.49 s.
- **`vanilla::files::map_files`:** reads first, then runs its function file by file on every
  core. This helps everything that reads the game's bundles.
- **PNG unfiltering in Rust** has a loop per filter instead of one generic loop: 1.5× faster.
- **`codec::inflate`, `zlib_decompress` and `png::unfilter`** use the native kernels when they
  are built in. The Rust versions stay as `inflate_rust`, `zlib_decompress_rust` and
  `unfilter_rust`.
- **PNG → RGBA** has direct paths for 8-bit RGBA and RGB, which are 98 % of the game's pixels.
  Decoding the game's textures takes 1.1 s in pure Rust and 0.65 s native, on one core.

### Fixed

- **PNG decoding refused 41 valid textures of the game itself**, which could then prove
  nothing in `kebitx migrate`:
  - **Interlaced (Adam7) PNGs (35)** are now decoded.
  - **A bad CRC in an ancillary chunk (6, a damaged `iCCP`)** is now ignored, as the PNG
    specification allows and libpng does. A bad CRC in a critical chunk is still an error.
  - **Checked against javax.imageio** on 287 of the game's PNGs, these included: the same
    pixels, every one.
  - **Effect on Faithful 32x:** one more texture converts with a pixel proof
    (`entity/equipment/humanoid_baby/chainmail` → `textures/models/armor/chainmail_baby`).

## [2.2.0] - 2026-10-19

Performance, decided by measurement. The Java analysis is about 3× faster, with identical
reports. Zig's and C's DEFLATE decoders (zlib, libdeflate) were measured on the same input
before choosing where to optimize, and the benchmarks stay in the repository.

### Changed

- **Faster Java analysis, with the same reports.** A mods folder is analysed in 2.23 s → 0.64 s,
  one mod in 1.07 s → 0.37 s.
  - **Game index:** the `.jar` is read into memory once and every core builds its classes'
    tables: 350 → 182 ms.
  - **ZIP reading:** a `.jar` is opened once instead of once per entry.
  - **DEFLATE:** decodes Huffman codes through a 10-bit lookup table, about 2× faster.
  - **CRC-32:** computes eight bytes at a time, 4× faster.
  - **Mods folder:** classes are read once and in parallel, and the game's index is built at
    the same time.

### Added

- **`native/zig/bench-inflate` and `native/c/bench-inflate`:** Zig's decoder, zlib and
  libdeflate measured on the same input as kebitx's (82, 260 and 597 MB/s; kebitx 85 → 187).
  - **Zig** brought nothing.
  - **libdeflate** would save about 36 ms of a 350 ms index, so no native code was added. The
    serial phases, fixed in Rust, saved 170 ms.
- **`ZipArchive::in_memory`:** an archive read whole, for reading most of it on every core.
- **`crates/kebitx-java/tests/profile.rs`:** the time per stage on the installed game and on a
  mods folder (ignored tests, run on demand).
- **`ZipArchive::read_raw`:** an entry's stored bytes.

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

[Unreleased]: https://github.com/Brashkie/kebitx/compare/v2.6.0...HEAD
[2.6.0]: https://github.com/Brashkie/kebitx/compare/v2.5.0...v2.6.0
[2.5.0]: https://github.com/Brashkie/kebitx/compare/v2.3.0...v2.5.0
[2.3.0]: https://github.com/Brashkie/kebitx/compare/v2.2.0...v2.3.0
[2.2.0]: https://github.com/Brashkie/kebitx/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/Brashkie/kebitx/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/Brashkie/kebitx/releases/tag/v2.0.0
