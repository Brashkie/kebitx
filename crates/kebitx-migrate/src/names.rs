//! Java texture names → Bedrock texture names, where the two editions name the
//! same picture differently (`block/oak_planks` is `blocks/planks_oak`).
//!
//! The table only PROPOSES. A proposal is used when the game confirms it: the
//! Bedrock file exists in the installed game, the game's own atlas shows it
//! (for blocks and items), and the report then checks proportions and that no
//! other Java file claims it. A rule that proposes a name the game does not
//! have proposes nothing. When several rules propose different existing files,
//! none is used: the table never picks between candidates.
//!
//! Paths are relative to `textures/`, without extension. `*` captures one
//! part of the name; the capture can be translated (`light_gray` is `silver`
//! in Bedrock's colour names, `dark_oak` is `big_oak` in its oldest wood
//! names, coral types are colours).

use kebitx_core::vanilla::VanillaCatalog;

#[derive(Clone, Copy)]
enum Cap {
    Plain,
    Color,
    Wood,
    Coral,
}

struct Rule(&'static str, &'static str, Cap);

use Cap::*;

/// Exact renames: when one of these names a Java texture, only it is tried.
const EXACT: &[(&str, &str)] = &[
    // ---- blocks
    ("block/andesite", "blocks/stone_andesite"),
    ("block/diorite", "blocks/stone_diorite"),
    ("block/granite", "blocks/stone_granite"),
    ("block/polished_andesite", "blocks/stone_andesite_smooth"),
    ("block/polished_diorite", "blocks/stone_diorite_smooth"),
    ("block/polished_granite", "blocks/stone_granite_smooth"),
    ("block/stone_bricks", "blocks/stonebrick"),
    ("block/mossy_stone_bricks", "blocks/stonebrick_mossy"),
    ("block/cracked_stone_bricks", "blocks/stonebrick_cracked"),
    ("block/chiseled_stone_bricks", "blocks/stonebrick_carved"),
    ("block/mossy_cobblestone", "blocks/cobblestone_mossy"),
    ("block/bricks", "blocks/brick"),
    ("block/nether_bricks", "blocks/nether_brick"),
    ("block/red_nether_bricks", "blocks/red_nether_brick"),
    ("block/end_stone_bricks", "blocks/end_bricks"),
    ("block/sandstone", "blocks/sandstone_normal"),
    ("block/chiseled_sandstone", "blocks/sandstone_carved"),
    ("block/cut_sandstone", "blocks/sandstone_smooth"),
    ("block/red_sandstone", "blocks/red_sandstone_normal"),
    (
        "block/chiseled_red_sandstone",
        "blocks/red_sandstone_carved",
    ),
    ("block/cut_red_sandstone", "blocks/red_sandstone_smooth"),
    (
        "block/chiseled_quartz_block",
        "blocks/quartz_block_chiseled",
    ),
    (
        "block/chiseled_quartz_block_top",
        "blocks/quartz_block_chiseled_top",
    ),
    ("block/quartz_pillar_side", "blocks/quartz_block_lines"),
    ("block/quartz_pillar_top", "blocks/quartz_block_lines_top"),
    ("block/purpur_pillar_side", "blocks/purpur_pillar"),
    ("block/dark_prismarine", "blocks/prismarine_dark"),
    ("block/grass_block_top", "blocks/grass_top"),
    ("block/dirt_path_top", "blocks/grass_path_top"),
    ("block/dirt_path_side", "blocks/grass_path_side"),
    ("block/podzol_top", "blocks/dirt_podzol_top"),
    ("block/podzol_side", "blocks/dirt_podzol_side"),
    ("block/rooted_dirt", "blocks/dirt_with_roots"),
    ("block/farmland", "blocks/farmland_dry"),
    ("block/farmland_moist", "blocks/farmland_wet"),
    ("block/packed_ice", "blocks/ice_packed"),
    ("block/slime_block", "blocks/slime"),
    ("block/wet_sponge", "blocks/sponge_wet"),
    ("block/cobweb", "blocks/web"),
    ("block/lily_pad", "blocks/waterlily"),
    ("block/sugar_cane", "blocks/reeds"),
    ("block/short_grass", "blocks/tallgrass"),
    ("block/dead_bush", "blocks/deadbush"),
    ("block/spawner", "blocks/mob_spawner"),
    ("block/note_block", "blocks/noteblock"),
    ("block/jack_o_lantern", "blocks/pumpkin_face_on"),
    ("block/carved_pumpkin", "blocks/pumpkin_face_off"),
    ("block/redstone_lamp", "blocks/redstone_lamp_off"),
    ("block/redstone_torch", "blocks/redstone_torch_on"),
    ("block/torch", "blocks/torch_on"),
    ("block/comparator", "blocks/comparator_off"),
    ("block/repeater", "blocks/repeater_off"),
    ("block/tripwire_hook", "blocks/trip_wire_source"),
    ("block/tripwire", "blocks/trip_wire"),
    ("block/furnace_front", "blocks/furnace_front_off"),
    ("block/smoker_front", "blocks/smoker_front_off"),
    (
        "block/blast_furnace_front",
        "blocks/blast_furnace_front_off",
    ),
    ("block/dispenser_front", "blocks/dispenser_front_horizontal"),
    ("block/dropper_front", "blocks/dropper_front_horizontal"),
    ("block/observer_back_on", "blocks/observer_back_lit"),
    ("block/piston_top", "blocks/piston_top_normal"),
    ("block/end_portal_frame_top", "blocks/endframe_top"),
    ("block/end_portal_frame_side", "blocks/endframe_side"),
    ("block/end_portal_frame_eye", "blocks/endframe_eye"),
    ("block/honey_block_top", "blocks/honey_top"),
    ("block/honey_block_side", "blocks/honey_side"),
    ("block/honey_block_bottom", "blocks/honey_bottom"),
    ("block/honeycomb_block", "blocks/honeycomb"),
    ("block/beehive_end", "blocks/beehive_top"),
    ("block/fletching_table_top", "blocks/fletcher_table_top"),
    ("block/anvil", "blocks/anvil_base"),
    ("block/anvil_top", "blocks/anvil_top_damaged_0"),
    ("block/chipped_anvil_top", "blocks/anvil_top_damaged_1"),
    ("block/damaged_anvil_top", "blocks/anvil_top_damaged_2"),
    ("block/melon_stem", "blocks/melon_stem_disconnected"),
    ("block/attached_melon_stem", "blocks/melon_stem_connected"),
    ("block/pumpkin_stem", "blocks/pumpkin_stem_disconnected"),
    (
        "block/attached_pumpkin_stem",
        "blocks/pumpkin_stem_connected",
    ),
    (
        "block/brown_mushroom_block",
        "blocks/mushroom_block_skin_brown",
    ),
    ("block/red_mushroom_block", "blocks/mushroom_block_skin_red"),
    ("block/mushroom_stem", "blocks/mushroom_block_skin_stem"),
    ("block/oak_door_bottom", "blocks/door_wood_lower"),
    ("block/oak_door_top", "blocks/door_wood_upper"),
    ("block/oak_trapdoor", "blocks/trapdoor"),
    ("block/dark_oak_sapling", "blocks/sapling_roofed_oak"),
    ("block/large_fern_bottom", "blocks/double_plant_fern_bottom"),
    ("block/large_fern_top", "blocks/double_plant_fern_top"),
    (
        "block/tall_grass_bottom",
        "blocks/double_plant_grass_bottom",
    ),
    ("block/tall_grass_top", "blocks/double_plant_grass_top"),
    ("block/lilac_bottom", "blocks/double_plant_syringa_bottom"),
    ("block/lilac_top", "blocks/double_plant_syringa_top"),
    ("block/peony_bottom", "blocks/double_plant_paeonia_bottom"),
    ("block/peony_top", "blocks/double_plant_paeonia_top"),
    ("block/rose_bush_bottom", "blocks/double_plant_rose_bottom"),
    ("block/rose_bush_top", "blocks/double_plant_rose_top"),
    ("block/sunflower_back", "blocks/double_plant_sunflower_back"),
    (
        "block/sunflower_bottom",
        "blocks/double_plant_sunflower_bottom",
    ),
    (
        "block/sunflower_front",
        "blocks/double_plant_sunflower_front",
    ),
    ("block/sunflower_top", "blocks/double_plant_sunflower_top"),
    ("block/poppy", "blocks/flower_rose"),
    ("block/dandelion", "blocks/flower_dandelion"),
    ("block/blue_orchid", "blocks/flower_blue_orchid"),
    ("block/allium", "blocks/flower_allium"),
    ("block/azure_bluet", "blocks/flower_houstonia"),
    ("block/red_tulip", "blocks/flower_tulip_red"),
    ("block/orange_tulip", "blocks/flower_tulip_orange"),
    ("block/white_tulip", "blocks/flower_tulip_white"),
    ("block/pink_tulip", "blocks/flower_tulip_pink"),
    ("block/oxeye_daisy", "blocks/flower_oxeye_daisy"),
    ("block/cornflower", "blocks/flower_cornflower"),
    (
        "block/lily_of_the_valley",
        "blocks/flower_lily_of_the_valley",
    ),
    ("block/wither_rose", "blocks/flower_wither_rose"),
    ("block/cave_vines", "blocks/cave_vines_head"),
    ("block/cave_vines_lit", "blocks/cave_vines_head_berries"),
    ("block/cave_vines_plant", "blocks/cave_vines_body"),
    (
        "block/cave_vines_plant_lit",
        "blocks/cave_vines_body_berries",
    ),
    ("block/crimson_nylium", "blocks/crimson_nylium_top"),
    ("block/warped_nylium", "blocks/warped_nylium_top"),
    ("block/smooth_stone", "blocks/stone_slab_top"),
    ("block/smooth_stone_slab_side", "blocks/stone_slab_side"),
    ("block/nether_quartz_ore", "blocks/quartz_ore"),
    ("block/terracotta", "blocks/hardened_clay"),
    ("block/item_frame", "blocks/itemframe_background"),
    ("block/lightning_rod_on", "blocks/lightning_rod_powered"),
    ("block/turtle_egg", "blocks/turtle_egg_not_cracked"),
    ("block/bamboo_stalk", "blocks/bamboo_stem"),
    ("block/bamboo_large_leaves", "blocks/bamboo_leaf"),
    ("block/bamboo_small_leaves", "blocks/bamboo_small_leaf"),
    ("block/composter_compost", "blocks/compost"),
    ("block/composter_ready", "blocks/compost_ready"),
    ("block/crimson_stem", "blocks/huge_fungus/crimson_log_side"),
    (
        "block/crimson_stem_top",
        "blocks/huge_fungus/crimson_log_top",
    ),
    ("block/warped_stem", "blocks/huge_fungus/warped_stem_side"),
    (
        "block/warped_stem_top",
        "blocks/huge_fungus/warped_stem_top",
    ),
    (
        "block/stripped_crimson_stem",
        "blocks/huge_fungus/stripped_crimson_stem_side",
    ),
    (
        "block/stripped_crimson_stem_top",
        "blocks/huge_fungus/stripped_crimson_stem_top",
    ),
    (
        "block/stripped_warped_stem",
        "blocks/huge_fungus/stripped_warped_stem_side",
    ),
    (
        "block/stripped_warped_stem_top",
        "blocks/huge_fungus/stripped_warped_stem_top",
    ),
    (
        "block/crimson_door_bottom",
        "blocks/huge_fungus/crimson_door_lower",
    ),
    (
        "block/crimson_door_top",
        "blocks/huge_fungus/crimson_door_top",
    ),
    (
        "block/warped_door_bottom",
        "blocks/huge_fungus/warped_door_lower",
    ),
    (
        "block/warped_door_top",
        "blocks/huge_fungus/warped_door_top",
    ),
    ("block/prismarine", "blocks/prismarine_rough"),
    ("block/nether_portal", "blocks/portal"),
    ("block/campfire_fire", "blocks/campfire"),
    ("block/soul_campfire_fire", "blocks/soul_campfire"),
    ("block/stonecutter_saw", "blocks/stonecutter2_saw"),
    ("block/kelp", "blocks/kelp_top"),
    ("block/rail", "blocks/rail_normal"),
    ("block/rail_corner", "blocks/rail_normal_turned"),
    ("block/powered_rail", "blocks/rail_golden"),
    ("block/powered_rail_on", "blocks/rail_golden_powered"),
    ("block/detector_rail", "blocks/rail_detector"),
    ("block/detector_rail_on", "blocks/rail_detector_powered"),
    ("block/activator_rail", "blocks/rail_activator"),
    ("block/activator_rail_on", "blocks/rail_activator_powered"),
    // ---- items
    ("item/black_dye", "items/dye_powder_black_new"),
    ("item/blue_dye", "items/dye_powder_blue_new"),
    ("item/brown_dye", "items/dye_powder_brown_new"),
    ("item/white_dye", "items/dye_powder_white_new"),
    ("item/ink_sac", "items/dye_powder_black"),
    ("item/lapis_lazuli", "items/dye_powder_blue"),
    ("item/cocoa_beans", "items/dye_powder_brown"),
    ("item/bone_meal", "items/dye_powder_white"),
    ("item/glow_ink_sac", "items/dye_powder_glow"),
    ("item/oak_door", "items/door_wood"),
    ("item/oak_sign", "items/sign"),
    ("item/dark_oak_sign", "items/sign_darkoak"),
    ("item/golden_apple", "items/apple_golden"),
    ("item/golden_carrot", "items/carrot_golden"),
    ("item/golden_spear", "items/spear/gold_spear"),
    ("item/wooden_spear", "items/spear/wood_spear"),
    (
        "item/golden_nautilus_armor",
        "items/nautilus_armor/gold_nautilus_armor",
    ),
    ("item/beef", "items/beef_raw"),
    ("item/chicken", "items/chicken_raw"),
    ("item/mutton", "items/mutton_raw"),
    ("item/porkchop", "items/porkchop_raw"),
    ("item/rabbit", "items/rabbit_raw"),
    ("item/cod", "items/fish_raw"),
    ("item/cooked_cod", "items/fish_cooked"),
    ("item/salmon", "items/fish_salmon_raw"),
    ("item/cooked_salmon", "items/fish_salmon_cooked"),
    ("item/pufferfish", "items/fish_pufferfish_raw"),
    ("item/tropical_fish", "items/fish_clownfish_raw"),
    ("item/baked_potato", "items/potato_baked"),
    ("item/poisonous_potato", "items/potato_poisonous"),
    ("item/melon_slice", "items/melon"),
    ("item/glistering_melon_slice", "items/melon_speckled"),
    ("item/fermented_spider_eye", "items/spider_eye_fermented"),
    ("item/popped_chorus_fruit", "items/chorus_fruit_popped"),
    ("item/bucket", "items/bucket_empty"),
    ("item/tropical_fish_bucket", "items/bucket_tropical"),
    (
        "item/tropical_fish_spawn_egg",
        "items/spawn_eggs/spawn_egg_tropicalfish",
    ),
    ("item/minecart", "items/minecart_normal"),
    ("item/map", "items/map_empty"),
    ("item/filled_map", "items/map_filled"),
    ("item/abandoned_camp_map", "items/map_abandoned_campsite"),
    ("item/jungle_pyramid_map", "items/map_jungle_temple"),
    ("item/ocean_monument_map", "items/map_monument"),
    ("item/woodland_mansion_map", "items/map_mansion"),
    ("item/book", "items/book_normal"),
    ("item/enchanted_book", "items/book_enchanted"),
    ("item/writable_book", "items/book_writable"),
    ("item/written_book", "items/book_written"),
    ("item/potion", "items/potion_bottle_drinkable"),
    ("item/splash_potion", "items/potion_bottle_splash"),
    ("item/lingering_potion", "items/potion_bottle_lingering"),
    ("item/glass_bottle", "items/potion_bottle_empty"),
    ("item/dragon_breath", "items/dragons_breath"),
    ("item/bow", "items/bow_standby"),
    ("item/fishing_rod", "items/fishing_rod_uncast"),
    ("item/firework_rocket", "items/fireworks"),
    ("item/firework_star", "items/fireworks_charge"),
    ("item/fire_charge", "items/fireball"),
    ("item/slime_ball", "items/slimeball"),
    ("item/redstone", "items/redstone_dust"),
    ("item/sugar_cane", "items/reeds"),
    ("item/nether_brick", "items/netherbrick"),
    ("item/heart_of_the_sea", "items/heartofthesea_closed"),
    ("item/nautilus_shell", "items/nautilus"),
    ("item/totem_of_undying", "items/totem"),
    ("item/turtle_scute", "items/turtle_shell_piece"),
    ("item/elytra_broken", "items/broken_elytra"),
    ("item/bell", "items/villagebell"),
];

