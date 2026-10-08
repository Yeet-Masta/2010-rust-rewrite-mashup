//! Survival block breaking, as vanilla's `MultiPlayerGameMode` and
//! `Player.getDestroySpeed` work it: progress each tick is the tool's speed
//! over the block's hardness, divided by 30 with the right tool and 100
//! without; off the ground or under water it is slower. Which tool suits a
//! block, and which tier it needs for drops, come from the data pack's
//! `mineable/*` and `incorrect_for_*_tool` tags.
use std::collections::HashMap;

use minecraft_terrain::scene::{Block, BlockPos, HandcraftedScene, Scene};
use minecraftoss_core::registries::Registries;
use minecraftoss_core::tags::TagId;
use minecraftoss_player::inventory::ItemStack;
use minecraftoss_player::loot::LootBook;
use minecraftoss_player::rng::XoroshiroRandom;

/// Ticks between one broken block and the next start (`destroyDelay`).
const DESTROY_DELAY: u32 = 5;

/// A tool tier: its mining speed and the tag of blocks it cannot harvest.
struct Tier {
    speed: f32,
    incorrect: Option<TagId>,
}

pub struct ToolRules {
    mineable: HashMap<&'static str, Option<TagId>>,
    tiers: HashMap<&'static str, Tier>,
    needs: [Option<TagId>; 3],
    sword_efficient: Option<TagId>,
    sword_instant: Option<TagId>,
    leaves: Option<TagId>,
    wool: Option<TagId>,
}

/// How a held item works on a block.
pub struct Dig {
    pub hardness: f32,
    pub speed: f32,
    /// The item harvests the block: it drops its loot.
    pub harvests: bool,
}

impl ToolRules {
    pub fn new(registries: &Registries) -> Self {
        let tag = |name: &str| registries.block_tags.id(name);
        let mineable = ["pickaxe", "axe", "shovel", "hoe"]
            .into_iter()
            .map(|tool| (tool, tag(&format!("minecraft:mineable/{tool}"))))
            .collect();
        let tiers = [
            ("wooden", 2.0),
            ("stone", 4.0),
            ("copper", 5.0),
            ("iron", 6.0),
            ("diamond", 8.0),
            ("golden", 12.0),
            ("netherite", 9.0),
        ]
        .into_iter()
        .map(|(tier, speed)| {
            let tag_name = if tier == "golden" { "gold" } else { tier };
            (
                tier,
                Tier {
                    speed,
                    incorrect: tag(&format!("minecraft:incorrect_for_{tag_name}_tool")),
                },
            )
        })
        .collect();
        Self {
            mineable,
            tiers,
            needs: [
                tag("minecraft:needs_stone_tool"),
                tag("minecraft:needs_iron_tool"),
                tag("minecraft:needs_diamond_tool"),
            ],
            sword_efficient: tag("minecraft:sword_efficient"),
            sword_instant: tag("minecraft:sword_instantly_mines"),
            leaves: tag("minecraft:leaves"),
            wool: tag("minecraft:wool"),
        }
    }

