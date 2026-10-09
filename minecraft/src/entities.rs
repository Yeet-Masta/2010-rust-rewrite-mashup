//! The world's mobs and item entities, as MinecraftOSS runs them: its
//! integrated server ticks the level at 20 Hz on its own thread with natural
//! spawning and the entity world, every passive and hostile mob's AI and
//! pathfinding. Here it is fed the streamed chunks and the player, and hands
//! back the tracked mobs, which are drawn with the viewer's mob renderers.
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use glam::{DVec3, Vec3};
use minecraft_terrain::client_mobs::{ClientMobs, server_mobs};
use minecraft_terrain::lighting::SkyLight;
use minecraft_terrain::mesh::{Atlas, ChunkMesh, ItemVisuals};
use minecraft_terrain::pack::{PackStack, ResourceId};
use minecraft_terrain::poof_particles::PoofParticles;
use minecraft_terrain::portal_particles::PortalParticles;
use minecraft_terrain::scene::{Block, HandcraftedScene, Scene};
use minecraft_terrain::server::{
    EntitySnapshot, MovingBlockView, PlayerEdit, ServerHandle, ServerItem, ServerSim, TickInput,
};
use minecraft_terrain::terrain::TerrainStream;
use minecraftoss_entities::tempt::PlayerCandidate;
use minecraftoss_entities::world::{EntityWorld, PlayerAttack, PlayerHit};
use minecraftoss_player::inventory::{Inventory, ItemStack};
use minecraftoss_player::items::{ItemEntity, WorldItems};

const TICK_SECONDS: f64 = 1.0 / 20.0;
/// The player's id in the entity world.
const PLAYER: u64 = 0;

/// The mobs drawn this frame: entity models (cut out, back-face culled,
/// translucent) and their shadows; and what goes with the particles (item
/// entities, held items, puffs, flames, potions).
#[derive(Default)]
pub struct MobMeshes {
    pub models: ChunkMesh,
    pub culled: ChunkMesh,
    pub translucent: ChunkMesh,
    pub shadows: ChunkMesh,
    pub items: ChunkMesh,
}