/// Patterns, tried when no exact rename names the texture.
const PATTERNS: &[Rule] = &[
    // ---- blocks
    Rule("block/*_planks", "blocks/planks_*", Wood),
    Rule("block/*_log", "blocks/log_*", Wood),
    Rule("block/*_log_top", "blocks/log_*_top", Wood),
    Rule("block/*_log", "blocks/*_log_side", Plain),
    Rule("block/*_leaves", "blocks/leaves_*", Wood),
    Rule("block/*_sapling", "blocks/sapling_*", Plain),
    Rule("block/*_door_bottom", "blocks/door_*_lower", Plain),
    Rule("block/*_door_top", "blocks/door_*_upper", Plain),
    Rule("block/*_wool", "blocks/wool_colored_*", Color),
    Rule("block/*_concrete", "blocks/concrete_*", Color),
    Rule("block/*_concrete_powder", "blocks/concrete_powder_*", Color),
    Rule(
        "block/*_glazed_terracotta",
        "blocks/glazed_terracotta_*",
        Color,
    ),
    Rule(
        "block/*_terracotta",
        "blocks/hardened_clay_stained_*",
        Color,
    ),
    Rule("block/*_stained_glass", "blocks/glass_*", Color),
    Rule(
        "block/*_stained_glass_pane_top",
        "blocks/glass_pane_top_*",
        Color,
    ),
    Rule("block/*_coral_block", "blocks/coral_*", Coral),
    Rule("block/dead_*_coral_block", "blocks/coral_*_dead", Coral),
    Rule("block/*_coral", "blocks/coral_plant_*", Coral),
    Rule("block/dead_*_coral", "blocks/coral_plant_*_dead", Coral),
    Rule("block/*_coral_fan", "blocks/coral_fan_*", Coral),
    Rule("block/dead_*_coral_fan", "blocks/coral_fan_*_dead", Coral),
    Rule("block/wheat_stage*", "blocks/wheat_stage_*", Plain),
    Rule("block/carrots_stage*", "blocks/carrots_stage_*", Plain),
    Rule("block/potatoes_stage*", "blocks/potatoes_stage_*", Plain),
    Rule("block/beetroots_stage*", "blocks/beetroots_stage_*", Plain),
    Rule("block/cocoa_stage*", "blocks/cocoa_stage_*", Plain),
    Rule(
        "block/nether_wart_stage*",
        "blocks/nether_wart_stage_*",
        Plain,
    ),
    Rule(
        "block/torchflower_crop_stage*",
        "blocks/torchflower_crop_stage_*",
        Plain,
    ),
    Rule(
        "block/destroy_stage_*",
        "environment/destroy_stage_*",
        Plain,
    ),
    // ---- items
    Rule("item/*_dye", "items/dye_powder_*", Color),
    Rule("item/*_bundle", "items/bundle_*", Plain),
    Rule("item/*_bundle_open_back", "items/bundle_*_open_back", Plain),
    Rule(
        "item/*_bundle_open_front",
        "items/bundle_*_open_front",
        Plain,
    ),
    Rule("item/*_harness", "items/harness/harness_*", Plain),
    Rule("item/*_spawn_egg", "items/spawn_eggs/spawn_egg_*", Plain),
    Rule("item/*_boat", "items/boat_*", Plain),
    Rule("item/*_door", "items/door_*", Plain),
    Rule("item/*_sign", "items/sign_*", Plain),
    Rule("item/golden_*", "items/gold_*", Plain),
    Rule("item/wooden_*", "items/wood_*", Plain),
    Rule("item/cooked_*", "items/*_cooked", Plain),
    Rule("item/*_seeds", "items/seeds_*", Plain),
    Rule("item/*_bucket", "items/bucket_*", Plain),
    Rule("item/*_minecart", "items/minecart_*", Plain),
    Rule("item/*_map", "items/map_*", Plain),
    Rule("item/music_disc_*", "items/record_*", Plain),
    Rule("item/light_0*", "items/light_block_*", Plain),
    Rule("item/light_1*", "items/light_block_1*", Plain),
];

