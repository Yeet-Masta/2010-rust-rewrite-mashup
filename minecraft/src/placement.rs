//! Placing the held item as a block, with vanilla's placement states for the
//! families players place most: torches on walls, two-block doors, beds and
//! tall flowers, ladders, trapdoors, and blocks that face the player. Stairs,
//! slabs, logs, rails, levers, furnaces and chests go through MinecraftOSS's
//! own placement rules. Items that are not blocks place nothing.
use minecraft_terrain::scene::{Block, BlockPos, Scene};
use minecraftoss_core::block::flags;
use minecraftoss_player::{Face, Hit, Player};

use crate::world::World;

/// Horizontal directions in `Direction.from2DDataValue` order.
const HORIZONTAL: [(&str, (i32, i32)); 4] = [
    ("south", (0, 1)),
    ("west", (-1, 0)),
    ("north", (0, -1)),
    ("east", (1, 0)),
];

/// The block an item places, for items whose block has another name.
fn block_for_item(item: &str) -> &str {
    match item {
        "minecraft:wheat_seeds" => "minecraft:wheat",
        "minecraft:carrot" => "minecraft:carrots",
        "minecraft:potato" => "minecraft:potatoes",
        "minecraft:beetroot_seeds" => "minecraft:beetroots",
        "minecraft:melon_seeds" => "minecraft:melon_stem",
        "minecraft:pumpkin_seeds" => "minecraft:pumpkin_stem",
        "minecraft:sweet_berries" => "minecraft:sweet_berry_bush",
        "minecraft:glow_berries" => "minecraft:cave_vines",
        "minecraft:cocoa_beans" => "minecraft:cocoa",
        "minecraft:string" => "minecraft:tripwire",
        "minecraft:redstone" => "minecraft:redstone_wire",
        "minecraft:water_bucket" => "minecraft:water",
        "minecraft:lava_bucket" => "minecraft:lava",
        other => other,
    }
}

/// The player's facing, as `getHorizontalDirection`.
fn facing(player: &Player) -> (&'static str, (i32, i32)) {
    let index = ((player.yaw / 90.0 + 0.5).floor() as i32).rem_euclid(4) as usize;
    HORIZONTAL[index]
}

fn opposite(direction: &str) -> &'static str {
    match direction {
        "north" => "south",
        "south" => "north",
        "east" => "west",
        "west" => "east",
        "up" => "down",
        _ => "up",
    }
}

fn face_name(face: Face) -> &'static str {
    match face {
        Face::Down => "down",
        Face::Up => "up",
        Face::North => "north",
        Face::South => "south",
        Face::West => "west",
        Face::East => "east",
    }
}

/// The player's nearest looking direction in three dimensions.
fn looking(player: &Player) -> &'static str {
    let look = player.look();
    let (x, y, z) = (look.x.abs(), look.y.abs(), look.z.abs());
    if y >= x && y >= z {
        if look.y > 0.0 { "up" } else { "down" }
    } else if x >= z {
        if look.x > 0.0 { "east" } else { "west" }
    } else if look.z > 0.0 {
        "south"
    } else {
        "north"
    }
}

/// What placing the held item sets: each position and its block.
pub struct Placed {
    pub blocks: Vec<(BlockPos, Block)>,
}