/// The player as the mobs see it this tick.
pub struct PlayerView {
    pub feet: DVec3,
    pub eye_height: f32,
    pub alive: bool,
    pub health: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: DVec3,
    /// Hurts the player took since the last tick: the mob behind each and
    /// its damage type.
    pub hurts: Vec<(Option<u64>, &'static str)>,
}

/// What the server sent back this frame.
#[derive(Default)]
pub struct Events {
    /// Blocks it changed (crops, doors, creeper blasts, endermen).
    pub changes: Vec<((i32, i32, i32), Option<Block>)>,
    /// Block entities it created, changed or removed (`None`), as saved,
    /// for the chunks to keep: they go to the chunk map before another
    /// chunk is sent.
    pub block_entities: Vec<((i32, i32, i32), Option<minecraftoss_core::nbt::Tag>)>,
    pub hits: Vec<PlayerHit>,
    /// Experience points of orbs the player took.
    pub experience: u32,
    /// A use on a mob that the mob took (the arm swings), and one it passed
    /// on (the held item is used instead).
    pub use_taken: bool,
    pub use_passed: bool,
    /// Where bone meal took (level event 1505).
    pub bone_meal_used: Vec<(i32, i32, i32)>,
    /// Entity events with particles (hearts, a villager's moods).
    pub mob_events: Vec<(u64, u8)>,
    /// The player's hits: the mob, and what the hit did.
    pub attacks: Vec<(u64, minecraftoss_entities::world::AttackResult)>,
}

pub struct Entities {
    server: ServerHandle,
    /// The mobs the player tracks, as of the last server tick.
    world: EntityWorld,
    /// The client's copies of them: tracked, interpolated, animated.
    client: ClientMobs,
    poof: PoofParticles,
    portal: PortalParticles,
    items: ItemVisuals,
    /// The blocks pistons are moving, as of the last server tick.
    moving_blocks: Vec<MovingBlockView>,
    /// Biome colours for moving blocks, read from the pack on first use.
    tint: Option<minecraft_terrain::mesh::BiomeTint>,
    clock: f64,
    ticks: u64,
    /// The player's inventory: vanilla slots, stacking and recipes.
    pub inventory: Inventory,
    /// The selected hotbar slot.
    pub selected: usize,
    /// Item entities as the client shows them: the server's, and drops the
    /// client spawned that the server has not taken yet.
    pub world_items: WorldItems,
    server_item_ids: HashSet<u32>,
    /// Client drops handed to the server: the command count that sent each.
    server_handed: HashMap<u32, (u64, ItemEntity)>,
    /// Stacks the server let the player pick up, not yet in the inventory.
    server_picked: Vec<(i32, [f64; 3], String, i32, Option<String>)>,
    server_snapshot: Option<EntitySnapshot>,
    server_handled: u64,
    /// Sounds the mob world made since the last take: event, block point,
    /// volume, pitch.
    pub sounds: Vec<(String, DVec3, f32, f32)>,
    random: minecraftoss_player::rng::LegacyRandom,
}

impl Entities {
    pub fn new(
        stream: &TerrainStream,
        seed: i64,
        jar: Option<&Path>,
        item_catalog: Option<&Path>,
    ) -> Self {
        let mut sim = ServerSim::new(
            stream.world_gen(),
            stream.states.clone(),
            "minecraft:overworld",
        );
        // Mobs save and load with the chunks they stand in.
        sim.set_entity_storage(stream.storage());
        let mut server = ServerHandle::spawn(sim);
        let mut inventory = Inventory::default();
        if let Some(jar) = jar {
            server.load_loot(jar.to_path_buf(), seed as u64);
            match minecraftoss_player::crafting::RecipeBook::from_jar(jar) {
                Ok(mut recipes) => {
                    if let Some(catalog) = item_catalog.and_then(|path| {
                        minecraftoss_player::item_catalog::ItemCatalog::from_path(path).ok()
                    }) {
                        recipes = recipes.with_item_catalog(Arc::new(catalog));
                    }
                    let recipes = Arc::new(recipes);
                    server.set_recipe_book(recipes.clone());
                    inventory.recipes = recipes;
                }
                Err(error) => log!("Recipes unavailable: {error:#}"),
            }
        }
        Self {
            server,
            world: EntityWorld::default(),
            client: ClientMobs::default(),
            poof: PoofParticles::default(),
            portal: PortalParticles::default(),
            items: ItemVisuals::default(),
            moving_blocks: Vec::new(),
            tint: None,
            clock: 0.0,
            ticks: 0,
            inventory,
            selected: 0,
            world_items: {
                // Clear of the server's entity ids, which count up from 1:
                // a drop the server has not taken yet is one with a high id.
                let mut items = WorldItems::default();
                items.number_from(1 << 31);
                items
            },
            server_item_ids: HashSet::new(),
            server_handed: HashMap::new(),
            server_picked: Vec::new(),
            server_snapshot: None,
            server_handled: 0,
            sounds: Vec::new(),
            random: minecraftoss_player::rng::LegacyRandom::new((seed ^ 0x1735) as u64),
        }
    }

    pub fn load_chunk(&mut self, chunk: &Arc<minecraftoss_core::Chunk>) {
        self.server.load_chunk(chunk);
    }

    pub fn unload_chunk(&mut self, pos: minecraftoss_core::ChunkPos) {
        self.server.unload_chunk(pos);
    }

    /// Before a save: the server saves its entities and hands over every
    /// block entity change so far, for the chunks to save with them.
    pub fn flush(&mut self) -> Vec<((i32, i32, i32), Option<minecraftoss_core::nbt::Tag>)> {
        self.server.flush()
    }

    /// The stack in the selected hotbar slot.
    pub fn held(&self) -> Option<&ItemStack> {
        self.inventory.slots[self.selected].as_ref()
    }