fn translate(cap: Cap, s: &str) -> Option<String> {
    Some(match cap {
        Plain => s.to_string(),
        Color => {
            if s == "light_gray" {
                "silver".into()
            } else {
                s.to_string()
            }
        }
        Wood => {
            if s == "dark_oak" {
                "big_oak".into()
            } else {
                s.to_string()
            }
        }
        Coral => match s {
            "tube" => "blue",
            "brain" => "pink",
            "bubble" => "purple",
            "fire" => "red",
            "horn" => "yellow",
            _ => return None,
        }
        .into(),
    })
}

/// `name` matched against `pattern` (one `*`): the captured part.
fn capture<'a>(pattern: &str, name: &'a str) -> Option<&'a str> {
    let (pre, post) = pattern.split_once('*')?;
    let mid = name.strip_prefix(pre)?.strip_suffix(post)?;
    (!mid.is_empty() && !mid.contains('/')).then_some(mid)
}

/// A rename the table proposes AND the game confirms, as a full stem
/// (`textures/blocks/planks_oak`). `java` is relative to `textures/`, without
/// extension, lowercased.
pub fn rename(java: &str, catalog: &VanillaCatalog) -> Option<String> {
    let confirmed = |rel: &str| {
        let stem = format!("textures/{rel}");
        let shown = match rel.split('/').next() {
            Some("blocks") => !catalog.terrain_tiles(&stem).is_empty(),
            Some("items") => catalog.on_item_atlas(&stem),
            _ => true,
        };
        (catalog.has_asset_stem(&stem) && shown).then_some(stem)
    };
    if let Some((_, b)) = EXACT.iter().find(|(j, _)| *j == java) {
        return confirmed(b);
    }
    let mut found: Vec<String> = PATTERNS
        .iter()
        .filter_map(|Rule(j, b, cap)| {
            let part = translate(*cap, capture(j, java)?)?;
            confirmed(&b.replacen('*', &part, 1))
        })
        .collect();
    found.sort();
    found.dedup();
    match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_one_part() {
        assert_eq!(
            capture("block/*_planks", "block/dark_oak_planks"),
            Some("dark_oak")
        );
        assert_eq!(capture("block/*_planks", "block/planks"), None);
        assert_eq!(
            capture("block/wheat_stage*", "block/wheat_stage7"),
            Some("7")
        );
        assert_eq!(capture("block/*_wool", "block/sub/red_wool"), None);
        assert_eq!(translate(Color, "light_gray").as_deref(), Some("silver"));
        assert_eq!(translate(Coral, "dead"), None);
    }

    #[test]
    fn nothing_is_proposed_without_the_game() {
        let empty = VanillaCatalog::empty();
        assert_eq!(rename("block/oak_planks", &empty), None);
        assert_eq!(rename("block/andesite", &empty), None);
    }

    #[test]
    fn exact_renames_are_unique_and_lowercase() {
        let mut seen = std::collections::HashSet::new();
        for (j, b) in EXACT {
            assert!(seen.insert(*j), "{j} twice");
            assert_eq!(*j, j.to_ascii_lowercase());
            assert_eq!(*b, b.to_ascii_lowercase());
        }
    }
}