pub fn place(world: &mut World, player: &Player, item: &str, hit: &Hit) -> Option<Placed> {
    let id = block_for_item(item).to_owned();
    let registries = world.registries.clone();
    let blocks = &registries.blocks;
    let block_id = blocks.block_by_name(&id)?;
    let replaceable = |pos: BlockPos| {
        world
            .scene
            .state_at(pos)
            .is_none_or(|state| blocks.is(state, flags::REPLACEABLE))
    };
    // A clicked grass tuft or snow layer is replaced; otherwise the block
    // goes against the clicked face.
    let pos = if replaceable(hit.pos) && !id.ends_with("_slab") {
        hit.pos
    } else {
        let (dx, dy, dz) = hit.face.offset();
        (hit.pos.0 + dx, hit.pos.1 + dy, hit.pos.2 + dz)
    };
    let range = world.stream.states.vertical_range();
    if !range.contains(&pos.1) {
        return None;
    }
    let has = |name: &str| {
        blocks
            .block(block_id)
            .properties()
            .iter()
            .any(|p| &*p.name == name)
    };
    let path = id.trim_start_matches("minecraft:");
    let (facing_name, (fx, fz)) = facing(player);
    let full = |block: Block| {
        world
            .stream
            .states
            .state_of(&block)
            .and_then(|state| world.stream.states.block(state).cloned())
    };
    let mut placed = Vec::new();
    if path.ends_with("torch") && !path.contains("wall") && path != "torchflower" {
        let face = face_name(hit.face);
        let block = match hit.face {
            Face::Up => Block::new(&id),
            Face::Down => return None,
            _ => Block::new(&id.replace("torch", "wall_torch")).with("facing", face),
        };
        placed.push((pos, full(block)?));
    } else if path == "ladder" {
        if matches!(hit.face, Face::Up | Face::Down) {
            return None;
        }
        placed.push((
            pos,
            full(Block::new(&id).with("facing", face_name(hit.face)))?,
        ));
    } else if path.ends_with("_door") {
        let above = (pos.0, pos.1 + 1, pos.2);
        if !replaceable(pos) || !replaceable(above) || !range.contains(&above.1) {
            return None;
        }
        let door = |half: &str| {
            Block::new(&id)
                .with("facing", facing_name)
                .with("half", half)
                .with("hinge", "left")
                .with("open", "false")
        };
        placed.push((pos, full(door("lower"))?));
        placed.push((above, full(door("upper"))?));
    } else if path.ends_with("_bed") {
        let head = (pos.0 + fx, pos.1, pos.2 + fz);
        if !replaceable(pos) || !replaceable(head) {
            return None;
        }
        let bed = |part: &str| {
            Block::new(&id)
                .with("facing", facing_name)
                .with("part", part)
        };
        placed.push((pos, full(bed("foot"))?));
        placed.push((head, full(bed("head"))?));
    } else if has("half")
        && blocks
            .block(block_id)
            .properties()
            .iter()
            .any(|p| &*p.name == "half" && p.values.iter().any(|v| &**v == "upper"))
    {
        // Tall flowers and grasses: two halves.
        let above = (pos.0, pos.1 + 1, pos.2);
        if !replaceable(pos) || !replaceable(above) || !range.contains(&above.1) {
            return None;
        }
        placed.push((pos, full(Block::new(&id).with("half", "lower"))?));
        placed.push((above, full(Block::new(&id).with("half", "upper"))?));
    } else if path.ends_with("_trapdoor") {
        let (facing, half) = match hit.face {
            Face::Up => (opposite(facing_name), "bottom"),
            Face::Down => (opposite(facing_name), "top"),
            face => (
                face_name(face),
                if hit.point.y - f64::from(hit.pos.1) > 0.5 {
                    "top"
                } else {
                    "bottom"
                },
            ),
        };
        placed.push((
            pos,
            full(Block::new(&id).with("facing", facing).with("half", half))?,
        ));
    } else if path.ends_with("_fence_gate") {
        placed.push((pos, full(Block::new(&id).with("facing", facing_name))?));
    } else if matches!(
        path,
        "piston" | "sticky_piston" | "dispenser" | "dropper" | "observer"
    ) {
        let look = looking(player);
        let facing = if path == "observer" {
            look
        } else {
            opposite(look)
        };
        placed.push((pos, full(Block::new(&id).with("facing", facing))?));
    } else if path.ends_with("_sign") && !path.contains("hanging") {
        let block = match hit.face {
            Face::Up => Block::new(&id).with(
                "rotation",
                &(((player.yaw + 180.0) * 16.0 / 360.0 + 0.5).floor() as i32)
                    .rem_euclid(16)
                    .to_string(),
            ),
            Face::Down => return None,
            face => Block::new(&id.replace("_sign", "_wall_sign")).with("facing", face_name(face)),
        };
        placed.push((pos, full(block)?));
    } else if path == "crafter" {
        // `CrafterBlock.getStateForPlacement`: the front towards the
        // player, the top away from them when the front is up or down. The
        // server sets `triggered`.
        let front = opposite(looking(player));
        let top = match front {
            "down" => opposite(facing_name),
            "up" => facing_name,
            _ => "up",
        };
        let orientation = format!("{front}_{top}");
        placed.push((
            pos,
            full(Block::new(&id).with("orientation", &orientation))?,
        ));
    } else if DELEGATED.iter().any(|suffix| path.ends_with(suffix)) || path == "chest" {
        return place_with_rules(world, player, &id, hit.pos, pos);
    } else {
        let mut block = Block::new(&id);
        let property = |name: &str| {
            blocks
                .block(block_id)
                .properties()
                .iter()
                .find(|p| &*p.name == name)
                .map(|p| p.values.clone())
        };
        if let Some(values) = property("facing") {
            let facing =
                if path == "end_rod" || path == "lightning_rod" || path.contains("amethyst") {
                    face_name(hit.face)
                } else if values.iter().any(|v| &**v == "up") {
                    opposite(looking(player))
                } else {
                    opposite(facing_name)
                };
            block = block.with("facing", facing);
        }
        if property("axis").is_some() {
            let axis = match hit.face {
                Face::Up | Face::Down => "y",
                Face::East | Face::West => "x",
                Face::North | Face::South => "z",
            };
            block = block.with("axis", axis);
        }
        placed.push((pos, full(block)?));
    }
    if !placed.iter().all(|(at, _)| replaceable(*at)) {
        return None;
    }
    // Plants need ground that grows them.
    let (first, _) = placed[0];
    let below = (first.0, first.1 - 1, first.2);
    if let Some(tag) = support_tag(&registries, path)
        && !world.scene.state_at(below).is_some_and(|state| {
            let block = usize::from(blocks.block_of(state).0);
            registries
                .block_tags
                .id(tag)
                .is_some_and(|tag| registries.block_tags.contains(tag, block))
        })
    {
        return None;
    }
    Some(Placed { blocks: placed })
}