    /// A stack of the right size limit for its item.
    pub fn stack(&self, id: &str, count: u8) -> ItemStack {
        let mut stack = ItemStack::new(id, count);
        stack.max = self.inventory.recipes.max_stack(id);
        stack
    }

    /// Block loot, popped out of a broken block (`Block.popResource`).
    pub fn drop_loot(&mut self, pos: (i32, i32, i32), drops: Vec<ItemStack>) {
        for mut drop in drops {
            if drop.components.is_none() {
                drop.max = drop.max.min(self.inventory.recipes.max_stack(&drop.id));
            }
            self.world_items.spawn_block_drop(drop, pos);
        }
    }

    /// Stacks thrown from the eye, as `Player.drop` throws them.
    pub fn throw(&mut self, stacks: Vec<ItemStack>, eye: DVec3, yaw: f32, pitch: f32) {
        for stack in stacks {
            self.world_items.toss(stack, eye, yaw, pitch);
        }
    }

    /// `ItemEntity`s to draw and pick up, as the viewer's `server_items_tick`
    /// does for a server-simulated world: client drops go to the server,
    /// stacks the server offered go into the inventory, and the server's
    /// items are mirrored for drawing.
    /// Items dropped here that the server has not been given, sent to it,
    /// as before the game ends so they save with the world.
    pub fn hand_over_drops(&mut self) {
        let entities = self.world_items.entities.clone();
        self.hand_over(&entities);
    }

    fn hand_over(&mut self, entities: &[ItemEntity]) {
        let to_hand: Vec<ItemEntity> = entities
            .iter()
            .filter(|e| {
                !self.server_item_ids.contains(&e.entity_id)
                    && !self.server_handed.contains_key(&e.entity_id)
            })
            .cloned()
            .collect();
        for entity in &to_hand {
            let components = entity.stack.components.as_ref().map(|c| c.to_string());
            self.server.spawn_item(
                &entity.stack.id,
                i32::from(entity.stack.count),
                components.as_deref(),
                entity.position.to_array(),
                entity.velocity.to_array(),
                i32::from(entity.pickup_delay),
                entity.age as i32,
            );
            self.server_handed
                .insert(entity.entity_id, (self.server.sent(), entity.clone()));
        }
    }

