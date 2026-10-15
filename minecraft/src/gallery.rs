//! `--mob-gallery`: every mob the game has, on a flat stage of grass built
//! in open sky above the spawn, in rows from the smallest at the front to
//! the giants at the back, with a water channel along the front for the
//! sea creatures. They have no AI (`NoAI`), so they stand still facing the
//! player, who hovers in front of the stage looking down at it; the
//! undead that the sun would burn wear a leather helmet.
use crate::game::Game;
use minecraft_terrain::scene::{Block, BlockPos};
use minecraftoss_core::nbt::Tag;
use std::collections::BTreeMap;

/// The sea creatures, in the channel.
const WATER: &[&str] = &[
    "cod", "salmon", "pufferfish", "tropical_fish", "tadpole", "squid", "glow_squid", "axolotl", "turtle", "dolphin", "nautilus", "zombie_nautilus",
    "drowned", "guardian", "elder_guardian",
];

/// The land rows, front to back, with each row's spacing.
const ROWS: &[(f64, &[&str])] = &[
    (3.5, &["silverfish", "endermite", "rabbit", "frog", "chicken", "bat", "parrot", "bee", "allay", "vex", "cat", "ocelot", "fox", "cave_spider", "armadillo"]),
    (3.5, &["pig", "sheep", "cow", "mooshroom", "wolf", "goat", "panda", "polar_bear", "llama", "trader_llama", "spider", "slime", "magma_cube", "sulfur_cube", "shulker"]),
    (3.5, &["zombie", "husk", "zombie_villager", "skeleton", "stray", "bogged", "parched", "wither_skeleton", "creeper", "witch", "enderman", "blaze", "breeze", "creaking", "phantom"]),
    (3.5, &["villager", "wandering_trader", "pillager", "vindicator", "evoker", "illusioner", "piglin", "piglin_brute", "zombified_piglin", "snow_golem", "copper_golem", "iron_golem", "strider"]),
    (4.5, &["horse", "donkey", "mule", "skeleton_horse", "zombie_horse", "camel", "camel_husk", "hoglin", "zoglin", "ravager", "sniffer", "warden"]),
    (12.0, &["ghast", "happy_ghast", "wither", "giant"]),
    (0.0, &["ender_dragon"]),
];

/// The undead that burn in daylight without a helmet (`burnUndead`).
const SUN_BURNT: &[&str] = &["zombie", "zombie_villager", "drowned", "skeleton", "stray", "bogged", "parched", "phantom"];

/// How far back each row stands from the stage's front edge.
const ROW_DEPTHS: [i32; 7] = [8, 14, 20, 26, 33, 46, 66];

const HALF_WIDTH: i32 = 32;
const DEPTH: i32 = 80;

/// The tag a gallery mob is summoned with: no AI, facing the front, a
/// helmet against the sun, a slime's middle size.
fn tag_for(kind: &str) -> Tag {
    let mut map = BTreeMap::new();
    map.insert("NoAI".to_owned(), Tag::Byte(1));
    map.insert("PersistenceRequired".to_owned(), Tag::Byte(1));
    map.insert("Rotation".to_owned(), Tag::List(vec![Tag::Float(180.0), Tag::Float(0.0)]));
    if SUN_BURNT.contains(&kind) {
        let helmet = Tag::Compound([("id".to_owned(), Tag::String("minecraft:leather_helmet".to_owned())), ("count".to_owned(), Tag::Int(1))].into_iter().collect());
        map.insert("equipment".to_owned(), Tag::Compound([("head".to_owned(), helmet)].into_iter().collect()));
    }
    if matches!(kind, "slime" | "magma_cube") {
        map.insert("Size".to_owned(), Tag::Int(2));
    }
    Tag::Compound(map)
}

/// Builds the stage and fills it.
pub fn build(game: &mut Game) {
    let feet = game.player.pos;
    let (x0, z0) = (feet.x.floor() as i32, feet.z.floor() as i32);
    // High above the spawn and the mountains about it.
    let y = (feet.y.floor() as i32 + 30).clamp(190, 280);
    // The stage: grass over dirt, the channel's floor of stone, its ends
    // of glass, and its water two deep.
    let mut changes: Vec<(BlockPos, Option<Block>)> = Vec::new();
    let channel = |z: i32| (z0 + 2..=z0 + 4).contains(&z);
    for x in x0 - HALF_WIDTH..=x0 + HALF_WIDTH {
        for z in z0..z0 + DEPTH {
            if channel(z) {
                changes.push(((x, y - 2, z), Some(Block::new("minecraft:stone"))));
                let water = if x == x0 - HALF_WIDTH || x == x0 + HALF_WIDTH { "minecraft:glass" } else { "minecraft:water" };
                changes.push(((x, y - 1, z), Some(Block::new(water))));
                changes.push(((x, y, z), Some(Block::new(water))));
            } else {
                changes.push(((x, y - 1, z), Some(Block::new("minecraft:dirt"))));
                changes.push(((x, y, z), Some(Block::new("minecraft:grass_block"))));
            }
        }
    }
    game.world.set_blocks(&changes);
    for (pos, _) in &changes {
        game.entities.edited(&game.world.scene, *pos, true);
    }
    // The sea creatures in the channel, the rest in rows on the grass.
    let at = |i: usize, count: usize, spacing: f64, depth: f64, height: f64| {
        let offset = (i as f64 - (count as f64 - 1.0) / 2.0) * spacing;
        [f64::from(x0) + 0.5 + offset, height, f64::from(z0) + depth]
    };
    for (i, kind) in WATER.iter().enumerate() {
        game.entities.summon_tagged(kind, at(i, WATER.len(), 3.5, 3.5, f64::from(y - 1)), tag_for(kind));
    }
    for ((spacing, kinds), depth) in ROWS.iter().zip(ROW_DEPTHS) {
        for (i, kind) in kinds.iter().enumerate() {
            game.entities.summon_tagged(kind, at(i, kinds.len(), *spacing, f64::from(depth) + 0.5, f64::from(y + 1)), tag_for(kind));
        }
    }
    // The player hovers before the stage, looking down along it.
    game.player.pos = glam::DVec3::new(f64::from(x0) + 0.5, f64::from(y + 9), f64::from(z0) - 7.0);
    game.player.velocity = glam::DVec3::ZERO;
    game.player.flying = true;
    game.player.yaw = 0.0;
    game.player.pitch = 25.0;
}