    /// The held item against a block, or `None` for one that cannot be
    /// broken (air, fluids, bedrock).
    pub fn dig(
        &self,
        registries: &Registries,
        block: &Block,
        held: Option<&ItemStack>,
    ) -> Option<Dig> {
        let path = block.id.path.as_str();
        if matches!(
            path,
            "water" | "lava" | "air" | "cave_air" | "void_air" | "bubble_column"
        ) {
            return None;
        }
        let blocks = &registries.blocks;
        let id = blocks.block_by_name(&block.id.key())?;
        let hardness = blocks.state(blocks.block(id).default_state()).destroy_speed;
        if hardness < 0.0 {
            return None;
        }
        let element = usize::from(id.0);
        let has = |tag: Option<TagId>| {
            tag.is_some_and(|tag| registries.block_tags.contains(tag, element))
        };
        let pickaxe = self.mineable.get("pickaxe").copied().flatten();
        let requires_tool = self.needs.iter().any(|&tag| has(tag))
            || matches!(path, "cobweb" | "snow" | "snow_block")
            || (has(pickaxe) && !pickaxe_without_tool(path));
        let item = held
            .map(|stack| stack.id.trim_start_matches("minecraft:"))
            .unwrap_or("");
        let (mut speed, mut correct) = (1.0f32, false);
        if let Some((tier, kind)) = item.rsplit_once('_')
            && let (Some(tier), Some(&mineable)) = (self.tiers.get(tier), self.mineable.get(kind))
        {
            // `Tool.getMiningSpeed` and `isCorrectForDrops`: the tier's speed
            // on any block of its kind; drops only where the tier is enough.
            if has(mineable) {
                speed = tier.speed;
                correct = !has(tier.incorrect);
            }
        } else if item.ends_with("_sword") {
            if path == "cobweb" {
                (speed, correct) = (15.0, true);
            } else if has(self.sword_instant) {
                speed = f32::MAX;
            } else if has(self.sword_efficient) {
                speed = 1.5;
            }
        } else if item == "shears" {
            if path == "cobweb" || has(self.leaves) {
                (speed, correct) = (15.0, true);
            } else if has(self.wool) {
                (speed, correct) = (5.0, true);
            } else if matches!(path, "vine" | "glow_lichen") {
                speed = 2.0;
            }
            correct |= matches!(path, "redstone_wire" | "tripwire");
        }
        Some(Dig {
            hardness,
            speed,
            harvests: correct || !requires_tool,
        })
    }
}

/// Pickaxe blocks that drop without one (`requiresCorrectToolForDrops`
/// left off).
fn pickaxe_without_tool(path: &str) -> bool {
    path.ends_with("_button")
        || path.ends_with("rail")
        || matches!(
            path,
            "ice"
                | "packed_ice"
                | "blue_ice"
                | "frosted_ice"
                | "piston"
                | "sticky_piston"
                | "piston_head"
                | "moving_piston"
                | "conduit"
        )
}

/// A block the player broke.
pub struct Broken {
    pub pos: BlockPos,
    pub block: Block,
    pub hardness: f32,
    pub drops: Vec<ItemStack>,
}

/// What one tick of holding the attack button did.
#[derive(Default)]
pub struct Swing {
    pub broken: Option<Broken>,
    /// A block being mined sounded its hit.
    pub hit: Option<(BlockPos, Block)>,
    /// The block took a tick of mining (`continueDestroyBlock` went on):
    /// it cracks a fragment off.
    pub cracked: Option<Block>,
}

pub struct Mining {
    rules: ToolRules,
    loot: Option<LootBook>,
    loot_seed: u64,
    loot_sequences: HashMap<String, XoroshiroRandom>,
    target: Option<(BlockPos, Block)>,
    held: Option<ItemStack>,
    ticks: u32,
    progress: f32,
    delay: u32,
}

impl Mining {
    pub fn new(registries: &Registries, loot: Option<LootBook>, seed: i64) -> Self {
        Self {
            rules: ToolRules::new(registries),
            loot,
            loot_seed: seed as u64,
            loot_sequences: HashMap::new(),
            target: None,
            held: None,
            ticks: 0,
            progress: 0.0,
            delay: 0,
        }
    }

    pub fn reset(&mut self) {
        self.target = None;
        self.ticks = 0;
        self.progress = 0.0;
    }