    fn server_items_tick(&mut self, feet: DVec3) {
        let entities = std::mem::take(&mut self.world_items.entities);
        self.hand_over(&entities);
        let previous: HashMap<u32, ItemEntity> =
            entities.into_iter().map(|e| (e.entity_id, e)).collect();
        let target = feet + DVec3::Y * 0.81;
        self.world_items.tick_pickup_effects(target);
        for (id, position, item, count, components) in std::mem::take(&mut self.server_picked) {
            let recipes = self.inventory.recipes.clone();
            let make_stack = |item: &str, count: i32| {
                let mut stack = ItemStack::new(item, count.clamp(0, 255) as u8);
                stack.components = components
                    .as_deref()
                    .and_then(|c| serde_json::from_str(c).ok());
                // Its own size limit, which components can set.
                stack.max =
                    minecraft_terrain::stacks::max_stack(item, stack.components.as_ref(), |id| {
                        i32::from(recipes.max_stack(id))
                    });
                stack
            };
            let taken = match self
                .inventory
                .add_item(make_stack(&item, count), self.selected)
            {
                None => count,
                Some(rest) => {
                    let rest_count = i32::from(rest.count);
                    self.server.spawn_item(
                        &item,
                        rest_count,
                        components.as_deref(),
                        feet.to_array(),
                        [0.0; 3],
                        0,
                        0,
                    );
                    count - rest_count
                }
            };
            if taken <= 0 {
                continue;
            }
            let transfer = make_stack(&item, taken);
            let snapshot = previous
                .get(&(id as u32))
                .cloned()
                .unwrap_or_else(|| ItemEntity {
                    entity_id: id as u32,
                    stack: transfer.clone(),
                    position: DVec3::from_array(position),
                    previous_position: DVec3::from_array(position),
                    velocity: DVec3::ZERO,
                    age: 0,
                    bob_offset: bob_offset(id),
                    pickup_delay: 0,
                    on_ground: true,
                });
            self.world_items.note_pickup(snapshot, target, transfer);
        }
        let Some(snapshot) = self.server_snapshot.take() else {
            self.world_items.entities = previous.into_values().collect();
            self.world_items.entities.sort_by_key(|e| e.entity_id);
            return;
        };
        let recipes = self.inventory.recipes.clone();
        self.world_items.entities = snapshot
            .items
            .into_iter()
            .map(|item: ServerItem| {
                let mut stack = ItemStack::new(&item.item, item.count.clamp(0, 255) as u8);
                stack.components = item
                    .components
                    .as_deref()
                    .and_then(|c| serde_json::from_str(c).ok());
                stack.max = minecraft_terrain::stacks::max_stack(
                    &item.item,
                    stack.components.as_ref(),
                    |id| i32::from(recipes.max_stack(id)),
                );
                ItemEntity {
                    entity_id: item.id as u32,
                    stack,
                    position: DVec3::from_array(item.position),
                    previous_position: DVec3::from_array(item.previous_position),
                    velocity: DVec3::from_array(item.velocity),
                    age: item.age.max(0) as u32,
                    bob_offset: bob_offset(item.id),
                    pickup_delay: item.pickup_delay.clamp(0, i32::from(u16::MAX)) as u16,
                    on_ground: item.on_ground,
                }
            })
            .collect();
        self.server_item_ids = self
            .world_items
            .entities
            .iter()
            .map(|e| e.entity_id)
            .collect();
        let handled = self.server_handled;
        self.server_handed.retain(|_, (sent, _)| *sent > handled);
        for (_, entity) in self.server_handed.values() {
            self.world_items.entities.push(entity.clone());
        }
    }

    /// An item in a first-person hand under `pose` (the arm's, before the
    /// item's display transform), lit by the light at `light_at`.
    #[allow(clippy::too_many_arguments)]
    pub fn held_item_mesh(
        &mut self,
        stack: &ItemStack,
        pose: glam::Mat4,
        left: bool,
        light_at: Vec3,
        packs: &PackStack,
        atlas: &Atlas,
        light: &SkyLight,
    ) -> ChunkMesh {
        let mut mesh = ChunkMesh::default();
        // A special model renderer's item (a shield, a trident) applies its
        // base model's display itself.
        if minecraft_terrain::special_icon::append_special_in_hand(
            &mut mesh,
            packs,
            atlas,
            &stack.id,
            stack.components.as_ref(),
            pose,
            left,
            true,
            &minecraft_terrain::mesh::level_item_shade,
        )
        .unwrap_or(false)
        {
            let at = (
                light_at.x.floor() as i32,
                light_at.y.floor() as i32,
                light_at.z.floor() as i32,
            );
            let (sky, block) = (light.get(at) as f32, light.get_block(at) as f32);
            for vertex in &mut mesh.vertices {
                vertex.sky_light = sky;
                vertex.block_light = block;
            }
            return mesh;
        }
        let display = ResourceId::parse(&stack.id)
            .ok()
            .and_then(|id| {
                minecraft_terrain::model::item_first_person_hand_transform(packs, &id, left).ok()
            })
            .unwrap_or(glam::Mat4::IDENTITY);
        let _ = self.items.append_posed_blocks(
            &mut mesh,
            &[(pose * display, light_at, stack.id.clone())],
            packs,
            atlas,
            light,
        );
        mesh
    }

