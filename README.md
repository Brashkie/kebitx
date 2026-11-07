<div align="center">

<img src="media/kebitx.png" alt="Kebitx" width="360">

# Kebitx

**A semantic engine for Minecraft add-ons and worlds: analyse, repair, compare, build, inspect and migrate from Java.**

[![CI](https://github.com/Brashkie/kebitx/actions/workflows/ci.yml/badge.svg)](https://github.com/Brashkie/kebitx/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Brashkie/kebitx?label=release)](https://github.com/Brashkie/kebitx/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/rust-std--only-orange.svg)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux%20%7C%20macOS%20%7C%20Android-lightgrey.svg)

[Installation](#installation) · [Quick start](#quick-start) · [Commands](#commands) ·
[Migration](#migrating-a-java-resource-pack) · [Roadmap](ROADMAP.md) · [Changelog](CHANGELOG.md)

</div>

---

Kebitx **understands** a Bedrock project. It indexes everything the project declares and
everything it references, resolves the two against each other and against the installed game,
and tells you, with evidence, what is broken, why, and how to fix it. It also **builds**
add-ons: packs go through a pipeline of filters, the same engine checks the result, and only
then is it exported to the game or packed as a `.mcaddon`.

> **Precision is the product's only currency.**
> Only a reference that is proven missing is ever reported as broken. Anything the engine does
> not understand suppresses the finding; it never becomes a guess.

## Contents

- [Highlights](#highlights)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Commands](#commands)
- [What it checks](#what-it-checks)
- [Migrating a Java resource pack](#migrating-a-java-resource-pack)
- [Behavior packs](#behavior-packs)
- [Worlds](#worlds)
- [Desktop app](#desktop-app)
- [Platforms](#platforms)
- [Performance](#performance)
- [Validated on a real world](#validated-on-a-real-world)
- [Architecture](#architecture)
- [Limits worth stating up front](#limits-worth-stating-up-front)
- [Contributing](#contributing)
- [License](#license)

## Highlights

- **Every finding is backed by proof.** The installed game's own files, including the
  `.brarchive` bundles, decide what is vanilla, so a reference to a vanilla asset is never
  reported as broken.
- **Worlds are analysed as the game loads them.** Every resource pack is resolved against the
  union of all of them, and the compatibility between packs is checked too.
- **Repairs are safe.** You see a preview first. A backup is made, only the exact text that
  needs fixing is edited, the world is analysed again, and the repair rolls itself back
  unless the result is strictly better.
- **Java → Bedrock migration is proven, not guessed.** Pixels, sound files, English strings and
  Mojang's own converted pairs decide what converts.
- **It is fast.** A 29-pack world with 13 000+ files is analysed in about 0.5 s.
- **Behavior packs and worlds too.** Entities, items, recipes, functions and scripts are followed as
  far as they can be proven, and a Bedrock world's saved data is read with kebitx's own reader of the
  game's database, without ever writing to it.
- **A desktop app in seven languages.** No toolkit and nothing to install: it finds what is on your
  computer, groups findings by what to fix, repairs with a preview and an undo, and makes a pack or a
  world one file for the game.
- **It is all its own code.** `Cargo.lock` lists no third-party crate and the repository holds no
  third-party code: JSON, ZIP, DEFLATE, PNG, LevelDB and the class-file reader are written here, in
  std-only Rust with no `unsafe` (except in the C ABI crate). It runs on Windows, Linux, macOS
  and Android.

## Installation

### Prebuilt binaries

Download the archive for your system from the
[latest release](https://github.com/Brashkie/kebitx/releases/latest), extract it and put
`kebitx` (or `kebitx.exe`) on your `PATH`.

| System | Archive |
|---|---|
| Windows x64 | `kebitx-<version>-x86_64-pc-windows-msvc.zip` |
| Linux x64 | `kebitx-<version>-x86_64-unknown-linux-gnu.tar.gz`, static: `…-linux-musl` |
| Linux ARM64 | `kebitx-<version>-aarch64-unknown-linux-gnu.tar.gz`, static: `…-linux-musl` |
| Android (Termux) | `kebitx-<version>-aarch64-unknown-linux-musl.tar.gz` |
| macOS Apple Silicon | `kebitx-<version>-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `kebitx-<version>-x86_64-apple-darwin.tar.gz` |

The Windows archive also has `kebitx-app.exe`, the desktop app. Keep it next to `kebitx_ffi.dll`, and
nothing needs installing.

Every release has a `SHA256SUMS` file to verify the downloads.

### From source

You need a stable [Rust toolchain](https://rustup.rs).

```bash
git clone https://github.com/Brashkie/kebitx.git
cd kebitx
cargo install --path crates/kebitx-cli     # installs `kebitx` into ~/.cargo/bin
kebitx --version
```

With [Zig 0.14](https://ziglang.org/download/) on the `PATH`, `--features native` builds in an
optional PNG kernel, with the same results. The prebuilt binaries do not use it: see
*Native kernel* under *Performance*.

## Quick start

```bash
kebitx analyze "<path to a pack or a world>"      # what is broken, and why
kebitx analyze "<world>" --all                    # list every finding
kebitx repair "<world>"                           # preview the safe repairs (writes nothing)
kebitx compare "<old version>" "<new version>"    # did the update break anything?
kebitx migrate "<java pack or mod .jar>"          # what converts to Bedrock, and what does not
```

On Windows, Kebitx finds the installed game by itself. On other systems, set
`KEBITX_BEDROCK_DIR` to the game's files (see [Platforms](#platforms)). Without them, every
verdict that needs the game says so instead of guessing.

## Commands

| Command | What it does | Exit code |
|---|---|---|
| `kebitx analyze <pack or world> [--all] [--timings]` | Resolves every reference and checks compatibility, graphics, unused and duplicate content | 0 clean, 1 problems |
| `kebitx analyze <Java pack or mod: folder, .jar, .zip> [--all] [--game <version>]` | Java Edition: assets, and a mod's code and Mixins, checked against the installed game | 0 clean, 1 problems |
| `kebitx analyze <mods folder> [--all] [--game <version>]` | Every mod of the folder against the game and each other, and what the loader refuses (duplicates, dependencies, versions, `breaks`) | 0 clean, 1 problems |
| `kebitx repair <world> [--fix case,unused] [--apply]` | Safe repairs: preview, backup, surgical edits, second analysis, automatic rollback | 0 done, 1 rolled back |
| `kebitx repair <mods folder> [--fix duplicates,incompatible] [--apply]` | Java: takes duplicate or incompatible mods out of the folder, verified the same way | 0 done, 1 rolled back |
| `kebitx repair --undo <backup>` | Puts a backup back (Bedrock or Java) | |
| `kebitx compare <before> <after> [--all]` | What is new, fixed, lost or removed between two versions | 0 OK, 1 regression |
| `kebitx build [project] [--target …] [--watch]` | Builds an add-on through filters, gated by the analysis | 0 built, 1 failed |
| `kebitx migrate <java pack, .jar or .zip> [--out <dir>] [--all]` | Java → Bedrock: report, then convert what is proven | 0 clean result |
| `kebitx world <Bedrock world> [--all]` | What a world has saved: chunks, blocks, entities, block entities, players, size. Never writes | |
| `kebitx catalog [--list stems\|flipbooks\|tiles\|sounds]` | What the installed game provides, as Kebitx sees it | |
| `kebitx --version`, `kebitx --help` | Version and usage | |

Exit code 2 always means bad usage, a path that does not exist, or a folder that holds no pack, world or mods.

## Where it is going

The long-term goal is a tool that goes from **detecting** problems to **diagnosing** them and
**proposing verifiable repairs**, and to build add-ons, across these areas:

| Area | What it means |
|---|---|
| **Geometry & graphics** | Invisible parts, UVs outside the texture, stretched textures, bones an animation needs but the model lacks, missing geometry, scale and rotation problems |
| **Animation & rendering** | Animations, animation controllers, render controllers, materials and Molang: are the references between models, textures and animations coherent? |
| **Repair & compatibility** | Duplicate identifiers, manifests, `min_engine_version` against the installed game, dependencies between resource and behavior packs, files the game does not merge |
| **Optimization** | Oversized textures, duplicated assets, redundant geometry, costly animations — with mobile devices as the reference target |
| **Java → Bedrock migration** | Convert what can be converted, flag what cannot, and produce a task list for what needs rebuilding. Whole worlds stay with Chunker; kebitx checks what it produces |
| **Java Edition (Phase 7, started)** | Analyse Java packs and mods for their own sake: broken models and textures (working today), mod bytecode checked against the installed game, loader dependencies, profiles (JFR, spark) attributed to mods. A runtime of its own with a JIT is research with conditions, not a plan |
| **Building** | `kebitx build`: filters over a scratch copy, the analysis as a gate, export to the game's development folders or to `.mcpack` / `.mcaddon` |
| **Worlds (Phase 9, started)** | Read a world with own code (done for Bedrock), then convert between editions with no other tool, entities included |
| **Applications (Phase 8, started)** | Desktop, Android (APK) and tablet apps over the same engine through its C ABI. The interface technology will be decided by prototypes and measurements |

The identity does not change along the way: **explain why something looks broken, never modify it
blindly.** See [ROADMAP.md](ROADMAP.md) for the phases and their order.

## What it checks

Working today (Cargo workspace; everything uses the standard library only, the benchmarks
included; `unsafe` forbidden):

- comment- and trailing-comma-tolerant JSON parser, with a nesting limit so hostile input is an
  error, never a crash
- filesystem discovery (textures / entities / models), entity parsing, shortname → file collapse
- texture resolver, geometry index (both model formats) + resolver
- **animation, animation-controller and render-controller indexes** — every reference a client
  entity makes, resolved against the resource packs only, as the client does: a controller that
  only a behavior pack defines is missing
- **render-controller slots** — `Texture.default`, `Geometry.baby`, `Material.default`: checked
  for each entity that uses the controller, since the same controller can be fine for one entity
  and broken for the next. Direct names are provable; `Array.x[query.variant]` is broken only if
  every candidate is; Molang expressions and per-bone materials are never guessed at
- **vanilla catalog** read from the installed game, including the `.brarchive` bundles used since
  1.26.51. Feeds `EdgeState::External`, so references the game provides are never findings
- **texture atlases** (`terrain_texture.json`, `item_texture.json`) in every shape: plain, list,
  `path` + tint, `variations`, per-face variations. Tint colours (`#rrggbb`) are not paths
- **empty texture sets** — a shortname that yields zero paths is a pink block, guaranteed
- **case mismatches** — references that resolve only when case is ignored: they work on Windows
  and fail on Android/iOS, so testing on a PC never reveals them
- **world mode** — every resource pack in a folder is resolved against the *union* of all of
  them, as the game does. The report is still per pack. Both layouts work: packs as subfolders
  (a build folder) or under `behavior_packs/` + `resource_packs/` (a real world)
- **compatibility between packs** — errors the game would hit (duplicate UUIDs, a
  `min_engine_version` above the installed game, missing pack dependencies, pack lists activating
  packs that are not there) and silent overrides (files the game does not merge, identifiers
  defined differently in two places), reported only when the two versions really differ
- **script modules** — invalid versions, a module required twice, and beta modules in a world
  whose `level.dat` has Beta APIs off; plus the list of module versions each pack uses.
  Whether the installed game supports a stable version is not checked: nothing installed
  says so
- **unused assets** — textures and sounds nothing references, with their size. Built to never
  report a file that is used: any quoted string in any text file of the world counts, paths
  built at runtime protect everything under them, and replacements of game files are used by
  definition
- **unused definitions** — geometry, animations and controllers nothing names: not an entity, an
  attachable, a block, a script or a function (`/playanimation`), not a model inheriting from
  it, and not the game
- **identical files** — same bytes in several places, with the space the extra copies take.
  Information only: two paths with the same bytes can be two override points, so nothing is
  merged automatically
- **graphics (Phase 2)** — UVs outside the canvas a model declares (faces that sample nothing:
  transparent or garbage), stretched textures (an image whose proportions differ from the
  declared canvas; a uniform HD scale is fine, a `uv_anim` flipbook strip is not judged),
  animations that move nothing because none of their bones exists in the entity's models,
  materials no pack and not the game define (a broken reference: it counts in the exit code),
  bones hanging from a parent that does not exist, repeated bone names, cubes turned inside out
  or of size 0, animations that scale a bone to 0 or 20×, compiled shaders only a modified
  client loads, and Vibrant Visuals content that needs a capable device
- **optimization for phones (Phase 5)** — oversized pack icons, textures of 2048×2048 or more
  with their decoded size, images whose content is another format than their name (a JPEG
  named `.png`), and the models with the most cubes; image formats are told by content
- **repair (Phase 3)** — see *Repairing* below: preview, backup, surgical edits, a second
  analysis, automatic rollback
- **every finding explained** — under each one, a confidence (high: the files alone decide
  what the game does; medium: it depends on how the world is played; low: may well be
  intended) and a possible fix
- unreadable files are reported, never skipped in silence

Unused and identical files and the graphics checks are information: they explain size and
looks, they never fail a run. Rotations are deliberately not judged: any angle is valid.

### Resolution model

Two spaces of resolution, on purpose:

- **textures** live in PATH space: `textures/.../horse_black` → probe `.png` / `.tga`
- **geometry** lives in IDENTIFIER space: `geometry.horse_v3` → found inside a model's
  `description.identifier`, so it needs an Index Pass over `models/` before resolving
- **animations and controllers** live in IDENTIFIER space too: `animation.horse.walk`,
  `controller.animation.horse.move`, `controller.render.horse` → indexed by the keys of
  `animations`, `animation_controllers` and `render_controllers` in any file of any pack

Texture resolution order:

1. exact match in the packs → `Resolved`
2. match ignoring case → `CaseMismatch` (broken on case-sensitive platforms only)
3. provided by the game → `External`
4. otherwise → `KnownMissing`

## Usage in depth

### Analysing

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -- analyze fixtures/horse_pack          # OK
cargo run -- analyze fixtures/geo_missing_pack    # 1 missing geometry, exit 1
cargo run -- analyze fixtures/atlas_pack          # 1 empty set + 1 case mismatch, exit 1
cargo run -- analyze fixtures/world_two_packs     # world mode: 0 broken
cargo run -- analyze fixtures/world_compat        # 4 compatibility errors, 3 warnings
cargo run -- analyze <world_folder> --all         # list every issue
```

A folder with a resource-pack manifest is analysed as one pack. A folder that contains packs, a
`level.dat` or `world_*_packs.json` is a world: every resource pack inside it is analysed
together, then the compatibility checks run over all its packs. Any other folder (a loose pack
without a manifest) is analysed as a single pack. Exit code: 0 clean, 1 broken references, case
mismatches or compatibility errors (warnings do not fail the run), 2 bad usage or path.

### Building an add-on

```json
{
  "name": "my_addon",
  "packs": { "behavior": "packs/BP", "resource": "packs/RP" },
  "filters": [
    { "name": "minify", "run": ["node", "filters/minify.js"] }
  ],
  "export": { "target": "development" }
}
```

```bash
kebitx build                      # in the folder with kebitx.json
kebitx build my_project --target mcaddon
kebitx build --no-check           # skip the analysis gate
kebitx build --watch              # rebuild whenever the sources change
```

A build copies the packs to `.kebitx/tmp` (the source is never modified), runs the filters there
in order, checks the result with the analysis engine, and exports:

| Target | Result |
|---|---|
| `local` | `build/<name>_BP`, `build/<name>_RP` |
| `development` | the game's `development_*_packs` |
| `mcaddon` | `build/<name>.mcaddon`, both packs |
| `mcpack` | `build/<name>_BP.mcpack`, `build/<name>_RP.mcpack` |
| `mcworld` | `build/<name>.mcworld`: the world template in `export.world` with the packs inside and activated |

`--watch` rebuilds once the sources have changed and settled (a burst of saves is one build);
what a build writes never triggers another. A filter is any program: it runs in the scratch folder, finds the packs
in `KEBITX_BP` / `KEBITX_RP`, and exits with 0 on success (`crates/kebitx-build/src/bin/filter_example.rs`
is a complete one). An export only replaces folders a build created; anything else at the
target is refused, never deleted.

### Analysing a Java resource pack or mod

```bash
kebitx analyze "Faithful 32x - 26.3.zip"           # a resource pack, folder or .zip
kebitx analyze FarmersDelight-1.21.1-1.3.4.jar     # a Fabric, Quilt, Forge or NeoForge mod
```

Archives are read in place, never extracted. Every verdict is proven against the pack and the
installed Java game's `.jar`. The version is the one whose format the pack declares
(`pack_format`, `supported_formats`, or `min_format` to `max_format`), else the newest stable
one, and the report says which. Kebitx checks:

| Broken (exit 1) | Why the game fails |
|---|---|
| a model whose `parent` does not exist | the model cannot load |
| a texture a model names that does not exist | the missing texture shows |
| a blockstate or item definition naming a missing model | the missing model shows |
| a face asking for `#variable` that its chain never sets (in a model a blockstate or item uses) | the missing texture shows |
| an animated texture whose image does not divide into its frames | the game refuses the sprite |
| a `.png` that is not a PNG | the game checks the signature (`PngInfo.validateHeader`: "Bad PNG Signature") |

Two checks are information only. A language that lacks keys `en_us` has is one, because the
game falls back to English. A replacement texture in other proportions than the game's is the
other: a uniform HD scale keeps the proportions, and animated strips are not compared.

What can be proven depends on the namespace:

- **`minecraft`** is checked against the pack and the game's `.jar`.
- **A mod's own namespace (its id)** is checked against the mod, which ships all of it.
- **Any other namespace** belongs to another mod and cannot be proven from here. Those
  references are counted as unchecked and never reported.

Sprites that the atlases generate without a file of their own name count as existing: armour
trim permutations, renamed `single` sprites and `unstitch` regions.

The test that keeps it honest is the game's own assets, analysed as a pack: Java 26.2 (20 873
references) and 26.3 (23 517 references) both give no finding at all.

On real content:

- **Faithful 32x for 26.3** (3953 textures) is analysed in 0.17 s. Nothing is broken, and one
  texture is reported as information: `particle/noxious_gas_08.png` is 32×30 while its seven
  sibling frames are 32×32.
- **Farmer's Delight** (NeoForge, declares Minecraft `[1.21.1]`) has 1591 references checked
  and 0 broken.

**A mod's code.** Kebitx reads class files itself (std only, hostile input is an error,
never a panic) and checks a mod's code against the game version. A failure here crashes the
game: `NoSuchMethodError` when the line runs, or a failed Mixin at start-up. It checks:

- every class, field and method of the game that the mod names;
- every `@Mixin` target.

Since 26.x, Java's `.jar` is not obfuscated, so the names a mod is compiled against are the
names in the `.jar`. A mod built for an obfuscated version names Fabric's intermediary classes
(`class_1234`); Kebitx says so and compares nothing.

```bash
kebitx analyze lithium-fabric-0.25.3+mc26.2.jar               # against 26.2, the version it names
kebitx analyze lithium-fabric-0.25.3+mc26.2.jar --game 26.3   # would it run on 26.3?
```

The version is the one the mod names, when it names exactly one that is installed (`~26.2`,
`26.3.x`, `[1.21.1]`). Otherwise it is the newest stable one, or the one you choose with
`--game`.

Members are looked up as the JVM does: the class, then its superclasses and interfaces.
Something is reported missing only when every class on the way is known.

- **Known classes:**
  - the game's own classes;
  - `java/lang/Object`;
  - the mod's classes;
  - the classes of the jars it bundles (`META-INF/jars/`, read in memory).
- **What Mixins add:** a Mixin's interfaces and its own members are merged into the game class
  it targets. That is how Fabric API gives `BlockStateModel` its `emitQuads`, so a mod that
  calls it is not reported.
- **Never claimed:**
  - **A chain through another JDK or library class** is counted as unchecked.
  - **A mod that runs with something that can change the game's classes.** What is not in the
    game's `.jar` is listed as information, with where it may come from. This applies to:
    - Forge and NeoForge, which patch the game;
    - a dependency the mod does not bundle.

The proofs:

| Test | Result |
|---|---|
| the game's code against itself | 26.2: 10 952 classes, 340 963 references; 26.3: 11 383 classes, 350 837 references; all resolve |
| Lithium for 26.3, on 26.3 | 696 classes, 291 Mixins, 2715 references: nothing missing |
| Sodium for 26.3, on 26.3 (9 bundled Fabric API jars) | 2763 references: nothing missing |
| Lithium for 26.2, on 26.2 | 2728 references: nothing missing |
| **Lithium for 26.2, on 26.3** | **6 failures, each confirmed in the two `.jar`s** |

**What a Mixin needs of its target** is read from its annotations. Each kind fails in its own
way when the game changes:

| Annotation | Must exist in the target | When it does not |
|---|---|---|
| `@Shadow`, `@Overwrite` | the member, with the `shadow$` prefix and aliases taken into account | Mixin refuses the class at start-up |
| `@Accessor("x")`, `@Invoker("y")` | the field or method (only explicit names are checked) | Mixin refuses the class |
| `@Inject`, `@Redirect`, `@ModifyArg`, ..., MixinExtras' `@WrapOperation`, ... | the method it goes into: the target's own, or one another Mixin adds, never an inherited one | it matches nothing |
| their `@At(target = ...)` | the call or field it hooks | it matches nothing |

When an injector matches nothing, the outcome depends on `require`, read from the annotation or
else the config's `injectors.defaultRequire`. With 1 or more, the game does not start (broken).
With 0, the injection silently does nothing (information). Selectors with wildcards or
quantifiers are not judged.

With these checks, Lithium for 26.2 on 26.3 gives **17 failures**. Here is a sample, confirmed
with `javap` (the JDK's own disassembler, run from the launcher's Java 25):

| Mixin needs | 26.2 | 26.3 |
|---|---|---|
| `@Shadow LivingEntity.swinging`, `swingTime`; `@Inject updateSwingTime()` | there | gone (a `SwingState` now) |
| `@Shadow MobSpawnSettings.spawners` | there | gone |
| `@Redirect` into `ServerChunkCache.tickChunks(ProfilerFiller, long)` | there | now `tickChunks(ProfilerFiller)` |
| `@Inject` into `BrewingStandBlockEntity.serverTick(Level, ...)` | there | now takes a `ServerLevel` |

Lithium and Sodium for 26.3, on 26.3, still give nothing broken (Lithium: 3594 references and
Mixin needs). Sodium has one injection that matches nothing in 26.3 and carries `require = 0`
on purpose; it is reported as information.

**A whole `mods/` folder.** Point `kebitx analyze` at a folder of `.jar` files and it is
analysed the way the loader sees it.

- **The game version:** the installed version every mod accepts, else the newest stable one, or
  the one you choose with `--game`.
- **Each mod:** its assets and its code, analysed against the game and the other mods. A
  dependency another mod of the folder provides is then proven too: its classes and its Mixins
  count.
- **What the loaders refuse to start with:**
  - the same mod id in two jars;
  - a required dependency nothing provides (jars bundled inside a mod count, as the loader loads
    them);
  - a dependency, `minecraft` included, in a version the requirement does not accept;
  - a mod another declares it breaks (Fabric `breaks`, NeoForge `incompatible`).

  Requirements are read as each loader reads them: Fabric's predicates (`~26.3-`, `>=0.19.2`,
  `26.3.x`, `^1.2`) and Maven ranges for Forge and NeoForge (`[1.21.1]`, `[1.0,2.0)`). The
  loaders' own versions and Java's are not in the folder and are not judged.
- **Information:**
  - two mods' Mixins that `@Overwrite` the same method, where only one change survives;
  - mods for several loaders in one folder;
  - jars with no mod metadata.

On real mods for 26.3:

- **Fabric API, Lithium, Mod Menu and Sodium:** nothing broken, in 2.2 s. Mod Menu needs four
  Fabric API modules and bundles them itself.
- **Two Lithiums, Sodium, Mod Menu and Farmer's Delight:** three refusals:
  - `lithium` in two jars;
  - Lithium 0.25.3 needs `minecraft ~26.2`;
  - Farmer's Delight needs `[1.21.1]`.

  It also reports Lithium 0.25.3's 17 code failures, and that Fabric and NeoForge mods are mixed.

**Repairing a mods folder.** The rules are the same as for Bedrock:
- a preview first;
- a backup next to the folder, never inside it, where a jar would load;
- a second analysis that keeps the result only if it is strictly better. Every problem left
  must have been there before, and at least one must be gone.

```bash
kebitx repair <mods folder>                           # preview
kebitx repair <mods folder> --apply                   # duplicates (the default)
kebitx repair <mods folder> --fix incompatible --apply
kebitx repair --undo "<mods folder>.kebitx-backups/<id>"
```

| Fix | What it does |
|---|---|
| `duplicates` (default) | of two jars with the same mod id, keeps the highest version the game accepts and takes the others out; when that cannot be decided (versions that do not read, a tie), nothing is proposed |
| `incompatible` | takes out mods whose `minecraft` requirement rejects the game's version |

Taking out a mod another one needs makes things worse, so it is undone on its own.

On a copy of the bad set above:
1. `duplicates` took out Lithium 0.25.3: 20 problems → 1.
2. `incompatible` took out Farmer's Delight: 1 → 0, and the folder then analyses clean.
3. `--undo` brought both jars back, byte for byte.

The last row of the first table shows what changed between 26.2 and 26.3. `RedStoneWireBlock` is now
`RedstoneWireBlock` (two Mixin targets and a constructor), and
`AbortableIterationConsumer$Continuation` and `FuelValues` are gone. That is why Lithium shipped
a release for 26.3.

### Comparing two versions

```bash
kebitx compare <before> <after> [--all]
```

Problems that are new in `<after>` (worked before, fail now: exit 1), problems fixed, and what
`<before>` defined that `<after>` no longer does — a texture, model or animation another pack
or a world may still use. Run it before publishing an update.

A problem counts as fixed only while its pack is still there. If a whole pack is gone (packs
are known by their manifest UUID, so a renamed folder is not), its problems are listed as lost
with it, never as fixed, and the missing pack is a regression (exit 1).

**Worlds converted by Chunker.** Checked on a real world: the reference world's 28 packs on
real chunks, Bedrock → Java 26.3 → Bedrock 1.26.50 with Chunker 1.21.0. The world that comes
back has its chunks and `level.dat` but none of its packs, and no pack lists (Chunker converts
worlds, not packs). `compare` reports it as a regression: 28 packs gone, 123 problems lost with
them, 0 fixed. With the packs copied back in, `analyze` prints exactly what it printed for the
original world, line for line, and `compare` finds no change. A converted world is checked as
any world, once its packs are put back.

### Repairing

```bash
kebitx repair <world>                         # preview: lists every change, writes nothing
kebitx repair <world> --apply                 # case fixes (the default repair)
kebitx repair <world> --fix case,unused --apply
kebitx repair --undo "<world>.kebitx-backups/<id>"
```

A repair writes nothing without `--apply`. Then it backs up every file it will touch (next to
the world, never inside it), replaces exact quoted literals in place — files are never
re-serialized, so comments, order, tints and `variations` stay byte for byte — and analyses the
world again. Unless the result is strictly better (nothing worse, something better), every file
is put back. On the reference world this caught a real bug in the orphan pass: deleting the
"unused" files broke one reference, the repair rolled itself back, and the world stayed
identical.

| Repair | What it does |
|---|---|
| `case` (default) | writes a reference with the same case as the file on disk (works on Windows, breaks on Android/iOS) |
| `unused` | deletes textures and sounds nothing references; needs the game catalog |

### Migrating a Java resource pack

```bash
kebitx migrate <java_pack_folder_or_jar>           # the compatibility report, writes nothing
kebitx migrate <java_pack_folder> --all            # every file and sound event: where it goes, or why not
kebitx migrate <java_pack_folder> --out <new_folder>
kebitx catalog [--list stems|flipbooks|tiles|sounds]   # what the installed game provides, as kebitx sees it
```

Every file of the Java pack gets a verdict: **converts**, **adapts** (convertible, but needs a
rule kebitx does not have yet: models, blockstates, GUI sprites, ...) or **rebuild** (shaders,
other namespaces: mod content). Only what converts is written, into a folder that must be new or
empty, with a generated `manifest.json` whose UUIDs are stable (the same pack converted twice is
an update, not a second pack). The result is analysed again before the command ends. It needs
the installed Bedrock game — its own files are the only proof of where something belongs — and,
for sounds, pixels and strings, the installed Java game.

**Textures.** A texture converts when Bedrock has a file at the same path; or when kebitx's
name table renames it (`block/oak_planks` → `blocks/planks_oak`, `light_gray` → `silver`,
`dark_oak` → `big_oak`, spawn eggs, buckets, music discs, ...) and the game confirms the rename:
the file exists AND the game's own terrain or item atlas shows it (Bedrock ships both
`boat_dark_oak` and `boat_darkoak`; only the one the atlas uses is right); or by a file name
unique among its kind. Anything weaker than the exact path must also:

- stay on the same subject (`equipment/humanoid_leggings/gold` is not a rabbit's `gold`),
- not be claimed by two Java files (`player/slim/alex` and `player/wide/alex` → one `alex.png`),
- have the same proportions as Bedrock's file. Java's `zombie.png` is 64×64, Bedrock's is
  64×32: the faces are laid out differently and copying it would scramble the model. A uniform
  scale (an HD pack) keeps the proportions and converts.

**Animated textures** become flipbook entries, tick for tick: per-frame times turn into one
`ticks_per_frame` (their greatest common divisor) with each frame repeated, `interpolate` into
`blend_frames` (always written: Bedrock blends by default, Java does not). The tile comes from
the game's own flipbook for that texture, else the one terrain tile that shows it. Frames that
are not a single column of squares, and textures outside the terrain atlas, stay to be adapted.
A `.mcmeta` that only holds settings (`mipmap_strategy`, `blur`) is not an animation.

**Sounds.** Java and Bedrock name their events differently (`entity.bat.ambient` is
`mob.bat.idle`), and no name rule is proof. The files are: a Java event maps to a Bedrock event
only when the Java game's own `sounds.json` (read from the launcher's asset index) gives it a
set of files that exactly one Bedrock event plays, and no other Java event does. Shared sets
(stone steps and stone digging play the same files) map to nothing. A mapped event is rewritten
as Java would play it — the pack's sounds with `replace`, Bedrock's plus the pack's without —
keeping Bedrock's category and subtitle; `volume` (up to 1), `pitch`, `weight` and `stream`
carry over, anything else (`attenuation_distance`, event references) stays to be adapted. An
event that plays what Java plays anyway writes nothing.

**Pixels.** A name is a hint; the picture is the proof. kebitx reads the Java game's own
textures from its `.jar` (the installed version whose `version.json` declares the pack's
format) and every texture of the Bedrock game, and compares them as seen (colour under full
transparency ignored). Three proofs, each unique on both sides — a picture either game has twice
proves nothing:

- **whole**: the Java picture is a Bedrock one at another path (`gui/hanging_signs/acacia` is
  `ui/hanging_sign_acacia`); the pack's texture is copied there;
- **cut**: Bedrock's picture is an aligned region of Java's — the top half of Java's 64×64
  `zombie.png` is Bedrock's 64×32 one — and the pack's texture is cut the same way (HD packs
  too, scaled);
- **cell**: the Java sprite is an aligned cell of a Bedrock sheet — Java's particle sprites in
  Bedrock's `particles.png` — and the pack's sprite is pasted into a copy of the game's sheet.

A pack picture identical to the Java game's writes nothing: Bedrock already has it.

When Bedrock REDREW a texture (other colours) on the same layout, the **shape** of what is drawn
proves the layout: same size, the same pixels transparent and opaque, unique in both games and
with enough shape to mean something (at least 256 pixels drawn, 5 % drawn and 5 % empty; a small
icon's outline can coincide). The pack's texture then fits Bedrock's file. That is how the 37
banner patterns convert — and the shapes show that Java's `diagonal_left` is Bedrock's
`banner_diagonal_up_left`: the editions' names are crossed, a name rule would have got it wrong.
Bedrock's material maps (`_mers`, `_normal`, `_heightmap`) are not colour pictures and are never
matched. Fox, sheep and saddles are drawn on other layouts in Bedrock: no proof, they stay to be
adapted.

**Models, blockstates and other data.** Every other file is compared with the Java game's own at
the same path (the same bytes, or the same JSON written another way). Unchanged, it needs
nothing in Bedrock. Changed or new, the report says what Bedrock needs instead: a Bedrock
resource pack cannot reshape the game's blocks and items, so a changed block model needs a
custom block, a changed item model an attachable.

**Converting models.** A changed or new block or item model with cubes is converted to Bedrock
geometry with rules read off Mojang's own conversion — the Bedrock game ships Java's
`straw_bed_foot` converted, and a test reproduces it cube for cube, face for face from the
installed games: `origin = [8 − to.x, from.y, from.z − 8]`, side faces `uv = [u1, v1]`,
`uv_size = [u2 − u1, v2 − v1]`, up/down faces `uv = [u2, v2]`, `uv_size = [u1 − u2, v1 − v2]`,
face rotation as `uv_rotation`, `#texture` as `material_instance`, `display` as
`item_display_transforms`. Parent chains are followed through the pack, then the Java game.
What that pair does not show is said so: element rotations (their sign under Bedrock's mirrored
x) and `rescale` are converted but marked to check in game; and Bedrock block geometry has no
`cullface` — Mojang removes the faces against a known neighbour by hand (`straw_bed_head`).

Block geometry and its `terrain_texture.json` tiles go into the pack; what uses them goes to
`kebitx_drafts/`, which the game does not load: a custom block for the behavior pack (it ADDS a
block, `kebitx:<name>` — a resource pack cannot reshape the game's own), and for items an
attachable with its geometry (how it sits in the hand is not checked). `kebitx_drafts/README.txt`
lists every model, its unverified elements and any texture with no proven Bedrock file.

**Data packs: recipes.** A Java pack's `data/` is read too. Recipes are compared by what they
mean — the grid of ingredients (padding trimmed), or the bag of them, the result and its count;
for cooking, input, output and stations (Java's one file per station is Bedrock's one recipe
with several tags). Both games ship their own recipes, and a test holds the converter to them:
**861 of Java 26.3's 1275 vanilla recipes, converted, are exactly Bedrock's own**; the rest are
real differences between the editions (Java dyes wool stairs, takes cut sandstone for stairs,
Bedrock makes dyes from its own flowers). The same pairs prove 18 names the editions spell
differently (`sugar_cane` → `reeds`, `bricks` → `brick_block`, `nether_star` → `netherstar`,
...), and only those are renamed.

A pack recipe the game already has writes nothing. A changed vanilla recipe is written with the
identifier of Bedrock's own recipe for it (the one its Java original converts to), so it
REPLACES it instead of adding a second way. A recipe naming an item or tag Bedrock's own
recipes never name stays for a person. Recipes go to a behavior pack next to the output,
`<out>_behavior_pack/`, with its own manifest (module `data`). Advancements, functions and world
generation unchanged from the Java game need nothing; changed ones are reported by kind.

**Tags.** Bedrock cannot add items to the game's tags. What it can do is take the items: a recipe
naming a tag Bedrock does not have is spelled out with the tag's Java items — the pack's tags,
else the Java game's, nested tags followed — one Bedrock recipe per item; a cell that takes
several alternatives is several recipes too (at most 64). Bedrock's own item list (its creative
catalog) says which items exist.

**Loot tables**, the same way: compared by meaning (pools, rolls, entries, weights, counts,
looting, smelting, conditions), with the Java 26 format (`modifier`, one or a list; `uniform`
counts) and Bedrock's. **84 of the 192 tables both games have at the same path mean the same**;
the rest are real differences (Bedrock's chests, double slabs in tables of their own, exploration
maps). Two readings come from Mojang's own tables, not from guessing: Java's
`survives_explosion` on a pool of single drops is Bedrock's `explosion_decay` (with one item they
are the same chance), and Java's smelting test — on fire, or a weapon that smelts loot — is
Bedrock's `on_fire`. The tables prove five names (`cod` → `fish`, `mutton` → `muttonRaw`, ...);
a pair lined up only because the editions list entries in another order proves nothing, and a
name either game also uses as itself is never renamed (that is how `iron_horse_armor` →
`copper_horse_armor` is kept out). A changed vanilla table replaces Bedrock's at the same path
(the report says whether the originals were proven the same); a function or condition with no
proven Bedrock form keeps the table for a person.

**OptiFine entity models** (`optifine/cem/*.jem`, `*.jpm`). Java's entity models are in its
code, so a part has nothing to be compared with — except the pixels its boxes paint: a part
whose boxes paint exactly what one bone's cubes paint, in a Bedrock geometry of that entity with
the same texture size, is that bone. `kebitx_drafts/cem/<entity>.txt` lists each part and the
bone it is proven to be. A cow's head and body are; its legs are not (Bedrock's four legs paint
the same pixels: which is which has no proof). Positions and animations are not converted:
OptiFine's axes have no pair in the games to check a transform against, and its expressions
are not Molang.

**Mods.** `kebitx migrate` also takes a `.jar` (or a `.zip`): it is extracted to a temporary
folder first, and an entry whose name would land outside it (`..`, an absolute path, a drive) is
refused. A mod is recognised by `fabric.mod.json`, `quilt.mod.json` or `META-INF/(neo)forge
mods.toml`; its name and the archive's give the manifests (stable UUIDs, as for packs).

A mod is code plus data. Its code (`.class`) is behaviour to reimplement — counted in
`kebitx_drafts/mod_tasks.txt`, never claimed as automatic. Its data says what its simple items
and blocks look like, and that converts, using only components Bedrock's OWN item and block
definitions use (read from the game's behavior packs):

- a flat item (`item/generated`, `item/handheld`, one layer) becomes a custom item with
  `minecraft:icon`, `minecraft:display_name` as a language key (as Bedrock's apple does) and,
  held like a tool, `minecraft:hand_equipped`; its texture goes to `textures/items/<mod>/` and
  `item_texture.json`;
- a block without states becomes a custom block: its Java model converted to geometry with the
  rules of Mojang's own conversion, `material_instances` (`alpha_test` only when its texture has
  transparent pixels), culling rules from `cullface` — Mojang's `straw_bed_foot` rules are
  exactly its Java cullfaces, ten of ten, and a test holds that — and `minecraft:loot` when the
  mod's loot table for it converts;
- the mod's recipes and loot tables may name its converted items and blocks; its language files
  give their names (`item.<mod>.<name>.name`, `tile.<mod>:<name>.name`) in every language
  Bedrock has.

A block with states is drafted with permutations, written as Bedrock's own `shelf_mushroom`
writes them — the one block both games define as data with states, and a test holds the rules
to it, eight variants of eight: Java's `facing` is `minecraft:cardinal_direction` (the same
values), a variant's `y` is the rotation `[0, (360 − y) mod 360, 0]` (`east`, `y` 90, is 270),
each combination of states one permutation with its geometry. How custom states are DECLARED
(`description.states`, the `placement_direction` trait) no definition of the game shows, so the
block goes to `kebitx_drafts/behavior_pack/blocks/` to be checked and moved, not into the pack;
rotations about x, `uvlock` with a rotation, random and multipart models stay in the task list.
An item with cubes is drafted as an attachable, like a changed vanilla item model. The code that
changes states (a crop growing, a lamp lighting) is Java: reimplement it.

**Functions** (`.mcfunction`). No file of either game pairs a Java command line with its
Bedrock form, so they are not translated. What is known is whether each command exists in
Bedrock — the game's `en_US.lang` names all 115 of its commands — and whether a line uses what
Bedrock's commands have no form for (NBT, item components in Java's form, macros).
`kebitx_drafts/functions/` lists every line: OK, or what to do.

References are read only when the pack needs them: recipes and loot tables when it has `data/`,
entity geometry when it has OptiFine models.

**What stays, with the evidence.** Fox, sheep and saddles: not one island of Bedrock's pixels
appears anywhere in Java's picture (0 of 2084 for the strider saddle, which has the same size) —
Bedrock redrew them on other layouts, and Java's layout lives in its code, not its files. 401
GUI sprites (47 of them nine-slice) match no Bedrock picture by pixels, shape or cell: Bedrock's
interface is drawn from other textures.

**Languages.** `lang/es_es.json` becomes `texts/es_ES.lang`. A Java key and a Bedrock key are the
same string when their English (`en_us.json` from the `.jar`, the game's `en_US.lang`) is
identical and unique on each side — or, for a text both use more than once ("Apple" is also a
join-code icon), unique within one family of keys (`block.` / `tile.`, `item.`, `entity.`,
`enchantment.`, `biome.`). Identical English means identical placeholders, so translations carry
over as they are; strings the game already says identically are not written.

On the Java 26.3 client assets: 10 226 of 11 605 files convert or need nothing (1197 textures by
path, 685 by a confirmed rename, 529 by identical pixels, 47 by layout, 186 cuts and cells with
nothing to change, 52 by name, 53 animations, 7419 data files unchanged), 1379 adapt; the
converted pack analyses with 0 broken references, in 7 seconds.
Of the 1991 Java sound events, 633 are proven to be Bedrock events; of the 8559 English strings,
3978. Underneath, the core reads PNG, TGA, DEFLATE and ZIP itself (`kebitx_core::codec`, std
only): every entry of the 26.3 `.jar` is read with its CRC-32 checked and every PNG decoded, as
a test.

The PNG decoder handles interlaced (Adam7) images, and skips an ancillary chunk with a bad CRC
as the specification allows; 41 of Bedrock's own textures needed one or the other. On 287 of
the game's PNGs, those 41 included, it gives the same RGBA pixels as javax.imageio, the JDK's
own decoder.

`cargo test --workspace` shows 229 green (in the pure and the native build) (plus the Zig consumer's own `zig build test`), among them the core's fixture tests, the repair tests, the migration tests and the builder's
end-to-end tests (a real filter program, every export target, the safety rule). Fixture tests use an **empty** vanilla
catalog, or a fake install built from `fixtures/fake_install`, so their verdict does not
depend on which Minecraft is installed on the machine. That is also why `horse_pack_broken` is
broken in the tests but clean from the command line: the texture it lacks is one the game ships.

## Behavior packs

`kebitx analyze` checks a behavior pack alone, or those a world uses against each other. It
follows what the files name, in entities, items, blocks, recipes, loot and trade tables, functions,
spawn rules, feature rules, animations and scripts.

| Verdict | Means | What falls here |
|---|---|---|
| **MISSING** | Broken: it counts, and the exit code says so | A script file or a script's relative import, a game module not asked for in the manifest, a function, a server animation or controller, a recipe symbol with no key |
| **CASE** | Works on Windows, not on Android or iOS | Any of those, or a table, written with the wrong case |
| **DEAD** | It names what does not exist, and that part does nothing. Reported, not counted | A component group, an event, a loot or trade table; an item, block, entity or feature in a namespace the packs here own |
| **Unverified** | Cannot be known from here. Only counted | A namespace no pack here owns, or anything a dependency that cannot be found could provide |

A world in the game's `minecraftWorlds` is read as the game loads it: the packs in its folder, and
those its lists activate from the game's folders (with what they depend on). A repair only writes
to the packs in its own folder: installed ones are shared with other worlds. A copy of a world
elsewhere has only what it carries.

Why two levels: the game's own entities name groups, events and loot tables that do not exist, and
run (`entities/fox.json` names `loot_tables/entities/fox.json`). A test runs the checker over the
game's files, and none of them may read as broken. `minecraft:` items, blocks and entities are never
judged: the game ships no list of them.

Not checked: what depends on a resource pack (icons, geometry), custom components in scripts, and
what scripts do when they run. Severity was decided from the files, not by playing.

## Worlds

`kebitx world <folder>` reads what a Bedrock world has saved: its chunks by dimension, how many of
each block, entity and block entity, its players, and what its size goes on. The app does the same
with **Inspect world**. It is kebitx's own reader of the game's LevelDB, checksums and Mojang's
compression included, with no library. It only reads: a world is never written. What cannot be read
is counted and said, and a format it does not know is never taken for empty.

It does not convert anything yet. This is the first step to converting worlds between editions with
kebitx's own code: see Phase 9 in [ROADMAP.md](ROADMAP.md). It has been tried on one real world, in two
versions of the game, where every block counted adds up to exactly what its chunks must hold.

## Desktop app

An early desktop app for Windows comes in the Windows archive of each release, and lives in
`apps/desktop`. It is C on Win32, with no toolkit and no
third-party library: 390 KB, plus the engine. It shows the same findings as `kebitx analyze`, with
its progress and a Cancel button.

Its first screen lists what it can check on this computer: resource and behavior packs, worlds that
use packs or have saved data, and the Java mods folder and resource packs. Double-click one. For anything else,
type or paste a path (with autocomplete), use Windows' own file dialog, which has the game's folders
in its side bar, or drop a folder on the window. When nothing could be checked, it says so and why:
it never reads as "OK". That covers a folder that holds no pack, a world with no packs, and a
behavior pack that names nothing.

What was opened lately comes first on that screen. Each report opens with its verdict in colour and
tiles that count each kind of finding; a click on a tile narrows the list, and so does the filter. A
finding's details say what is wrong and, apart, how to fix it. Findings come grouped by what to fix,
what touches most first: thirty entities naming one missing texture are one row, with all thirty in
its details. The next step stands out: first **Preview repair**, then **Apply repair**. **Copy report**
puts everything on the clipboard, to paste anywhere, **Show folder** opens it in Explorer, and
**Package** makes a pack a `.mcpack` and a world a `.mcworld`, one file ready to open in the game, and
**Inspect world** shows what a world has saved.

It speaks English, Spanish, Portuguese, French, German, Russian and Chinese, findings included. It
starts in your system's language, and the choice stays. The window has a dark and a light theme, or
follows the system's, and every look is drawn on Windows' real controls, so the keyboard and screen
readers work as anywhere.

The texts live in `crates/kebitx-core/lang`, one file per language, with each line the translation of
the same line of `en.txt`; a test refuses a file with a line or a value missing. They were written
without a native review: corrections are welcome. The CLI stays in English.

The app keeps its language, its theme and what was opened lately in `%APPDATA%\kebitx`.

Keys: Ctrl+O a folder, Ctrl+Shift+O a file, Ctrl+L the path, Ctrl+F the filter, Enter to go, F5 to
analyse again, Esc to cancel or to clear the filter, Ctrl+C on a row to copy it, Alt+Home for the first
screen.

On Linux, `apps/desktop/src/x11.c` is a simpler version with no library but libc: it speaks the X11
protocol itself, and has its own folder browser, but neither this look nor the languages. It has not
run on a real desktop yet: CI runs it under Xvfb, and it is not in the release archives.

It repairs with the same rules as the CLI, with the safe fixes only (wrong case in a Bedrock pack or
world, duplicate jars in a mods folder): a preview that writes nothing, a backup, a second analysis
that decides, and an undo. It applies only what the preview showed: if the folder changed since,
it refuses and asks for a new preview.

To build it yourself:

```bash
cargo build --release -p kebitx-ffi
cd apps/desktop
zig build --release=safe     # Zig is the C compiler here
cp ../../target/release/kebitx_ffi.dll zig-out/bin/
zig-out/bin/kebitx-app.exe
```

Linux, macOS and Android come next: see Phase 8 in [ROADMAP.md](ROADMAP.md).

## Platforms

kebitx is std-only Rust (plus the optional Zig consumer), and runs wherever its packs and
worlds are: Windows, Linux, macOS, Android (from a terminal on the device). What it needs from
the system is only where the games keep their files:

| | Windows | Linux / macOS / Android |
|---|---|---|
| Bedrock's own files (the catalog) | the GDK install (`XboxGames`) or the UWP package, newest version | `KEBITX_BEDROCK_DIR` |
| Java's own files | `%APPDATA%\.minecraft` | `~/.minecraft`, `~/Library/Application Support/minecraft` |
| catalog cache | `%LOCALAPPDATA%\kebitx\cache` | `~/Library/Caches`, `$XDG_CACHE_HOME` or `~/.cache` |
| `com.mojang` (builds) | the launcher's, or the UWP package's | Android's, or `KEBITX_COM_MOJANG` |

`KEBITX_BEDROCK_DIR` takes any of the layouts the game's files come in: an installed client
(`data/resource_packs`), a **Bedrock Dedicated Server** (`resource_packs` at its root — Linux
or Windows), or **Mojang's published samples** (`resource_pack`, `behavior_pack`). Pointed at a
client's `data` folder, which is a server's layout, the catalog is identical field for field to
the installed one. A GDK install's version is read from its `MicrosoftGame.Config`. Without a
source, every verdict that needs the game says so instead of guessing.

The code that only builds on Windows (the registry, the package manager) is behind
`cfg(windows)`. Clippy is clean, warnings as errors, for Windows, Linux x86_64 and ARM64 (glibc,
musl), Android and macOS; Linux binaries link with `zig cc` (`native/zig/cross`). CI runs every
test on Windows, Linux and macOS. On this machine the tests ran on Windows only: the other
systems are proven to build, and their test runs are CI's.

## Performance

Measured on the reference world (29 packs,
13 000+ files) on a 12-thread machine, each stage in isolation:

| Stage | Before | After |
|---|---|---|
| game catalog (1123 bundles) | 978 ms | 233 ms |
| references | 731 ms | 132 ms |
| compatibility | 924 ms | 179 ms |
| graphics | 824 ms | 153 ms |
| unused assets | 1608 ms | 231 ms |
| unused definitions | 1503 ms | 210 ms |
| identical files | 524 ms | 91 ms |
| **whole `kebitx analyze`, wall clock** | **7.6 s** | **0.47 s** |

The game catalog costs 233 ms only the first time: it is then kept on disk and loads in ~12 ms.

Where the time went, and what fixed it — measured first, every time:

- **walking folders** cost a system call per file (`is_dir`); the directory listing already
  says what each entry is
- **one core** for thousands of independent files: `par::par_map` (std threads, work taken
  from a shared counter, results back in input order so every report stays identical)
- **finding the game** started PowerShell on every run (~500 ms); the registry, read with
  `reg.exe`, answers in ~10 ms
- **JSON objects** were `BTreeMap`s, a node allocation for every tiny `{"uv": ..}`; now a sorted
  vector with the same behaviour, plus a copy-the-run fast path for strings and Clinger's exact
  fast path for numbers (checked bit for bit against the standard parser)
- **game bundles** are opened reading only their index; contents are fetched when needed
- **stages** run at the same time; their output is printed in the usual order
- **the game catalog** is cached on disk (`%LOCALAPPDATA%\kebitx\cache`), keyed by the install
  folder, the game version and the modification time of its packs: an update rebuilds it. A
  damaged or foreign cache file is rejected whole, never half-loaded; `KEBITX_NO_CACHE=1` skips it
- **each file is read once** per analysis (`snapshot`): several stages read the same entity or
  model, and on Windows the open itself is the cost. Only during `kebitx analyze`; the library,
  the tests and the builder always read the disk

The report is byte-for-byte the same as before, and the same with `KEBITX_THREADS=1`.

```bash
kebitx analyze <world> --timings                          # time per stage + wall clock
KEBITX_THREADS=1 kebitx analyze <world>                   # sequential, for comparison
cargo bench -p kebitx-core                                # fixtures and a synthetic model
KEBITX_BENCH_WORLD=<world> cargo bench -p kebitx-core     # the numbers that matter
```

### Java Edition: the same method

Measured on the game's own `.jar` (26.3: 11 383 classes, 22.3 MB compressed, 60 MB inflated)
and the real mods above:

| Step | Before | After |
|---|---|---|
| reading the classes from the `.jar` (warm cache, 1 core) | 1.24 s | 0.44 s |
| DEFLATE + CRC-32 on the same 11 383 streams (1 core) | 701 ms | 334 ms |
| the game's class index, built whole (all cores) | 430 ms | 182 ms, now lazy |
| `kebitx analyze` on a mods folder (4 mods) | 2.23 s | 0.42 s |
| `kebitx analyze` on one mod (Sodium) | 1.07 s | 0.16 s |

Where the time went, and what fixed it:

- **Every entry opened the archive file again:** 11 383 opens for one `.jar`, 9.1 s on a cold
  cache. Now the file is opened once and shared. Only the seek and read run under its lock;
  decoding runs outside it, on every core.
- **DEFLATE decoded Huffman codes one bit at a time** (zlib's reference `puff` method). A
  10-bit lookup table per code now finds most symbols in one step. Long codes and the end of
  the stream keep the old path, so errors on hostile input are unchanged. Copies that do not
  overlap are done in one block.
- **CRC-32 went byte by byte,** at 493 MB/s. Now it works eight bytes at a time ("slicing by
  8", tables built at compile time): 2 GB/s, with the same checksums.
- **A mods folder read each class three times, one core at a time.** Now classes are read once,
  in parallel. The game's index is built while the mods are read, and the Overwrite check
  reuses the classes already read.
- **The game's index queued on one file handle, then built its tables in order.** Now the
  `.jar` is read into memory once (about 30 ms), and every core reads, decodes and builds the
  tables of its classes. Only the final inserts run in order: 350 ms → 182 ms.
- **Then the whole index stopped being built.** A mod names a small part of the game, so the
  index now lists the `.jar`'s names and reads a class the first time something asks about it.
  A test checks every class of every installed game: its entry name is the class it holds.
  One mod (Lithium): 316 → 165 ms. A mods folder: 635 → 477 ms.

The reports are identical before and after: the same 3 refusals and 17 failures on the bad set,
and the same 2982 and 3594 references for Sodium and Lithium.

### DEFLATE: why kebitx has its own

Every decoder was measured on the same input: the 11 383 class files of the 26.3 `.jar`, each
inflated to its known size and checked against its CRC-32, on one core.

| Decoder | Language | Speed |
|---|---|---|
| `std.compress.flate` | Zig | 82 MB/s |
| kebitx, first version | Rust | 85 MB/s |
| kebitx, with a lookup table | Rust | 187 MB/s |
| zlib 1.3.2 | C | 260 MB/s |
| **kebitx, now** | **Rust, no `unsafe`** | **313 MB/s** |
| the same loop without bounds checks | Zig | about 4 % more |
| libdeflate 1.26, called from kebitx | C | 436 MB/s |

libdeflate reached 597 MB/s in its own C benchmark, with no allocation around each call. 436 MB/s
is the figure on the same harness as kebitx's 313 MB/s.

What the time went to, measured before each change:

- **Building the Huffman tables** was 28 % of the decoding. Now each code is written once and
  the table grows by doubling, and the tables are kept per thread.
- **Bit-by-bit refills and one symbol per loop.** The fast loop refills 64 bits at once and takes
  up to three literals per round.
- **Copies.** Most matches are short, so the first words are copied with no branch between them.
- **Bounds checks were not the problem:** the same loop in Zig, without them, gained about 4 %.

Anything not simple (the end of a block, a long code, an error, the output's limit) puts the state
back as it was before that round, and a careful path that checks everything decides. So the fast
loop has no verdict of its own. `crates/kebitx-core/tests/native.rs` proves both paths give the
same bytes, the same input used and the same error, on 150 streams from another compressor, on
damaged streams and on 20 000 random inputs.

libdeflate is still faster in isolation, but on real commands the difference is small:

| Command | kebitx alone | with libdeflate and the Zig kernel |
|---|---|---|
| `analyze` on one mod (Sodium) | 152 ms | 155 ms |
| `analyze` on a mods folder (4 mods) | 424 ms | 407 ms |
| `migrate` on Faithful 32x | 3.21 s | 3.09 s |

That is not worth third-party C on 7 targets, so it was removed.

### Native kernel: PNG unfiltering in Zig

One optional kernel remains, and it is kebitx's own code. Undoing the PNG filters with Zig's
`@Vector` runs at 1 228 MB/s on baseline x86-64, against 651 MB/s for the specialised Rust, on
4 747 textures of Bedrock (186 MB). Zig brings portable SIMD here; Rust's `std::simd` is nightly only.

```bash
cargo build --release -p kebitx-cli --features native    # needs Zig 0.14 on the PATH (or ZIG=...)
```

On whole commands it does not show: `migrate` on Faithful 32x takes 3.19 s without it and 3.20 s
with it. So **the prebuilt binaries are pure Rust**, and need no Zig to build. The feature stays
for where unfiltering dominates, and is tested in CI on Windows, Linux and macOS.

- **The boundary:** `crates/kebitx-native` holds it, and it is the second crate with `unsafe`.
  Without the feature it holds nothing.
- **The proof:** `crates/kebitx-core/tests/native.rs` compares both paths on every filter case, and
  the reports are identical in both builds.

### `kebitx migrate`, stage by stage

| `kebitx migrate` on Faithful 32x (3 953 textures) | Before | After |
|---|---|---|
| extracting the `.zip` | 1.84 s | 0.95 s |
| the game's 8 218 textures (`BedrockPixels`) | 1.47 s | 0.62 s |
| matching the pack's pictures (`inspect`) | 2.15 s | 0.49 s |
| **the whole command** | **6.9 s** | **3.2 s** |

The full report (`--all`, 5 104 lines) is identical before and after. What was found in each
stage:

- **Extracting:** files were created one after the other. Now every name and the total size
  are checked first, from the archive's directory, so hostile archives are still refused before
  anything is written. Then every core decodes and writes.
- **Reading the game's textures:** `map_files` gave each bundle of the game to one core, with
  thousands of textures inside, while the other cores waited. Now the bundles are read first
  and each file is decoded on any core: 1.3 s → 0.3 s.
- **Matching:** 494 GUI sprites took 21.7 s of CPU between them. Looking for a sprite as a cell of
  the game's sheets cropped every cell into a new picture to hash it, and built each cell size's
  index under one lock. Now regions are hashed in place (a test proves the same hashes as the
  crops), each cell size has its own slot (`OnceLock`), and the files are judged on every core.

## Validated on a real world

29 packs, 13 of them resource packs. Run against the packs *before* any hand fixes, kebitx found
on its own the defects that had been hunted manually: 6 case mismatches (`desert_3` vs
`Desert_3.png`), 24 broken `variations` entries in `smooth_stone` / `double_stone_slab`, and a
projectile texture that did not exist. On the fixed world: 697 entities, 906 geometries,
1833 atlas entries, 0 broken — with 201 textures correctly classified as `External`.
2820 render-controller slots checked (the game's controllers included), 0 missing.
Compatibility over all 29 packs: 0 errors, 20 warnings — among them three items or blocks whose
file reuses another one's identifier, so one of each pair never exists in game.
Assets: 5099 textures and sounds, 348 unused (4.1 MB) and 166 groups of identical files
(0.92 MB in extra copies). Every unused file checked by hand was really unused — 88 bullet
sounds no sound definition plays, a leftover ostrich texture, and leftovers in the world's own
packs. Definitions: 3209, of which 1245 unused; an independent text search over every file
outside the definition folders finds none of them.
Graphics: 1385 models and 46942 cubes; 113 models with UVs outside their canvas (a sign whose
back face samples nothing, a donkey copied from a larger model with its saddle off the image),
and 1 stretched texture among 400 texture/model pairs. 1236 entity animations compared bone by
bone: 49 move nothing (a penguin whose head bone is named `cabeza`, 16 misplaced backpacks,
corpses without their blood bones). Materials: 896 references, 2 missing.
With the animation index: 2862 animation references, 125 missing on the client — every one a
defect shipped by an addon, 10 of them a controller that only a behavior pack defines.

## Architecture

### Languages

**Rust is the backbone.** The semantic core, the orchestration, the rules and every diagnosis live
in Rust. Other languages are added **only where they bring something Rust does not**, behind a
C ABI, and only once a measurement says they are needed.

| Language | Role | When |
|---|---|---|
| **Rust** | Semantic core: indexes, reference resolution, dependency graph, diagnostics, repairs, orchestration of native modules | Always — this is the default home for any new code |
| **Zig** | Selective native processing: binary buffers, geometry transforms, format conversion | When a benchmark shows a hot path worth it. Also as the cross-compilation toolchain (Linux / macOS / Android from Windows) and as the reference consumer of the C ABI |
| **C / C++** | The apps of Phase 8: kebitx's own code on each system's APIs, with the engine behind the C ABI | For interfaces. No third-party library, and no toolkit |

Zig is **not** automatically faster than Rust or C++. Performance depends on the algorithm,
memory layout, libraries and measurements. A module moves to native code only with a benchmark
in hand — otherwise the project ends up as three duplicated implementations.

That rule has been applied, and it also works in reverse: code leaves when it stops paying.

- **DEFLATE:** libdeflate (C) was added when it was 3.2× faster than kebitx's decoder. Once
  kebitx's own was within 4 % on real commands, libdeflate was removed.
- **PNG unfiltering:** a Zig `@Vector` kernel is 1.9× faster in isolation, and stays as an option.
  It does not show on whole commands, so the prebuilt binaries are pure Rust.

See *Performance*.

### Layout today

```text
kebitx/
├── Cargo.toml              # workspace
├── crates/
│   ├── kebitx-core/        # library: indexes, resolvers, vanilla catalog, reports
│   │   ├── src/
│   │   └── tests/          # integration tests (shared helpers in tests/common/)
│   ├── kebitx-build/       # the builder: project file, filters, gate, export, ZIP writer
│   ├── kebitx-repair/      # repairs: plan, backup, surgical edits, second analysis, rollback
│   ├── kebitx-migrate/     # Java → Bedrock: report, textures, sounds, models, data, mods
│   ├── kebitx-world/       # worlds: own reader of Bedrock's LevelDB, and what is saved in it
│   ├── kebitx-java/        # Java Edition: the installed game, analysis of packs and mods
│   ├── kebitx-ffi/         # the C ABI (include/kebitx.h); a crate with `unsafe`
│   ├── kebitx-native/      # optional native kernel (`native` feature): the other crate with `unsafe`
│   └── kebitx-cli/         # the `kebitx` executable (analyze, build, repair, compare, migrate)
├── apps/
│   └── desktop/            # the desktop app: C on the system's own APIs, no toolkit (Windows today)
├── native/
│   └── zig/
│       ├── kebitx-native/  # the optional native kernel: PNG unfiltering with SIMD
│       ├── kebitx-check/   # the C ABI's reference consumer, in Zig
│       └── cross/          # zig cc as the linker for Linux x86-64 / ARM64 builds
├── fixtures/               # synthetic packs and worlds, shared by every crate
├── .github/workflows/      # ci.yml (every push and pull request), release.yml (tags)
├── CHANGELOG.md
├── LICENSE
├── README.md
└── ROADMAP.md
```

### Using kebitx from other languages

```bash
cargo build --release -p kebitx-ffi          # target/release/kebitx_ffi.dll (.so / .dylib)
cd native/zig/kebitx-check && zig build      # ReleaseSafe
zig-out/bin/kebitx-check ../../../target/release/kebitx_ffi.dll <pack or world>
```

`crates/kebitx-ffi/include/kebitx.h` is the whole contract: `kebitx_analyze(path)` returns a JSON
report (counts, and every issue with its confidence and fix), freed with `kebitx_string_free`. It also
has what the apps use: reports with the lines people read, repairs, packaging, the inspector and the
languages.

Cross-compiling the CLI with Zig as the linker:

```bash
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=native/zig/cross/zig-cc-x86_64-linux-gnu.cmd \
  cargo build --release -p kebitx-cli --target x86_64-unknown-linux-gnu

# Android (Termux / adb shell): a fully static ARM64 binary
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=native/zig/cross/zig-cc-aarch64-linux-musl.cmd \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C link-self-contained=no" \
  cargo build --release -p kebitx-cli --target aarch64-unknown-linux-musl
```

### How it grows

Every folder appears when it has code in it, never empty in advance.

| When | What is added |
|---|---|
| End of Phase 1 | `kebitx-formats`: the file readers (tolerant JSON, `.brarchive`, atlases) moved out of the core, with `bedrock` and later `java` modules |
| Security track | `fuzz/`: cargo-fuzz targets for the JSON and `.brarchive` readers |
| Phase 2 | `kebitx-models`: geometry, UVs, bones, texture space |
| Phase 3 | `kebitx-repair`: safe, verifiable repairs (done: case, unused) |
| Phase 4 | `kebitx-migrate`: Java → Bedrock (done: report, textures, pixels, flipbooks, sounds, languages, models, data, mods) |
| Phase 7 | `kebitx-java`: Java Edition as a target (started: the installed game, analysis of packs and mods) |
| Phase 9 | `kebitx-world`: worlds with no other tool (started: the reader of Bedrock's LevelDB and the inspector) |
| Phase 6 | `kebitx-ffi`: the C ABI, the only crate where `unsafe` may exist; `native/zig/` as its first consumer (done) |
| A benchmark asks for it | `native/zig/…` for that one hot path, and only while it pays |

Diagnostics stay in `kebitx-core` until the graphics diagnostics of Phase 2 make a separate
analyzer crate worth it. Visual-regression and migration fixtures go under `fixtures/` when
those phases start.

## Limits worth stating up front

**Detecting a visual defect is not the same as guaranteeing it disappears in game.** Kebitx can
prove many defects statically. Some depend on the real renderer, on the game version, or on Molang
expressions evaluated at runtime. Those need integration tests and, where feasible, comparison
against reference screenshots — not a static claim.

**Custom shaders.** Community shader packs for Bedrock (`renderer/materials/*.material.bin`) only
take effect with a modified client; the unmodified game ignores them. The official path is
Vibrant Visuals (`atmospherics`, `color_grading`, `lighting`, `water`, …). Kebitx should say so
instead of letting a pack look fine on paper and do nothing in game.

**Geometry is not polygon meshes.** Bedrock models are, in practice, cubes with per-face UVs
grouped in bones. Metrics are about cubes, bones and texture space — not polygon counts.

**Java → Bedrock.** Textures, models, sounds, recipes and data can often be converted or adapted.
Code written for Forge or Fabric cannot be converted automatically: its behaviour has to be
reimplemented. Java shaders have no general Bedrock equivalent. Converting whole *worlds*
is still a job for tools such as Chunker: Kebitx reads Bedrock worlds today, and converting them with
its own code is the goal of Phase 9.

## Contributing

Issues and pull requests are welcome. Before opening a pull request, run the same checks as CI:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# if you touch codec/ or native/ (needs Zig 0.14): the native build must give the same bytes
cargo test --workspace --features kebitx-core/native
(cd native/zig/kebitx-native && zig build test)
```

New findings follow the project's one rule: a problem is reported only when it can be proven
from the files. When a check cannot decide, it stays silent. Add a fixture under `fixtures/`
that shows the case. Notable changes go in [CHANGELOG.md](CHANGELOG.md), under `Unreleased`.

### Releasing

1. Set the version in `Cargo.toml` (`[workspace.package]`) and run `cargo check`, which updates
   `Cargo.lock` (the release builds with `--locked`).
2. Move the `Unreleased` notes in `CHANGELOG.md` to a new `## [X.Y.Z] - YYYY-MM-DD` section.
3. Commit, then push a tag `vX.Y.Z`.

The [release workflow](.github/workflows/release.yml) refuses a tag that does not match
`Cargo.toml` or has no changelog section. It runs every test, builds every platform, and
publishes **Kebitx X.Y.Z** with the changelog section as its notes. A tag with a suffix, such
as `v2.1.0-rc.1`, is published as a pre-release.

## License

Kebitx is released under the [MIT License](LICENSE).

Minecraft is a trademark of Mojang Synergies AB. Kebitx is not affiliated with or endorsed by
Mojang or Microsoft. It reads the game files you have installed and does not redistribute them.