    /// One tick of the attack button held on `target` (nothing when the
    /// player looks at no block), `pressed` this tick. Creative players
    /// break blocks outright.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        registries: &Registries,
        scene: &HandcraftedScene,
        target: Option<BlockPos>,
        held: Option<&ItemStack>,
        on_ground: bool,
        eyes_in_water: bool,
        creative: bool,
        pressed: bool,
    ) -> Swing {
        // `startDestroyBlock`, on a press, doesn't wait out the delay that
        // holding the button does.
        if pressed {
            self.delay = 0;
        }
        if self.delay > 0 {
            self.delay -= 1;
            return Swing::default();
        }
        let Some(pos) = target else {
            self.reset();
            return Swing::default();
        };
        let Some(block) = Scene::block(scene, pos).cloned() else {
            self.reset();
            return Swing::default();
        };
        if self.target.as_ref() != Some(&(pos, block.clone())) || self.held.as_ref() != held {
            self.target = Some((pos, block.clone()));
            self.held = held.cloned();
            self.ticks = 0;
            self.progress = 0.0;
        }
        let Some(dig) = self.rules.dig(registries, &block, held) else {
            self.reset();
            return Swing::default();
        };
        if creative {
            if held.is_some_and(|stack| {
                stack.id.ends_with("_sword")
                    || stack.id.ends_with("_spear")
                    || stack.id.ends_with("mace")
            }) {
                return Swing::default();
            }
            self.reset();
            self.delay = DESTROY_DELAY;
            return Swing {
                broken: Some(Broken {
                    pos,
                    block,
                    hardness: dig.hardness,
                    drops: Vec::new(),
                }),
                hit: None,
                cracked: None,
            };
        }
        let mut speed = dig.speed;
        if eyes_in_water {
            speed *= 0.2;
        }
        if !on_ground {
            speed /= 5.0;
        }
        let per_tick = if dig.hardness == 0.0 {
            1.0
        } else {
            speed / dig.hardness / if dig.harvests { 30.0 } else { 100.0 }
        };
        self.ticks += 1;
        self.progress += per_tick;
        if self.progress < 1.0 {
            // `MultiPlayerGameMode.continueDestroyBlock`: a hit sound every
            // four ticks.
            let hit = (self.ticks % 4 == 1).then(|| (pos, block.clone()));
            return Swing {
                broken: None,
                hit,
                cracked: Some(block),
            };
        }
        let instant = self.ticks == 1;
        self.reset();
        if !instant {
            self.delay = DESTROY_DELAY;
        }
        let drops = if dig.harvests {
            self.drops(&block, held)
        } else {
            Vec::new()
        };
        Swing {
            broken: Some(Broken {
                pos,
                block,
                hardness: dig.hardness,
                drops,
            }),
            hit: None,
            cracked: None,
        }
    }

    fn drops(&mut self, block: &Block, held: Option<&ItemStack>) -> Vec<ItemStack> {
        let Some(loot) = self.loot.as_ref() else {
            return vec![ItemStack::new(block.id.key(), 1)];
        };
        let block = minecraftoss_player::Block {
            id: block.id.key(),
            properties: block.properties.clone(),
        };
        loot.roll_drops_named(&block, held, self.loot_seed, &mut self.loot_sequences)
            .unwrap_or_else(Vec::new)
    }

    /// The block being mined and its destroy stage, 0 to 9.
    pub fn stage(&self) -> Option<(BlockPos, u32)> {
        let (pos, _) = self.target.as_ref()?;
        (self.progress > 0.0).then(|| (*pos, ((self.progress * 10.0) as u32).min(9)))
    }

    /// A cube a hair larger than the block being mined, textured with its
    /// destroy stage from the strip of ten: position then uv.
    pub fn crack_mesh(&self) -> (Vec<[f32; 5]>, Vec<u32>) {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        let Some(((x, y, z), stage)) = self.stage() else {
            return (vertices, indices);
        };
        let stage = stage as f32;
        let a = [x as f32 - 0.002, y as f32 - 0.002, z as f32 - 0.002];
        let b = [x as f32 + 1.002, y as f32 + 1.002, z as f32 + 1.002];
        let p = [
            [a[0], a[1], a[2]],
            [b[0], a[1], a[2]],
            [b[0], b[1], a[2]],
            [a[0], b[1], a[2]],
            [a[0], a[1], b[2]],
            [b[0], a[1], b[2]],
            [b[0], b[1], b[2]],
            [a[0], b[1], b[2]],
        ];
        for face in [
            [0, 3, 2, 1],
            [5, 6, 7, 4],
            [4, 7, 3, 0],
            [1, 2, 6, 5],
            [3, 7, 6, 2],
            [4, 0, 1, 5],
        ] {
            let first = vertices.len() as u32;
            for (index, uv) in
                face.into_iter()
                    .zip([[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]])
            {
                let q = p[index];
                vertices.push([q[0], q[1], q[2], (stage + uv[0]) / 10.0, uv[1]]);
            }
            indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        }
        (vertices, indices)
    }
}