    /// An item in a hand of the inventory's player, under `pose` and that
    /// hand's display transform, shaded by `shade` (the GUI's lights).
    #[allow(clippy::too_many_arguments)]
    pub fn hand_item_mesh(
        &mut self,
        mesh: &mut ChunkMesh,
        stack: &ItemStack,
        pose: glam::Mat4,
        left: bool,
        shade: &dyn Fn(Vec3) -> f32,
        packs: &PackStack,
        atlas: &Atlas,
    ) {
        // A special model renderer's item (a shield, a trident) first.
        if minecraft_terrain::special_icon::append_special_in_hand(
            mesh,
            packs,
            atlas,
            &stack.id,
            stack.components.as_ref(),
            pose,
            left,
            false,
            shade,
        )
        .unwrap_or(false)
        {
            return;
        }
        let tint = crate::creative::potion_color(stack)
            .map(|c| [(c >> 16) & 255, (c >> 8) & 255, c & 255].map(|v| v as f32 / 255.0));
        let _ = self
            .items
            .append_hand_item(mesh, &stack.id, pose, left, tint, shade, packs, atlas);
    }

    /// A mob's feet, width and height, as of the last server tick.
    pub fn mob_bounds(&self, id: u64) -> Option<crate::emitters::Bounds> {
        self.world.mob_bounds(id)
    }

    /// The nearest living mob on the look ray within reach, and how far.
    pub fn mob_on_ray(&self, eye: DVec3, look: DVec3, reach: f64) -> Option<f64> {
        self.world
            .mob_on_ray(eye, look, reach)
            .map(|(_, distance)| distance)
    }

    /// An attack on the mob the player looks at; whether there was one.
    pub fn attack(&mut self, eye: DVec3, look: DVec3, reach: f64, attack: PlayerAttack) -> bool {
        let Some((hit, _)) = self.world.mob_on_ray(eye, look, reach) else {
            return false;
        };
        self.server.mob_action(
            hit,
            Some(attack),
            &self.inventory.clone(),
            self.selected,
            false,
        );
        true
    }

    /// The held item used on the mob the player looks at (shears on a
    /// sheep, a bucket on a cow, wheat to breed); whether there was one.
    pub fn interact(&mut self, eye: DVec3, look: DVec3, reach: f64, creative: bool) -> bool {
        let Some((hit, _)) = self.world.mob_on_ray(eye, look, reach) else {
            return false;
        };
        self.server
            .mob_action(hit, None, &self.inventory.clone(), self.selected, creative);
        true
    }

    /// A block the player placed or broke, for the level the mobs walk in.
    pub fn edited(&mut self, scene: &HandcraftedScene, pos: (i32, i32, i32), placed: bool) {
        self.server.player_edit(
            scene,
            pos,
            if placed {
                PlayerEdit::Place
            } else {
                PlayerEdit::Break
            },
        );
    }

    /// A block the player placed from `stack`: a container takes the
    /// stack's components (a shulker box's contents, a name).
    pub fn placed(&mut self, scene: &HandcraftedScene, pos: (i32, i32, i32), stack: &ItemStack) {
        self.server.player_place(scene, pos, stack);
    }

    /// The block `broken` the player broke at `pos`, and how: true when the
    /// server makes its drops (containers, whose loot reads what they hold),
    /// so the client drops none of its own.
    pub fn broke(
        &mut self,
        scene: &HandcraftedScene,
        pos: (i32, i32, i32),
        broken: &minecraft_terrain::scene::Block,
        breaker: minecraft_terrain::server::Breaker,
    ) -> bool {
        self.server.player_break(scene, pos, broken, breaker)
    }

    /// A mob of `kind` (`cow`, `zombie`) at a point, as a spawn egg makes one.
    pub fn summon(&mut self, kind: &str, at: [f64; 3]) {
        self.server.summon(format!("minecraft:{kind}"), at, None);
    }

    /// A right click on a block the level acts on (doors, levers, buttons);
    /// whether it did.
    pub fn use_block(
        &mut self,
        scene: &HandcraftedScene,
        pos: (i32, i32, i32),
        facing: &'static str,
    ) -> bool {
        self.server.use_block(scene, pos, facing)
    }

    /// `BoneMealItem.useOn` on a clicked face; whether it took arrives with
    /// the server's output.
    pub fn bone_meal(&mut self, pos: (i32, i32, i32), face: &'static str) {
        self.server.bone_meal(pos, face);
    }

