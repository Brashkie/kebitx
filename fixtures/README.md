# Fixtures

Synthetic packs (authored here, no vanilla bytes) but structurally faithful to the real cases.
Texture files are empty (0 bytes): the resolver only checks existence. Model files carry a real
geometry identifier so the geometry index has something to resolve.

Shared by every crate of the workspace; tests reach them through `tests/common::fixture`.

| Pack / dir           | textures declared/files | geometry | notes |
|----------------------|-------------------------|----------|-------|
| villager_pack        | 33 / 28                 | 1 present | six `base*` -> one file; `//` comment; new-format model |
| horse_pack           | 17 / 17                 | 1 present | one `.tga` among `.png`; new-format model |
| horse_pack_broken    | 17 / 16 on disk         | 1 present | missing `horse_armor_leather.tga` (the game ships it: clean with the real catalog) |
| models_both_formats  | —                       | 2 (old+new) | one old top-level-key model, one new-array model |
| geo_missing_pack     | 1 / 1                   | 1 absent | entity references a geometry not in models/ |
| atlas_pack           | atlases                 | —        | every atlas shape; one empty set, one case mismatch |
| world_two_packs      | world: rp_a, rp_b, bp_x | —        | rp_a's atlas is satisfied only by rp_b's texture |
| world_anims          | world: rp, bp           | —        | animations, controllers (map + legacy array), render controllers; a controller only the BP defines |
| world_rc             | world: rp_main, rp_override | —    | render-controller slots: direct, arrays (partial / all missing / undefined), expression, per-bone material, case; rp_override redefines one controller |
| build_project        | packs/BP + packs/RP     | —        | a tiny clean add-on for the builder's tests (they copy it to a temp folder and write `kebitx.json` there), and `world/`, a world template for the `mcworld` target |
| world_graphics       | world: rp               | 10 models | box UV overflowing, a flat plane whose box layout would overflow but whose real faces fit, per-face UV overflowing (and a flipped face inside), no declared canvas; images with real PNG headers: exact, HD (uniform scale), stretched, and a flipbook strip under `uv_anim`. Bones: an old-format child inheriting `Head`, an entity with two models, a model defined twice with different bones, a model only the game has; animations that move all, some, or none of the bones. Structure: a bone hanging from a missing parent, a repeated bone name, inverted and empty cubes. Constant and keyframed scales (0, 25×), a Molang one, one flat axis. A compiled shader file and an `atmospherics/` folder |
| world_scripts        | real layout + `level.dat` (Beta APIs off) | — | a beta script module; an invalid module version, a module required twice, a legacy array version |
| world_compat         | real layout: behavior_packs/, resource_packs/, world_*_packs.json | — | one of each compatibility defect, plus identical copies that must not be reported |
| fake_install         | a fake game install     | —        | one render controller, `geometry.cow`, `textures/blocks/stone` and `materials/entity.material` (`entity_static`, `entity_alphatest`) in its resource packs, one animation controller in its behavior packs |
| materials_pack       | 0 / 0                   | —        | an entity naming a game material, a material its own `materials/custom.material` defines (other capitals), and one nobody defines |
| world_assets         | world: rp, rp2, bp      | 5 defined | one asset per rule that makes it "used" (exact, case, UI binding, texture set, engine path, game replacement, sound definition, script-built path, comment); one unused, one named only by `textures_list.json`; identical bytes across packs, and a same-size file that differs. Definitions: used by the entity, by inheritance, by the game, by a `/playanimation` function; one unused of each kind |
| copied_cow_pack      | 0 / 0                   | 1 absent | uses the fake game's `controller.render.cow` without declaring its texture and material |
| optimize_pack        | 3 images               | 2 models | a 1200×1082 pack icon, a 4096×2048 texture, a small one, a JPEG named `.png`; a 500-cube model and a 1-cube one |

The packs above that predate render-controller checking name a placeholder
`controller.render.x`; each defines it in `render_controllers/fixture.render_controllers.json`, so
a fixture carries only the defect it was built for.