/// Blocks MinecraftOSS's own placement shapes: stairs and slabs by where
/// they are clicked, rails and chests by their neighbours, leaves kept.
const DELEGATED: [&str; 7] = [
    "_stairs", "_slab", "rail", "_button", "lever", "_leaves", "hopper",
];

/// The block tag a plant's ground must be in.
fn support_tag(
    registries: &minecraftoss_core::registries::Registries,
    path: &str,
) -> Option<&'static str> {
    let tagged = |tag: &str| {
        registries
            .blocks
            .block_by_name(&format!("minecraft:{path}"))
            .is_some_and(|id| {
                registries
                    .block_tags
                    .id(tag)
                    .is_some_and(|tag| registries.block_tags.contains(tag, usize::from(id.0)))
            })
    };
    Some(match path {
        "melon_stem" => "minecraft:supports_melon_stem",
        "pumpkin_stem" => "minecraft:supports_pumpkin_stem",
        "sugar_cane" => "minecraft:supports_sugar_cane",
        "cactus" => "minecraft:supports_cactus",
        "bamboo" => "minecraft:supports_bamboo",
        "nether_wart" => "minecraft:supports_nether_wart",
        "wither_rose" => "minecraft:supports_wither_rose",
        "dead_bush" | "short_dry_grass" | "tall_dry_grass" => "minecraft:supports_dry_vegetation",
        _ if tagged("minecraft:crops") => "minecraft:supports_crops",
        "short_grass" | "fern" | "tall_grass" | "large_fern" | "sweet_berry_bush" | "bush"
        | "firefly_bush" => "minecraft:supports_vegetation",
        _ if tagged("minecraft:saplings")
            || tagged("minecraft:flowers")
            || tagged("minecraft:small_flowers") =>
        {
            "minecraft:supports_vegetation"
        }
        _ => return None,
    })
}