    /// A left click on a block the level acts on (a note block plays).
    pub fn attack_block(&mut self, scene: &HandcraftedScene, pos: (i32, i32, i32)) {
        self.server.attack_block(scene, pos);
    }

    /// Ticks the client's mobs and sends a server tick when one is due, then
    /// takes what came back.
    pub fn tick(
        &mut self,
        dt: f64,
        day_ticks: i64,
        bright_outside: bool,
        player: &mut PlayerView,
    ) -> Events {
        if !self.server.running() {
            // Mobs, items and saving all stop with it: end the game, with
            // the server's own crash report beside the log.
            panic!("the integrated server stopped");
        }
        self.clock += dt;
        // The server keeps vanilla's 20 ticks a second at any frame rate,
        // catching up after a slow frame as the game's own ticks do.
        let mut due = 0;
        while self.clock >= TICK_SECONDS && due < 10 {
            self.clock -= TICK_SECONDS;
            due += 1;
        }
        if due == 10 {
            self.clock = self.clock.min(TICK_SECONDS);
        }
        let ticked = due > 0;
        for _ in 0..due {
            self.ticks += 1;
            // Packets first, then the client level's entity ticks.
            self.client.tick();
            for enderman in self.world.endermen() {
                if let Some(mob) = self.client.get(enderman.id) {
                    self.portal.emit_enderman(mob.position, 0.6, 2.9);
                }
            }
            self.portal.tick();
            let feet = player.feet;
            let center = ((feet.x.floor() as i32) >> 4, (feet.z.floor() as i32) >> 4);
            let candidate = PlayerCandidate {
                id: PLAYER,
                position: feet,
                eye_height: player.eye_height,
                main_hand_cow_food: false,
                offhand_cow_food: false,
                main_hand_pig_food: false,
                offhand_pig_food: false,
                main_hand_chicken_food: false,
                offhand_chicken_food: false,
                main_hand_carrot_on_a_stick: false,
                offhand_carrot_on_a_stick: false,
                main_hand_wolf_interest: false,
                offhand_wolf_interest: false,
                main_hand_horse_tempt: false,
                offhand_horse_tempt: false,
                alive: player.alive,
                spectator: !player.alive,
                attackable: player.alive,
            };
            self.server.tick(TickInput {
                day_ticks,
                players: if player.alive {
                    vec![feet.to_array()]
                } else {
                    Vec::new()
                },
                difficulty: 2,
                simulation_center: center,
                simulation_distance: 8,
                pickup: player.alive.then(|| {
                    (
                        feet.to_array(),
                        Box::new(self.inventory.clone()),
                        self.selected,
                    )
                }),
                mob_players: vec![candidate],
                mob_views: vec![(
                    PLAYER,
                    minecraftoss_entities::enderman::PlayerView {
                        head_yaw: player.yaw,
                        pitch: player.pitch,
                        disguised: false,
                    },
                )],
                mob_vitals: vec![(
                    PLAYER,
                    minecraftoss_entities::monster_ai::PlayerVitals {
                        health: player.health,
                        velocity: player.velocity,
                        ..Default::default()
                    },
                )],
                bright_outside,
                tracking: (feet.to_array(), 160.0),
                player_hurts: std::mem::take(&mut player.hurts),
                spawn_mobs: true,
            });
        }
        let mut events = Events::default();
        for output in self.server.poll() {
            self.server_handled = output.handled;
            if let Some(snapshot) = output.entities {
                self.server_snapshot = Some(snapshot);
            }
            self.server_picked.extend(output.picked);
            events.changes.extend(output.changes);
            events.block_entities.extend(output.block_entities);
            events.bone_meal_used.extend(output.bone_meal_used);
            for summoned in &output.summoned {
                if let Err(error) = summoned {
                    log!("Could not summon: {error}");
                }
            }
            events.experience += output
                .orbs_taken
                .iter()
                .map(|&(_, _, value)| value.max(0) as u32)
                .sum::<u32>();
            for sound in &output.mob_sounds {
                self.sounds.push((
                    sound.event.clone(),
                    sound.position,
                    sound.volume,
                    sound.pitch,
                ));
            }
            events.mob_events.extend(output.entity_events.iter().copied());
            if let Some(moving) = output.moving_blocks {
                self.moving_blocks = moving;
            }
            for result in &output.mob_results {
                events.mob_events.extend(result.events.iter().copied());
                events.attacks.extend(result.attack.clone());
                if result.used {
                    events.use_taken |= result.handled;
                    events.use_passed |= !result.handled;
                }
                for sound in &result.sounds {
                    self.sounds.push((
                        sound.event.clone(),
                        sound.position,
                        sound.volume,
                        sound.pitch,
                    ));
                }
                // What the action did to the held item: a bucket of milk, a
                // spent wheat, worn shears.
                for (slot, stack) in &result.slots {
                    if let Some(held) = self.inventory.slots.get_mut(*slot) {
                        *held = stack.clone();
                    }
                }
            }
            // `ServerExplosion`'s sound: loud, pitched down.
            for blast in &output.explosions {
                let pitch =
                    (1.0 + (self.random.next_float() - self.random.next_float()) * 0.2) * 0.7;
                self.sounds.push((
                    "minecraft:entity.generic.explode".to_owned(),
                    blast.position,
                    4.0,
                    pitch,
                ));
            }
            if let Some(mobs) = output.mobs {
                self.world = *mobs;
                for (feet, width, height) in self.client.receive(server_mobs(&self.world)) {
                    self.poof.spawn(feet, width, height);
                }
            }
            events.hits.extend(
                output
                    .player_hits
                    .into_iter()
                    .filter(|h| h.player_id == PLAYER),
            );
        }
        if ticked {
            self.server_items_tick(player.feet);
            // `ItemEntity.playerTouch`'s pickup pop.
            for _ in 0..self.world_items.take_pickup_sounds() {
                let pitch =
                    ((self.random.next_float() - self.random.next_float()) * 0.7 + 1.0) * 2.0;
                self.sounds.push((
                    "minecraft:entity.item.pickup".to_owned(),
                    player.feet,
                    0.2,
                    pitch,
                ));
            }
        }
        events
    }