/// MinecraftOSS's placement rules, for the blocks it shapes by their
/// neighbours or by where they are clicked.
fn place_with_rules(
    world: &mut World,
    player: &Player,
    id: &str,
    clicked: BlockPos,
    pos: BlockPos,
) -> Option<Placed> {
    if world
        .scene
        .state_at(pos)
        .is_some_and(|state| !world.registries.blocks.is(state, flags::REPLACEABLE))
        && !id.ends_with("_slab")
    {
        return None;
    }
    // The placement may land on the clicked block (a slab merging) or beside
    // it; both are put back after, for the caller to set what was placed.
    let candidates: Vec<(BlockPos, Option<Block>)> = [clicked, pos]
        .into_iter()
        .map(|at| (at, Scene::block(&world.scene, at).cloned()))
        .collect();
    let placed_at = player.place_target_with(&mut world.scene, minecraftoss_player::Block::new(id));
    let block = placed_at.and_then(|at| Scene::block(&world.scene, at).cloned());
    for (at, before) in candidates {
        world.scene.set(at, before);
    }
    let (placed_at, block) = (placed_at?, block?);
    let block = world
        .stream
        .states
        .state_of(&block)
        .and_then(|state| world.stream.states.block(state).cloned())?;
    Some(Placed {
        blocks: vec![(placed_at, block)],
    })
}

/// Whether a block at `pos` would meet the player's box.
pub fn blocks_player(world: &World, player: &Player, pos: BlockPos, block: &Block) -> bool {
    let Some(state) = world.stream.states.state_of(block) else {
        return false;
    };
    let height = if player.crouching { 1.5 } else { 1.8 };
    let (min, max) = (
        player.pos - glam::DVec3::new(0.3, 0.0, 0.3),
        player.pos + glam::DVec3::new(0.3, height, 0.3),
    );
    world
        .registries
        .blocks
        .collision_boxes(state)
        .iter()
        .any(|b| {
            let (bx, by, bz) = (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
            min.x < bx + b[3]
                && max.x > bx + b[0]
                && min.y < by + b[4]
                && max.y > by + b[1]
                && min.z < bz + b[5]
                && max.z > bz + b[2]
        })
}

/// What a tool does to the block it is used on: a hoe tills, a shovel
/// flattens a path, an axe strips bark. The changed block and its sound.
pub fn tool_use(
    world: &World,
    tool: &str,
    pos: BlockPos,
    face: Face,
) -> Option<(Block, &'static str)> {
    let block = world.block(pos)?;
    let path = block.id.path.as_str();
    let open_above = || world.block((pos.0, pos.1 + 1, pos.2)).is_none();
    let full = |id: &str, from: &Block| {
        let mut block = Block::new(id);
        if let Some(axis) = from.properties.get("axis") {
            block = block.with("axis", axis);
        }
        world
            .stream
            .states
            .state_of(&block)
            .and_then(|state| world.stream.states.block(state).cloned())
    };
    if tool.ends_with("_hoe") && face != Face::Down && open_above() {
        let tilled = match path {
            "grass_block" | "dirt" | "dirt_path" => "minecraft:farmland",
            "coarse_dirt" | "rooted_dirt" => "minecraft:dirt",
            _ => return None,
        };
        return Some((full(tilled, block)?, "minecraft:item.hoe.till"));
    }
    if tool.ends_with("_shovel") && face != Face::Down && open_above() {
        let registries = &world.registries;
        let id = registries.blocks.block_by_name(&block.id.key())?;
        let tag = registries.block_tags.id("minecraft:turns_into_dirt_path")?;
        if !registries.block_tags.contains(tag, usize::from(id.0)) {
            return None;
        }
        return Some((
            full("minecraft:dirt_path", block)?,
            "minecraft:item.shovel.flatten",
        ));
    }
    if tool.ends_with("_axe") && !path.starts_with("stripped_") {
        let stripped = if path.ends_with("_log")
            || path.ends_with("_wood")
            || path.ends_with("_stem")
            || path.ends_with("_hyphae")
            || path == "bamboo_block"
        {
            format!("minecraft:stripped_{path}")
        } else {
            return None;
        };
        world.registries.blocks.block_by_name(&stripped)?;
        return Some((full(&stripped, block)?, "minecraft:item.axe.strip"));
    }
    None
}