    /// Whether a server tick ran since the last frame, and the partial tick.
    pub fn partial(&self) -> f32 {
        (self.clock / TICK_SECONDS).clamp(0.0, 1.0) as f32
    }

    /// Steps the client-side puffs, which settle on the scene's blocks.
    pub fn tick_scene(&mut self, scene: &HandcraftedScene) {
        self.poof.tick(scene);
    }

    /// The mobs this frame, drawn with the viewer's renderers.
    pub fn meshes(
        &mut self,
        scene: &HandcraftedScene,
        packs: &PackStack,
        atlas: &Atlas,
        light: &SkyLight,
        forward: Vec3,
        camera: DVec3,
        sky_darken: u8,
    ) -> MobMeshes {
        use minecraft_terrain::*;
        // Mobs appear on the client once their trackers start.
        self.client.spawn_missing(&server_mobs(&self.world));
        let w = &self.world;
        let poses = &self.client;
        let partial = self.partial();
        let mut out = MobMeshes::default();
        cow_render::append_cows(
            &mut out.models,
            w.cows().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        sheep_render::append_sheep(
            &mut out.models,
            w.sheep().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        pig_render::append_pigs(
            &mut out.models,
            w.pigs().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        chicken_render::append_chickens(
            &mut out.models,
            w.chickens().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        bat_render::append_bats(
            &mut out.culled,
            w.bats().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        let zombie_items = zombie_render::append_zombies(
            &mut out.models,
            w.zombies().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        creeper_render::append_creepers(
            &mut out.models,
            w.creepers().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        spider_render::append_spiders(
            &mut out.models,
            w.spiders().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        let skeleton_items = skeleton_render::append_skeletons(
            &mut out.models,
            w.skeletons().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        let villager_items = villager_render::append_villagers(
            &mut out.models,
            w.villagers().iter(),
            poses,
            &|pos| Scene::block(scene, pos).and_then(|b| b.properties.get("facing").cloned()),
            atlas,
            light,
            partial,
        );
        horse_render::append_horses(
            &mut out.models,
            &mut out.translucent,
            w.cows().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        slime_render::append_slimes(
            &mut out.models,
            &mut out.translucent,
            w.slimes().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        let carried = enderman_render::append_endermen(
            &mut out.models,
            w.endermen().iter(),
            poses,
            atlas,
            light,
            partial,
            self.ticks.rotate_left(32) ^ (partial.to_bits() as u64),
        );
        let witch_items = witch_render::append_witches(
            &mut out.models,
            w.witches().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        let poppies = golem_render::append_iron_golems(
            &mut out.models,
            w.iron_golems().iter(),
            poses,
            atlas,
            light,
            partial,
        );
        wolf_render::append_wolves(
            &mut out.models,
            w.wolves().iter(),
            poses,
            atlas,
            light,
            partial,
            w.game_time(),
        );
        flame_render::append_flames(
            &mut out.items,
            w.burning()
                .into_iter()
                .map(|(previous, now, width, height)| {
                    (previous.lerp(now, f64::from(partial)), width, height)
                }),
            atlas,
            forward,
            light,
        );
        witch_render::append_potions(
            &mut out.items,
            w.potions().iter().filter(|p| p.potion.alive).map(|p| {
                (
                    p,
                    p.previous_position
                        .lerp(p.potion.position, f64::from(partial)),
                )
            }),
            atlas,
            forward,
            light,
        );
        if let Ok(items) = self
            .items
            .mesh(&self.world_items, packs, atlas, light, partial)
        {
            let base = out.items.vertices.len() as u32;
            out.items.vertices.extend(items.vertices);
            out.items
                .indices
                .extend(items.indices.iter().map(|i| i + base));
        }
        self.poof
            .append_mesh(&mut out.items, atlas, forward, partial, light);
        self.portal
            .append_mesh(&mut out.items, atlas, forward, partial, light);
        let held: Vec<_> = skeleton_items
            .into_iter()
            .chain(zombie_items)
            .chain(villager_items)
            .chain(witch_items)
            .collect();
        let _ = self
            .items
            .append_held_items(&mut out.items, &held, packs, atlas, light);
        let carried: Vec<_> = carried.into_iter().chain(poppies).collect();
        let _ = self
            .items
            .append_posed_blocks(&mut out.items, &carried, packs, atlas, light);
        // Blocks pistons are moving (`PistonHeadRenderer`).
        if !self.moving_blocks.is_empty() {
            let tint = self.tint.get_or_insert_with(|| {
                mesh::BiomeTint::from_pack(packs).unwrap_or_else(|_| mesh::BiomeTint::empty())
            });
            let parts: Vec<_> = self
                .moving_blocks
                .iter()
                .flat_map(|view| mesh::moving_block_parts(view, partial))
                .collect();
            let _ = mesh::append_moving_blocks(&mut out.items, scene, &parts, packs, atlas, tint, light);
        }
        // Their shadows, at `getMaxLocalRawBrightness`.
        let casters = client_mobs::shadow_casters(w, poses, camera, partial);
        let raw = |pos: (i32, i32, i32)| {
            light
                .get(pos)
                .saturating_sub(sky_darken)
                .max(light.get_block(pos))
        };
        if let Ok(shadows) = mesh::entity_shadows(&casters, scene, atlas, &raw, 0.0) {
            out.shadows = shadows;
        }
        out
    }
}

/// The viewer's `server_bob_offset`: a server item's bob phase from its id.
fn bob_offset(id: i32) -> f32 {
    let hash = (id as u32)
        .wrapping_mul(0x9e37_79b9)
        .rotate_left(13)
        .wrapping_mul(0x85eb_ca6b);
    hash as f32 / u32::MAX as f32 * std::f32::consts::TAU
}
