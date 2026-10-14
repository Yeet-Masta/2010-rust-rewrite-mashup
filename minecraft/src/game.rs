//! The game: one player in a MinecraftOSS world. Movement, survival and the
//! inventory run on vanilla's 20 Hz tick through MinecraftOSS's player crate;
//! the world streams, mobs move and the frame is drawn every frame.
use std::path::PathBuf;
use std::time::Instant;

use glam::{DVec3, Mat3, Mat4, Vec3};
use minecraft_terrain::mesh::SectionVertex;
use minecraft_terrain::scene::{Block, BlockPos, Scene};
use minecraft_terrain::sections::CullCamera;
use minecraftoss_entities::world::{PlayerAttack, PlayerHitKind};
use minecraftoss_player::food::{FoodUse, FoodUseTick};
use minecraftoss_player::inventory::ItemStack;
use minecraftoss_player::effects::{Attribute, EffectInstance, MobEffect};
use minecraftoss_player::survival::{Armor, Difficulty};
use minecraftoss_player::{GameMode, HitFrom, IncomingHit, Player};

use crate::entities::{Entities, PlayerView};
use crate::gui::{Button, Gui, Hud, Screen, Slot};
use crate::mining::Mining;
use crate::particles::Particles;
use crate::render::{Renderer, UiList, WorldDraw};
use crate::sounds::Sounds;
use crate::world::World;

#[path = "creative_screen.rs"]
mod creative_screen;
#[path = "menu_screen.rs"]
mod menu_screen;
#[path = "ranged.rs"]
mod ranged;

const TICK_SECONDS: f64 = 1.0 / 20.0;
/// Vanilla's default field of view.
const FOV: f32 = 70.0;
/// Degrees of turn per pixel of mouse travel at the default sensitivity.
const LOOK: f64 = 0.15;

/// Keys and buttons the game reads, gathered by the window.
#[derive(Default)]
pub struct Input {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    /// Space presses not yet seen by a tick: a tap between two frames
    /// still jumps, and two quick taps still toggle flying.
    pub jump_taps: u32,
    pub sneak: bool,
    pub sprint: bool,
    pub shift: bool,
    pub ctrl: bool,
    /// Mouse travel since the last frame, in pixels.
    pub look: (f64, f64),
    /// The cursor in window pixels.
    pub mouse: (f32, f32),
    pub attack: bool,
    pub use_item: bool,
    /// Buttons pressed and released this frame: (right button, pressed).
    pub clicks: Vec<(bool, bool)>,
    pub middle_click: bool,
    /// The middle button pressed (true) and released this frame, for a
    /// screen's drags.
    pub middle_clicks: Vec<bool>,
    pub scroll: f32,
    pub keys: Vec<Key>,
    /// The hotbar save (C) and load (X) activators held.
    pub save_hotbar: bool,
    pub load_hotbar: bool,
}

/// Keys with an action of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Inventory,
    Escape,
    Drop,
    Hotbar(usize),
    /// F: the hands swap (over a screen's slot, it swaps with the offhand).
    SwapOffhand,
    Debug,
    HideHud,
    Screenshot,
    Forward,
    /// The chat key, which opens the creative search tab.
    Chat,
    Backspace,
    /// A character typed.
    Char(char),
}

pub struct Options {
    pub seed: i64,
    pub view_distance: i32,
    pub creative: bool,
    pub save: Option<PathBuf>,
    pub time: Option<f64>,
    /// `/weather` at the start: rain, and thunder with it.
    pub weather: Option<(bool, bool)>,
    /// Vanilla's VSync video setting, on by default; off, the frame rate
    /// is held to its default maximum of 120.
    pub vsync: bool,
}

#[derive(Default)]
struct Walk {
    /// `walkDist`, `bob` and their values a tick ago.
    dist: f32,
    previous_dist: f32,
    bob: f32,
    previous_bob: f32,
    /// `moveDist` and `nextStep`, for footsteps.
    move_dist: f32,
    next_step: f32,
    /// `walkAnimation`'s position and speed, for the model's legs.
    animation_pos: f32,
    animation_speed: f32,
}

pub struct Game {
    pub world: World,
    pub player: Player,
    pub creative: bool,
    pub entities: Entities,
    pub screen: Screen,
    pub quit: bool,
    sounds: Sounds,
    mining: Mining,
    pub particles: Particles,
    emitters: crate::emitters::Emitters,
    ambient: crate::ambient::Ambient,
    /// Rain, snow and lightning as the client shows them.
    weather: crate::weather::Weather,
    save_dir: Option<PathBuf>,
    previous: DVec3,
    eye_height: f64,
    previous_eye_height: f64,
    clock: f64,
    ticks: u64,
    /// Whether the player's chunk has arrived, so its physics can run.
    landed: bool,
    swing: crate::hand::Swing,
    attack_presses: u32,
    use_presses: u32,
    use_delay: u32,
    /// How far the held item is raised, and its value a tick ago.
    equip: f32,
    previous_equip: f32,
    equipped: Option<String>,
    eating: Option<FoodUse>,
    /// The item in use that is not eaten (a bow drawn).
    using: Option<ranged::ItemUse>,
    walk: Walk,
    fov: f32,
    previous_fov: f32,
    /// The last forward press, for double-tap sprinting.
    forward_tapped: Option<u64>,
    tap_sprint: bool,
    highlight: Option<(ItemStack, f64)>,
    highlight_slot: Option<(usize, Option<ItemStack>)>,
    debug: bool,
    hide_hud: bool,
    pub screenshot: bool,
    hurts: Vec<(Option<u64>, &'static str)>,
    drag: Option<(bool, Vec<Slot>)>,
    /// The last slot pressed, with which button and when, for double clicks.
    last_click: Option<(Slot, bool, Instant)>,
    score: u32,
    frames: (u32, f64, u32),
    /// Survival status before this tick, for its hurt sounds.
    health_before: f32,
    creative_screen: creative_screen::CreativeScreen,
    jump_taps: u32,
    jump_latched: bool,
    /// The tick the player died on.
    died_at: u64,
    /// The menu open on the server, with its screen.
    menu: Option<menu_screen::ClientMenu>,
    /// A use of a menu block the server has not answered yet.
    use_pending: Option<BlockPos>,
    /// The player's ender inventory, as the server last sent it, to save.
    ender_items: Vec<Option<ItemStack>>,
    /// `Player.enchantmentSeed`, saved as `XpSeed`: what the enchanting
    /// table offers from. A new player's is 0 until a menu re-rolls it.
    enchantment_seed: i32,
    /// The action bar's message, and the ticks it has left.
    overlay: Option<(serde_json::Value, u32)>,
    /// `LocalPlayer.spinningEffectTime` and `spinningEffectSpeed`, for
    /// nausea's warp.
    spin: (f32, f32),
    /// `Hud`'s `lastHealth`, `healthBlinkTime`, `displayHealth` and
    /// `lastHealthTime`.
    health_blink: (i32, u64, i32, Instant),
}

impl Game {
    pub fn new(world: World, options: &Options) -> Self {
        let saved = options.save.as_deref().and_then(crate::save::read);
        let jar = world.jar.clone();
        let mut entities = Entities::new(
            &world.stream,
            world.seed,
            jar.as_deref(),
            world.item_catalog.as_deref(),
        );
        let loot = jar.as_deref().and_then(|jar| {
            minecraftoss_player::loot::LootBook::from_jar(jar)
                .map_err(|e| log!("Block loot unavailable: {e:#}"))
                .ok()
        });
        let mining = Mining::new(&world.registries, loot, world.seed);
        let sounds = Sounds::load(&world.packs);
        let particles = Particles::new(&world.packs, &world.atlas, world.seed as u64);
        let (x, y, z) = world.stream.player_spawn;
        let mut player = Player::new(DVec3::new(x, y, z));
        let mut creative = options.creative;
        let mut world = world;
        // A chunk the server unloads may still have changes on their way
        // back: the chunk map keeps it until they are in.
        world.stream.hold_forgotten_chunks();
        if let Some(saved) = saved.as_ref() {
            creative = saved.creative || options.creative;
            player.pos = DVec3::from_array(saved.position);
            player.yaw = saved.yaw;
            player.pitch = saved.pitch;
            // `readAdditionalSaveData`: the effects, with their maxima,
            // before the health and absorption they bound.
            player.survival.effects = saved.effects.clone();
            player.survival.refresh_attributes();
            player.survival.health = saved.health.clamp(1.0, player.survival.max_health);
            player.survival.set_absorption(saved.absorption);
            player.survival.fire_ticks = saved.fire;
            player.survival.food.level = saved.food;
            player.survival.food.saturation = saved.saturation;
            (
                player.survival.experience_level,
                player.survival.experience_progress,
                player.survival.total_experience,
            ) = saved.experience;
            world.day.set(saved.day_ticks);
            entities.selected = saved.selected;
            let recipes = entities.inventory.recipes.clone();
            // The item's own stack size, which worn tools keep.
            let sized = |stack: &Option<ItemStack>| {
                stack.clone().map(|mut stack| {
                    stack.max = stack
                        .components
                        .as_ref()
                        .and_then(|c| c.get("minecraft:max_stack_size")?.as_u64())
                        .map_or_else(
                            || recipes.max_stack(&stack.id),
                            |max| max.clamp(1, 99) as u8,
                        );
                    stack
                })
            };
            for (slot, stack) in entities.inventory.slots.iter_mut().zip(&saved.slots) {
                *slot = sized(stack);
            }
            let ender: Vec<Option<ItemStack>> = saved.ender.iter().map(sized).collect();
            entities.set_ender_items(ender);
            if let Some(weather) = saved.weather.clone() {
                entities.load_weather(weather);
            }
        }
        if let Some((raining, thundering)) = options.weather {
            entities.set_weather(raining, thundering);
        }
        if let Some(ticks) = options.time {
            world.day.set(ticks);
        }
        player.set_game_mode(if creative {
            GameMode::Creative
        } else {
            GameMode::Survival
        });
        player.flying = creative && saved.as_ref().is_some_and(|saved| saved.flying);
        let previous = player.pos;
        let health = player.survival.health;
        let new_world = saved.is_none();
        let ender_items = saved
            .as_ref()
            .map_or_else(Vec::new, |saved| saved.ender.clone());
        // `Player.readAdditionalSaveData`: a saved seed of 0 becomes a
        // random one, as does a new world's.
        let enchantment_seed = match saved.as_ref().map(|saved| saved.enchantment_seed) {
            Some(0) | None => {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as u64);
                minecraftoss_player::rng::LegacyRandom::new(nanos).next_i32()
            }
            Some(seed) => seed,
        };
        let mut game = Self {
            world,
            player,
            creative,
            entities,
            screen: Screen::Playing,
            quit: false,
            sounds,
            mining,
            particles,
            emitters: crate::emitters::Emitters::default(),
            ambient: crate::ambient::Ambient::default(),
            weather: crate::weather::Weather::new(),
            save_dir: options.save.clone(),
            previous,
            eye_height: 1.62,
            previous_eye_height: 1.62,
            clock: 0.0,
            ticks: 0,
            landed: false,
            swing: crate::hand::Swing::default(),
            attack_presses: 0,
            use_presses: 0,
            use_delay: 0,
            equip: 1.0,
            previous_equip: 1.0,
            equipped: None,
            eating: None,
            using: None,
            walk: Walk::default(),
            fov: 1.0,
            previous_fov: 1.0,
            forward_tapped: None,
            tap_sprint: false,
            highlight: None,
            highlight_slot: None,
            debug: false,
            hide_hud: false,
            screenshot: false,
            hurts: Vec::new(),
            drag: None,
            last_click: None,
            score: 0,
            frames: (0, 0.0, 0),
            health_before: health,
            creative_screen: creative_screen::CreativeScreen::new(options.save.as_deref()),
            jump_taps: 0,
            jump_latched: false,
            died_at: 0,
            menu: None,
            use_pending: None,
            ender_items,
            enchantment_seed,
            overlay: None,
            spin: (0.0, 0.0),
            health_blink: (0, 0, 0, Instant::now()),
        };
        // A new world's seed is on disk before any of its chunks.
        if new_world {
            game.save();
        }
        game
    }

    fn mode(&self) -> GameMode {
        if self.creative {
            GameMode::Creative
        } else {
            GameMode::Survival
        }
    }

    fn alive(&self) -> bool {
        self.creative || self.player.survival.health > 0.0
    }

    /// The game menu, as Esc or the window going to the background opens
    /// it; not over the loading screen, which has to finish first.
    pub fn pause(&mut self) {
        if self.screen == Screen::Playing && self.landed {
            self.screen = Screen::Paused;
        }
    }

    /// Whether the mouse drives the camera: in play, with no screen open.
    pub fn captures_mouse(&self) -> bool {
        self.screen == Screen::Playing
    }

    fn block_reach(&self) -> f64 {
        if self.creative { 5.0 } else { 4.5 }
    }

    fn entity_reach(&self) -> f64 {
        if self.creative { 5.0 } else { 3.0 }
    }

    fn feet_block(&self) -> BlockPos {
        let p = self.player.pos;
        (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
    }

    fn eyes_in_water(&self) -> bool {
        let eye = self.player.eye().floor();
        self.world
            .block((eye.x as i32, eye.y as i32, eye.z as i32))
            .is_some_and(|b| {
                b.id.path == "water" || b.properties.get("waterlogged").is_some_and(|w| w == "true")
            })
    }

    fn play(&mut self, event: &str, at: Option<DVec3>, volume: f32, pitch: f32) {
        self.sounds
            .play(&self.world.packs, event, at, volume, pitch);
    }

    fn start_swing(&mut self) {
        self.swing.start(self.swing_duration());
    }

    /// `getCurrentSwingDuration`: six ticks, quicker with haste and
    /// slower with mining fatigue.
    fn swing_duration(&self) -> i32 {
        self.player.survival.effects.swing_duration(6)
    }

    /// Puts away what an open screen holds, and saves: the changed chunks,
    /// then the player, so the player never runs ahead of the world. The
    /// rest of the world is written as it drops.
    pub fn shutdown(&mut self) {
        if matches!(
            self.screen,
            Screen::Inventory | Screen::Crafting | Screen::Creative
        ) {
            self.close_container();
        }
        // A menu's container closes first (a barrel saves shut), and the
        // carried stack comes back.
        self.settle_menu();
        // Leftovers that didn't fit are items on the ground: they save
        // with the server's entities.
        self.entities.hand_over_drops();
        if self.save_dir.is_some() {
            self.save_chunks();
        }
        self.save();
    }

    /// Saves the chunks changed since they were last saved, once the
    /// server has saved its entities and handed over what its block
    /// entities hold now (`Command::Flush`), so containers save with what
    /// is in them. First everything the server was sent is taken in (a
    /// menu's answer, pickups), so the player saved after this is the one
    /// the world saved with: vanilla saves both from one server state.
    fn save_chunks(&mut self) {
        self.catch_up();
        let block_entities = self.entities.flush();
        self.world.stream.record_block_entities(block_entities);
        self.world.stream.save_edited();
    }

    /// Writes the player beside the world's region files.
    pub fn save(&mut self) {
        let Some(dir) = self.save_dir.as_ref() else {
            return;
        };
        // Died and not respawned: the world opens with the player respawned.
        let dead = !self.creative && self.player.survival.health <= 0.0;
        let position = if dead {
            let (x, y, z) = self.world.stream.respawn_position();
            [x, y, z]
        } else {
            self.player.pos.to_array()
        };
        let status = &self.player.survival;
        let saved = crate::save::Saved {
            seed: self.world.seed,
            creative: self.creative,
            position,
            yaw: self.player.yaw,
            pitch: self.player.pitch,
            health: if dead { 20.0 } else { status.health },
            food: if dead { 20 } else { status.food.level },
            saturation: if dead { 5.0 } else { status.food.saturation },
            experience: if dead {
                (0, 0.0, 0)
            } else {
                (
                    status.experience_level,
                    status.experience_progress,
                    status.total_experience,
                )
            },
            day_ticks: self.world.day.ticks,
            // Death clears them (`LivingEntity.die`'s `removeAllEffects`).
            effects: if dead { Default::default() } else { status.effects.clone() },
            absorption: if dead { 0.0 } else { status.absorption },
            fire: if dead { 0 } else { status.fire_ticks },
            flying: self.player.flying,
            selected: self.entities.selected,
            slots: self.entities.inventory.slots.clone(),
            ender: self.ender_items.clone(),
            enchantment_seed: self.enchantment_seed,
            weather: self.entities.weather.clone(),
        };
        match crate::save::write(dir, &saved) {
            Ok(()) => log!("Saved the world to {}", dir.display()),
            Err(error) => log!("Could not save the world: {error}"),
        }
    }

    /// One frame: input, the ticks due, streaming, mobs, and what to draw.
    pub fn frame(
        &mut self,
        dt: f64,
        input: &mut Input,
        renderer: &mut Renderer,
        gui: &mut Gui,
    ) -> (Option<WorldDraw>, UiList) {
        let size = renderer.size();
        gui.layout(size, input.mouse);
        self.keys(input, gui);
        self.screen_input(input, gui);
        self.flush_menu();
        if self.captures_mouse() {
            self.player.yaw += input.look.0 * LOOK;
            self.player.pitch = (self.player.pitch + input.look.1 * LOOK).clamp(-90.0, 90.0);
            let notches = take_notches(&mut input.scroll);
            if notches != 0 {
                self.entities.selected =
                    (self.entities.selected as i32 - notches).rem_euclid(9) as usize;
            }
            for (right, pressed) in input.clicks.drain(..) {
                if pressed {
                    if right {
                        self.use_presses += 1;
                    } else {
                        self.attack_presses += 1;
                    }
                }
            }
            if input.middle_click && !self.inventory_busy() {
                self.pick_block();
            }
        }
        // Notches no screen used are gone; part of one waits for the next.
        input.scroll = input.scroll.fract();
        input.clicks.clear();
        input.middle_clicks.clear();
        self.jump_taps = (self.jump_taps + std::mem::take(&mut input.jump_taps)).min(4);
        // The pause menu stops the world, as it does in singleplayer.
        let world_dt = if self.screen == Screen::Paused {
            0.0
        } else {
            dt
        };
        self.clock += world_dt;
        let mut ran = 0;
        while self.clock >= TICK_SECONDS && ran < 10 {
            self.clock -= TICK_SECONDS;
            self.tick(input);
            ran += 1;
        }
        if ran == 10 {
            self.clock = 0.0;
        }
        let partial = (self.clock / TICK_SECONDS) as f32;

        // Chunks around the player, and the mobs in them.
        let feet = self.feet_block();
        let (loaded, forgotten) = self.world.stream.server_tick(feet, &mut self.world.scene);
        for chunk in &loaded {
            self.entities.load_chunk(chunk);
        }
        for pos in forgotten {
            self.entities.unload_chunk(pos);
        }
        let mut view = PlayerView {
            feet: self.player.pos,
            eye_height: self.player.eye_height() as f32,
            alive: self.alive() && self.landed,
            health: self.player.survival.health,
            yaw: self.player.yaw as f32,
            pitch: self.player.pitch as f32,
            velocity: self.player.velocity,
            hurts: std::mem::take(&mut self.hurts),
            creative: self.creative,
            shield: None,
        };
        let bright = self.world.sky_light_level() > 11.0;
        self.entities.hold_pickups = self.pickups_wait();
        let events = self
            .entities
            .tick(world_dt, self.world.day.ticks as i64, bright, &mut view);
        self.hurts = view.hurts;
        self.handle_events(events, input.use_item);
        // Forgotten chunks whose last changes from the server are in may
        // drop now.
        for pos in self.entities.unloads_answered() {
            self.world.stream.release_chunk(pos);
        }
        self.check_death();

        // The camera.
        let pos = self.previous.lerp(self.player.pos, f64::from(partial));
        let eye_height = self.previous_eye_height
            + (self.eye_height - self.previous_eye_height) * f64::from(partial);
        let eye = pos + DVec3::Y * eye_height;
        let look = self.player.look().as_vec3();
        let forward = look.normalize_or(Vec3::Z);
        let up = Vec3::Y;
        let right = forward.cross(up).normalize_or(Vec3::X);
        self.sounds.set_listener(eye, right);
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        let fov_modifier = self.previous_fov + (self.fov - self.previous_fov) * partial;
        let water = if self.eyes_in_water() {
            60.0 / 70.0
        } else {
            1.0
        };
        let fov = FOV * fov_modifier * water;
        let bob = self.bob_matrix(partial);
        let view_matrix = bob * self.nausea_matrix(partial) * Mat4::look_to_rh(Vec3::ZERO, forward, up);
        let projection = Mat4::perspective_infinite_reverse_rh(fov.to_radians(), aspect, 0.05);
        self.world.vision = self.vision(partial);
        let camera = CullCamera {
            position: eye,
            forward,
            fov_degrees: fov.max(90.0),
            aspect,
            yaw_degrees: (-forward.x).atan2(forward.z).to_degrees(),
            pitch_degrees: (-forward.y).asin().to_degrees(),
        };
        let update = self.world.frame(&camera);
        let visible = update.visible.iter().map(|(pos, _)| *pos).collect();
        renderer.update_sections(update.uploads, update.removed);
        renderer.animate(self.ticks);
        let environment = self
            .world
            .environment(world_dt, eye.to_array(), forward, aspect);
        if let Some((vertices, indices)) = self.world.clouds(eye.to_array()) {
            renderer.set_clouds(&vertices, &indices);
        }

        // Mobs, items, particles, cracks, the outline and the hand.
        let sky_darken = (15.0 - self.world.sky_light_level()).clamp(0.0, 15.0) as u8;
        let mut meshes = self.entities.meshes(
            &self.world.scene,
            &self.world.packs,
            &self.world.atlas,
            &self.world.light,
            forward,
            eye,
            sky_darken,
        );
        let particles: Vec<SectionVertex> = meshes
            .items
            .vertices
            .iter()
            .map(SectionVertex::from_vertex)
            .collect();
        let particle_indices = std::mem::take(&mut meshes.items.indices);
        let outline = if self.captures_mouse() && self.alive() && !self.hide_hud {
            self.outline()
        } else {
            Vec::new()
        };
        let hand = if self.hide_hud {
            Default::default()
        } else {
            self.hand_mesh(partial, eye)
        };
        let sprites = self.particles.mesh(
            &self.world,
            &crate::particles::Camera {
                eye,
                right,
                up: right.cross(forward).normalize_or(Vec3::Y),
                yaw: (self.player.yaw as f32).to_radians(),
                pitch: (self.player.pitch as f32).to_radians(),
            },
            partial,
        );
        let draw = WorldDraw {
            clip_from_rel: projection * view_matrix,
            sprites,
            eye: eye.as_vec3().to_array(),
            environment,
            visible,
            particles: (particles, particle_indices),
            entities: [
                std::mem::take(&mut meshes.models),
                std::mem::take(&mut meshes.culled),
                std::mem::take(&mut meshes.translucent),
                std::mem::take(&mut meshes.shadows),
            ],
            cracks: self.mining.crack_mesh(),
            weather: self.weather.mesh(&self.world, eye, self.ticks as i64, partial),
            lightning: crate::weather::Weather::bolt_mesh(self.entities.lightning()),
            outline,
            hand,
            screen_fire: self.screen_fire(),
            hand_clip: {
                // Vanilla's fixed 70 degree hand, reverse-Z with no far plane.
                let f = 1.0 / (35.0f32.to_radians()).tan();
                Mat4::from_cols(
                    glam::Vec4::new(f / aspect, 0.0, 0.0, 0.0),
                    glam::Vec4::new(0.0, f, 0.0, 0.0),
                    glam::Vec4::new(0.0, 0.0, 0.0, -1.0),
                    glam::Vec4::new(0.0, 0.0, 0.05, 0.0),
                )
            },
        };

        // The 2D layer.
        self.frames.0 += 1;
        self.frames.1 += dt;
        if self.frames.1 >= 1.0 {
            self.frames = (0, self.frames.1 - 1.0, self.frames.0);
        }
        let ui = self.ui(gui, dt, update.visible.len());
        gui.flush(renderer);
        (Some(draw), ui)
    }

    /// What the server sent back: block and block entity changes, the
    /// mobs' doings, the menu's updates and the uses' answers, sounds.
    fn handle_events(&mut self, events: crate::entities::Events, use_held: bool) {
        if events.picked_up {
            self.inventory_changed();
        }
        self.world.set_blocks(&events.changes);
        // Before another chunk goes to the server, as it expects.
        self.world
            .stream
            .record_block_entities(events.block_entities);
        // `ExperienceOrb.playerTouch`: each orb first mends (`repairPlayerItems`),
        // and what is left is experience.
        let mut experience = 0;
        for value in events.experience {
            let selected = self.entities.selected;
            let remaining = self.entities.inventory.repair_with_experience(selected, value);
            experience += remaining.max(0) as u32;
        }
        if experience > 0 && !self.creative {
            self.player.survival.give_experience_points(experience);
            self.score += experience;
            let pitch = (self.sounds.random() - self.sounds.random()) * 0.35 + 0.9;
            self.play("minecraft:entity.experience_orb.pickup", None, 0.1, pitch);
        }
        for hit in events.hits {
            self.hurt_by_mob(hit);
        }
        // `BeaconBlockEntity.applyEffects`: each power as an ambient
        // effect with particles.
        for (effect, duration, amplifier) in events.beacon_effects {
            let Some(kind) = MobEffect::from_id(effect) else { continue };
            let instance = EffectInstance::with_flags(kind, duration, amplifier, true, true, true);
            self.player.survival.add_effect_instance(instance);
        }
        for splash in events.splashes {
            self.splashed(splash);
        }
        for broke in events.potion_breaks {
            self.potion_broke(broke);
        }
        for (id, event) in events.mob_events {
            if let Some(bounds) = self.entities.mob_bounds(id) {
                crate::emitters::entity_event(&mut self.particles, &self.world, bounds, event);
            }
        }
        for (id, result) in events.attacks {
            self.attack_particles(id, &result);
            self.attack_landed(&result);
        }
        for pos in events.bone_meal_used {
            let mut sounds = Vec::new();
            crate::ambient::bone_meal(&mut self.particles, &self.world, pos, &mut sounds);
            for (event, at, volume, pitch) in sounds {
                self.play(event, Some(at), volume, pitch);
            }
        }
        for (pos, flint_and_steel) in events.ignited {
            self.ignited(pos, flint_and_steel);
        }
        if events.use_taken {
            self.start_swing();
        }
        // A mob with nothing to do with the held item: it's used in the
        // air, as food is eaten (`MultiPlayerGameMode.useItem`).
        if events.use_passed
            && use_held
            && self.eating.is_none()
            && self.using.is_none()
            && self.captures_mouse()
            && self.alive()
        {
            let main = self.entities.held().cloned();
            if !main.as_ref().is_some_and(|_| self.start_using(self.entities.selected)) {
                let food = &self.player.survival.food;
                self.eating = main.and_then(|stack| FoodUse::start(self.entities.selected, &stack, food, self.mode()));
                if self.eating.is_none() {
                    self.start_using(ranged::OFFHAND);
                }
            }
        }
        for (event, at, volume, pitch) in std::mem::take(&mut self.entities.sounds) {
            self.play(&event, Some(at), volume, pitch);
        }
        for (pos, id, data) in events.level_events {
            self.level_event(pos, id, data);
        }
        for blast in events.explosions {
            let center = DVec3::from_array(blast.center);
            self.particles.explosion(
                &self.world,
                center,
                blast.radius,
                blast.block_count,
                blast.small,
            );
        }
        for update in events.menu {
            self.menu_update(update);
        }
        // A use of a menu block is answered; only one is out at a time.
        for result in events.use_results {
            self.use_pending = None;
            if let Some(message) = result.overlay {
                self.overlay = Some((message, 60));
            }
            // The client keeps no statistics yet: `result.stats` go unread.
        }
        // An answer may have let the next batch go.
        self.flush_menu();
    }

    /// `LevelEventHandler.levelEvent` for the events the server sends: the
    /// menus' blocks' sounds at their centres, pitched from 0.9 to 1.0; the
    /// brewing stand's and the crafter's sounds, at pitch 1, and the
    /// crafter's smoke out of its front.
    fn level_event(&mut self, pos: BlockPos, id: i32, data: i32) {
        let at = DVec3::new(
            f64::from(pos.0) + 0.5,
            f64::from(pos.1) + 0.5,
            f64::from(pos.2) + 0.5,
        );
        let (event, pitch) = match id {
            1029 => ("minecraft:block.anvil.destroy", None),
            1035 => ("minecraft:block.brewing_stand.brew", Some(1.0)),
            1030 => ("minecraft:block.anvil.use", None),
            1042 => ("minecraft:block.grindstone.use", None),
            1044 => ("minecraft:block.smithing_table.use", None),
            1049 => ("minecraft:block.crafter.craft", Some(1.0)),
            1050 => ("minecraft:block.crafter.fail", Some(1.0)),
            2010 => return self.shoot_particles(pos, data, crate::particles::Type::WhiteSmoke),
            _ => return,
        };
        let pitch = pitch.unwrap_or_else(|| self.sounds.random() * 0.1 + 0.9);
        self.play(event, Some(at), 1.0, pitch);
    }

    /// A potion that broke near the player (`ThrownSplashPotion.onHitAsPotion`
    /// and `affectEntitiesAround` on the player's side): its lasting
    /// effects, its instant ones at the splash's nearness, and water's
    /// dousing.
    fn splashed(&mut self, splash: minecraftoss_entities::world::PlayerSplash) {
        if !self.alive() {
            return;
        }
        let health = self.player.survival.health;
        for effect in splash.effects {
            self.player.survival.add_effect_instance(effect);
        }
        for (kind, amplifier, scale) in splash.instant {
            self.player.survival.apply_instant(kind, amplifier, scale, self.creative);
        }
        // `extinguishFire`, with its hiss when it was burning.
        if splash.extinguish && self.player.survival.on_fire() {
            self.player.survival.clear_fire();
            let pitch = 1.6 + (self.sounds.random() - self.sounds.random()) * 0.4;
            self.play("minecraft:entity.generic.extinguish_fire", Some(self.player.pos), 0.7, pitch);
        }
        if self.player.survival.health < health {
            self.play("minecraft:entity.player.hurt", None, 1.0, 1.0);
            self.player.hurt_time = 10;
        }
    }

    /// `LevelEventHandler.potionSplashParticles` (events 2002 and 2007)
    /// and the break's sound (1053 and 1054): eight shards of the bottle
    /// and a hundred motes in the potion's colour flung out in a ring.
    fn potion_broke(&mut self, broke: minecraftoss_entities::world::PotionBreak) {
        use crate::particles::{Options, Type};
        let block = broke.position.floor();
        let at = DVec3::new(block.x + 0.5, block.y, block.z + 0.5);
        let shard = Options::Item("minecraft:splash_potion".to_owned());
        for _ in 0..8 {
            let random = &mut self.particles.random;
            let motion = DVec3::new(random.next_gaussian() * 0.15, random.next_double() * 0.2, random.next_gaussian() * 0.15);
            self.particles.spawn(&self.world, &shard, at, motion);
        }
        let rgb = [(broke.color >> 16) & 255, (broke.color >> 8) & 255, broke.color & 255].map(|c| c as f32 / 255.0);
        let kind = if broke.instant { Type::InstantEffect } else { Type::Effect };
        for _ in 0..100 {
            let random = &mut self.particles.random;
            let distance = random.next_double() * 4.0;
            let angle = random.next_double() * std::f64::consts::PI * 2.0;
            let (vx, vz) = (angle.cos() * distance, angle.sin() * distance);
            let vy = 0.01 + random.next_double() * 0.5;
            let brightness = 0.75 + random.next_float() * 0.25;
            let options = Options::Spell(kind, rgb.map(|c| c * brightness), distance as f32);
            let pos = DVec3::new(at.x + vx * 0.1, at.y + 0.3, at.z + vz * 0.1);
            self.particles.spawn(&self.world, &options, pos, DVec3::new(vx, vy, vz));
        }
        let pitch = self.sounds.random() * 0.1 + 0.9;
        self.play("minecraft:entity.splash_potion.break", Some(at), 1.0, pitch);
    }

    /// `LivingEntity.tickEffects` on the client: now and then (one tick in
    /// four, fifteen when invisible, five times rarer when every effect is
    /// ambient) a swirl in one visible effect's colour, faint for an
    /// ambient one, somewhere in the player's box.
    fn effect_particles(&mut self) {
        use crate::particles::{Options, Type};
        let effects = &self.player.survival.effects;
        let colours: Vec<[f32; 4]> = effects
            .iter()
            .filter(|e| e.visible)
            .map(|e| {
                let c = e.effect.color();
                let alpha = if e.ambient { 38.0 / 255.0 } else { 1.0 };
                [alpha, ((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0]
            })
            .collect();
        if colours.is_empty() {
            return;
        }
        let bound = if effects.has(MobEffect::Invisibility) { 15 } else { 4 } * if effects.all_ambient() { 5 } else { 1 };
        let random = &mut self.particles.random;
        if random.next_int(bound) != 0 {
            return;
        }
        let colour = colours[random.next_int(colours.len() as i32) as usize];
        let p = self.player.pos;
        let height = if self.player.swimming { 0.6 } else if self.player.crouching { 1.5 } else { 1.8 };
        let x = p.x + 0.6 * (2.0 * random.next_double() - 1.0) * 0.5;
        let y = p.y + height * random.next_double();
        let z = p.z + 0.6 * (2.0 * random.next_double() - 1.0) * 0.5;
        self.particles.spawn(&self.world, &Options::Color(Type::EntityEffect, colour), DVec3::new(x, y, z), DVec3::ONE);
    }

    /// `AreaEffectCloud.clientTick`: a waiting cloud's two motes at its
    /// heart, every other tick, or a working cloud's mote for each square
    /// block it covers, in the potion's colour (waiting, half of them
    /// white).
    fn cloud_particles(&mut self) {
        use crate::particles::{Options, Type};
        use minecraftoss_player::mth::{cos, sin};
        let clouds: Vec<_> = self.entities.clouds().iter().map(|c| (c.position, c.radius, c.waiting, c.color())).collect();
        for (at, radius, waiting, color) in clouds {
            let random = &mut self.particles.random;
            if waiting && random.next_bool() {
                continue;
            }
            let (count, spread) = if waiting {
                (2, 0.2_f32)
            } else {
                ((std::f32::consts::PI * radius * radius).ceil() as i32, radius)
            };
            let rgb = [(color >> 16) & 255, (color >> 8) & 255, color & 255].map(|c| c as f32 / 255.0);
            for _ in 0..count {
                let random = &mut self.particles.random;
                let angle = random.next_float() * std::f32::consts::TAU;
                let distance = random.next_float().sqrt() * spread;
                let pos = DVec3::new(at.x + f64::from(cos(f64::from(angle)) * distance), at.y, at.z + f64::from(sin(f64::from(angle)) * distance));
                let white = waiting && random.next_bool();
                let argb = if white { [1.0; 4] } else { [1.0, rgb[0], rgb[1], rgb[2]] };
                self.particles.spawn(&self.world, &Options::Color(Type::EntityEffect, argb), pos, DVec3::ZERO);
            }
        }
    }

    /// `LevelEventHandler.shootParticles`: ten particles out of the face
    /// `data` names (`Direction.from3DDataValue`), spread across it.
    fn shoot_particles(&mut self, pos: BlockPos, data: i32, particle: crate::particles::Type) {
        const STEPS: [(f64, f64, f64); 6] = [
            (0.0, -1.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 0.0, -1.0),
            (0.0, 0.0, 1.0),
            (-1.0, 0.0, 0.0),
            (1.0, 0.0, 0.0),
        ];
        let (nx, ny, nz) = STEPS[(data % 6).unsigned_abs() as usize];
        let (x, y, z) = (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
        for _ in 0..10 {
            let random = &mut self.particles.random;
            let pow = random.next_double() * 0.2 + 0.01;
            let px = x + nx * 0.6 + 0.5 + nx * 0.01 + (random.next_double() - 0.5) * nz * 0.5;
            let py = y + ny * 0.6 + 0.5 + ny * 0.01 + (random.next_double() - 0.5) * ny * 0.5;
            let pz = z + nz * 0.6 + 0.5 + nz * 0.01 + (random.next_double() - 0.5) * nx * 0.5;
            let vx = nx * pow + random.next_gaussian() * 0.01;
            let vy = ny * pow + random.next_gaussian() * 0.01;
            let vz = nz * pow + random.next_gaussian() * 0.01;
            self.particles.spawn(
                &self.world,
                &particle.into(),
                DVec3::new(px, py, pz),
                DVec3::new(vx, vy, vz),
            );
        }
    }

    fn keys(&mut self, input: &mut Input, gui: &Gui) {
        for key in std::mem::take(&mut input.keys) {
            if self.screen == Screen::Creative && self.creative_key(key, gui, input.ctrl) {
                continue;
            }
            if self.screen == Screen::Menu && self.menu_key(key, gui, input.ctrl) {
                continue;
            }
            match (key, self.screen) {
                (Key::Escape, Screen::Playing) => self.pause(),
                (Key::Escape, Screen::Paused) => self.screen = Screen::Playing,
                (
                    Key::Escape | Key::Inventory,
                    Screen::Inventory | Screen::Crafting | Screen::Creative,
                ) => self.close_container(),
                // Not while the server has the inventory's copy.
                (Key::Inventory, Screen::Playing)
                    if self.alive() && self.landed && !self.inventory_busy() =>
                {
                    if self.creative {
                        self.open_creative(gui);
                        self.screen = Screen::Creative;
                    } else {
                        self.screen = Screen::Inventory;
                    }
                }
                (Key::Hotbar(slot), Screen::Playing) => {
                    if self.inventory_busy() || !self.hotbar_keys(slot, input) {
                        self.entities.selected = slot;
                    }
                }
                // `SWAP_ITEM_WITH_OFFHAND`: the hands trade stacks.
                (Key::SwapOffhand, Screen::Playing) if self.alive() && !self.inventory_busy() => {
                    self.entities
                        .inventory
                        .slots
                        .swap(self.entities.selected, 40);
                    self.eating = None;
                }
                (Key::Hotbar(slot), Screen::Inventory | Screen::Crafting) => {
                    // `ArmorSlot.mayPickup`: binding holds a worn piece.
                    if let (Some(Slot::Inventory(index)), _) =
                        gui.slot_at(self.screen == Screen::Crafting)
                        && !self.entities.inventory.bound(index)
                    {
                        self.entities.inventory.number_swap(index, slot);
                    }
                }
                // `THROW` over a stack: one of it, or all with control.
                (Key::Drop, Screen::Inventory | Screen::Crafting) => {
                    if let (Some(Slot::Inventory(index)), _) =
                        gui.slot_at(self.screen == Screen::Crafting)
                        && self.entities.inventory.cursor.is_none()
                        && !self.entities.inventory.bound(index)
                    {
                        let dropped = self.entities.inventory.drop_selected(index, input.ctrl);
                        self.throw(dropped.into_iter().collect());
                    }
                }
                (Key::Drop, Screen::Playing) if self.alive() && !self.inventory_busy() => {
                    let selected = self.entities.selected;
                    if let Some(stack) = self.entities.inventory.drop_selected(selected, input.ctrl)
                    {
                        self.throw(vec![stack]);
                        self.start_swing();
                    }
                }
                (Key::Debug, _) => self.debug = !self.debug,
                (Key::HideHud, _) => self.hide_hud = !self.hide_hud,
                (Key::Screenshot, _) => self.screenshot = true,
                (Key::Forward, Screen::Playing) => {
                    if self
                        .forward_tapped
                        .is_some_and(|tick| self.ticks - tick <= 7)
                    {
                        self.tap_sprint = true;
                    }
                    self.forward_tapped = Some(self.ticks);
                }
                _ => {}
            }
        }
    }

    /// Clicks on the open screen's slots and buttons, with vanilla's
    /// container rules: shift moves a stack across, a double click gathers,
    /// a drag shares the carried stack out.
    fn screen_input(&mut self, input: &mut Input, gui: &Gui) {
        match self.screen {
            Screen::Playing => {}
            Screen::Paused | Screen::Dead => {
                for (_, pressed) in input.clicks.drain(..) {
                    if !pressed || self.screen == Screen::Dead && !self.death_buttons_ready() {
                        continue;
                    }
                    match gui.button_at(self.screen) {
                        Some(Button::Resume) => self.screen = Screen::Playing,
                        Some(Button::Respawn) => self.respawn(),
                        Some(Button::SaveAndQuit) => self.quit = true,
                        None => {}
                    }
                }
            }
            Screen::Creative => self.creative_input(input, gui),
            Screen::Menu => self.menu_input(input, gui),
            Screen::Inventory | Screen::Crafting => {
                let workbench = self.screen == Screen::Crafting;
                let (slot, outside) = gui.slot_at(workbench);
                // `AbstractContainerMenu`'s quick craft: a slot joins the drag
                // if it can take the carried item and there are items left
                // for it.
                if let Some((_, slots)) = self.drag.as_mut()
                    && let Some(slot) = slot
                    && !slots.contains(&slot)
                    && let Some(carried) = self.entities.inventory.cursor.as_ref()
                    && slots.len() < usize::from(carried.count)
                    && drag_accepts(&self.entities.inventory, slot, carried)
                {
                    slots.push(slot);
                }
                for (right, pressed) in input.clicks.drain(..).collect::<Vec<_>>() {
                    if pressed {
                        self.press_slot(slot, outside, right, input.shift);
                    } else if let Some((drag_right, slots)) = self.drag.take()
                        && drag_right == right
                    {
                        self.release_drag(slots, right, workbench);
                    }
                }
            }
        }
    }

    fn press_slot(&mut self, slot: Option<Slot>, outside: bool, right: bool, shift: bool) {
        let inventory = &mut self.entities.inventory;
        let Some(slot) = slot else {
            if outside {
                let thrown = inventory.click(None, right, false);
                self.throw(thrown.into_iter().collect());
            }
            return;
        };
        // A double click gathers into the cursor, from an empty slot (one
        // the first click picked up), never from a result slot.
        let double = self.last_click.is_some_and(|(last, last_right, at)| {
            last == slot && last_right == right && at.elapsed().as_millis() < 250
        }) && !right
            && !shift;
        self.last_click = Some((slot, right, Instant::now()));
        let empty = match slot {
            Slot::Inventory(index) => inventory.slots.get(index).is_some_and(Option::is_none),
            Slot::Crafting(index) => inventory.crafting.get(index).is_some_and(Option::is_none),
            Slot::Workbench(index) => inventory.workbench.get(index).is_some_and(Option::is_none),
            _ => false,
        };
        if double && empty && inventory.cursor.is_some() {
            inventory.pickup_all(right);
            return;
        }
        if inventory.cursor.is_some()
            && !shift
            && !matches!(slot, Slot::CraftingResult | Slot::WorkbenchResult)
        {
            // The press may start a drag; a single slot is a plain click.
            self.drag = Some((right, vec![slot]));
            return;
        }
        match slot {
            // The crafting table's own shift-click: into its grid.
            Slot::Inventory(index) if shift && self.screen == Screen::Crafting => {
                inventory.quick_move_to_workbench(index)
            }
            Slot::Inventory(index) => {
                let thrown = inventory.click(Some(index), right, shift);
                self.throw(thrown.into_iter().collect());
            }
            Slot::Crafting(index) => inventory.click_crafting_slot(index, right, shift),
            Slot::Workbench(index) => inventory.click_workbench_slot(index, right, shift),
            Slot::CraftingResult => {
                inventory.take_crafting_output(shift);
            }
            Slot::WorkbenchResult => {
                inventory.take_workbench_output(shift);
            }
            Slot::Creative(_) | Slot::Destroy => {}
        }
    }

    fn release_drag(&mut self, slots: Vec<Slot>, right: bool, workbench: bool) {
        let inventory = &mut self.entities.inventory;
        if let [slot] = slots[..] {
            match slot {
                Slot::Inventory(index) => {
                    let thrown = inventory.click(Some(index), right, false);
                    self.throw(thrown.into_iter().collect());
                }
                Slot::Crafting(index) => inventory.click_crafting_slot(index, right, false),
                Slot::Workbench(index) => inventory.click_workbench_slot(index, right, false),
                _ => {}
            }
            return;
        }
        // Menu quick-craft indices: player slots, then the grid's.
        let indices: Vec<usize> = slots
            .iter()
            .filter_map(|slot| match *slot {
                Slot::Inventory(index) => Some(index),
                Slot::Crafting(index) | Slot::Workbench(index) => Some(43 + index),
                _ => None,
            })
            .collect();
        inventory.distribute_crafting(&indices, right, workbench);
    }

    fn close_container(&mut self) {
        let inventory = &mut self.entities.inventory;
        let mut thrown = inventory.settle_crafting();
        thrown.extend(inventory.settle_workbench());
        thrown.extend(inventory.settle_cursor());
        thrown.extend(inventory.take_pending_drops());
        self.throw(thrown);
        self.drag = None;
        self.screen = Screen::Playing;
    }

    fn throw(&mut self, stacks: Vec<ItemStack>) {
        if stacks.is_empty() {
            return;
        }
        let eye = self.player.eye();
        self.entities.throw(
            stacks,
            eye,
            self.player.yaw as f32,
            self.player.pitch as f32,
        );
    }

    /// The middle button: the targeted block's item into the hand.
    fn pick_block(&mut self) {
        let Some(hit) = crate::target::target(
            &self.world,
            self.player.eye(),
            self.player.look(),
            self.block_reach(),
        ) else {
            return;
        };
        let Some(block) = self.world.block(hit.pos) else {
            return;
        };
        let Some(id) = block_item(block) else {
            return;
        };
        let inventory = &mut self.entities.inventory;
        if inventory
            .recipes
            .item_catalog()
            .is_some_and(|catalog| catalog.get(&id).is_none())
        {
            return;
        }
        let selected = self.entities.selected;
        // `Inventory.getSuitableHotbarSlot`: the first empty hotbar slot
        // from the selected one on, else the selected one.
        let suitable = (0..9)
            .map(|i| (selected + i) % 9)
            .find(|&i| inventory.slots[i].is_none())
            .unwrap_or(selected);
        if let Some(slot) =
            (0..9).find(|&i| inventory.slots[i].as_ref().is_some_and(|s| s.id == id))
        {
            self.entities.selected = slot;
        } else if let Some(slot) =
            (9..36).find(|&i| inventory.slots[i].as_ref().is_some_and(|s| s.id == id))
        {
            // `pickSlot`: swapped into the hotbar.
            inventory.slots.swap(slot, suitable);
            self.entities.selected = suitable;
        } else if self.creative {
            // `addAndPickItem`: what the slot held moves to a free slot.
            if let Some(displaced) = inventory.slots[suitable].take()
                && let Some(free) = (0..36).find(|&i| i != suitable && inventory.slots[i].is_none())
            {
                inventory.slots[free] = Some(displaced);
            }
            let stack = self.entities.stack(&id, 1);
            self.entities.inventory.slots[suitable] = Some(stack);
            self.entities.selected = suitable;
        }
    }

    /// One 20 Hz tick.
    fn tick(&mut self, input: &Input) {
        self.ticks += 1;
        if self.screen == Screen::Dead && self.ticks == self.died_at + 20 {
            self.death_effects();
        }
        // `MinecraftServer.autoSave`: every five minutes.
        if self.ticks.is_multiple_of(6000) && self.save_dir.is_some() {
            self.save_chunks();
            self.save();
        }
        self.previous = self.player.pos;
        self.previous_eye_height = self.eye_height;
        self.previous_equip = self.equip;
        self.walk.previous_dist = self.walk.dist;
        self.walk.previous_bob = self.walk.bob;
        self.previous_fov = self.fov;
        let held_id = self.entities.held().map(|s| s.id.clone());
        if held_id == self.equipped {
            self.equip = (self.equip + 0.4).min(1.0);
        } else {
            self.equip = (self.equip - 0.4).max(0.0);
            if self.equip <= 0.0 {
                self.equipped = held_id;
            }
        }
        let feet = self.feet_block();
        if !self.landed
            && self.world.chunk_ready(feet)
            && self.world.chunk_ready((feet.0, feet.1 - 1, feet.2))
        {
            self.landed = true;
        }
        // Hands wait while the server has the inventory's copy.
        let playing =
            self.screen == Screen::Playing && self.alive() && self.landed && !self.inventory_busy();
        if playing {
            self.tick_hands(input);
        } else {
            self.attack_presses = 0;
            self.use_presses = 0;
            self.mining.reset();
            self.eating = None;
            self.using = None;
        }
        self.swing.tick(self.swing_duration());
        // `LocalPlayer.tickSpinningEffect`: nausea turns the warp at seven
        // degrees a tick, as strong as it has faded in.
        let nausea = self.player.survival.effect(MobEffect::Nausea).map_or(0.0, |e| e.blend_factor(1.0));
        self.spin.1 = if nausea > 0.0 { 7.0 } else { 0.0 };
        self.spin.0 += self.spin.1;
        if let Some((_, left)) = self.overlay.as_mut() {
            *left = left.saturating_sub(1);
        }
        if self.overlay.as_ref().is_some_and(|(_, left)| *left == 0) {
            self.overlay = None;
        }
        if self.landed && self.alive() && self.world.chunk_ready(feet) {
            self.tick_movement(if self.screen == Screen::Playing {
                Some(input)
            } else {
                None
            });
        }
        self.entities.tick_scene(&self.world.scene);
        self.tick_menu();
        // `ClientLevel.tick`'s explosion block particles, then
        // `ClientLevel.animateTick`, then `ParticleEngine.tick`.
        self.particles.tick_explosions(&self.world);
        let mut sounds = Vec::new();
        // `ClientLevel.tick`'s weather, then the bolts' own ticks.
        if let Some(weather) = &self.entities.weather {
            self.weather
                .set_levels(weather.rain_level, weather.thunder_level);
        }
        self.weather.tick(
            &self.world,
            &mut self.particles,
            self.player.eye(),
            self.ticks as i64,
            self.entities.lightning(),
            &mut sounds,
        );
        self.world.rain_level = self.weather.rain_level;
        self.world.thunder_level = self.weather.thunder();
        self.world.sky_flash = self.weather.flashing();
        if self.landed {
            let eye = self.player.eye().floor();
            let around = (eye.x as i32, eye.y as i32, eye.z as i32);
            self.ambient
                .tick(&self.world, &mut self.particles, around, &mut sounds);
        }
        self.cloud_particles();
        if self.alive() {
            self.effect_particles();
        }
        self.particles.player = Some((self.player.pos, self.player.velocity.y));
        let entities = &self.entities;
        self.emitters
            .tick(&mut self.particles, &self.world, |id| entities.mob_bounds(id));
        self.particles.tick(&self.world);
        sounds.append(&mut self.particles.sounds);
        for (event, at, volume, pitch) in sounds {
            // A sound played to the player has no position.
            let at = (!at.x.is_nan()).then_some(at);
            self.play(event, at, volume, pitch);
        }
        if let Some((_, since)) = self.highlight.as_mut() {
            *since += TICK_SECONDS;
        }
        let selected = self.entities.selected;
        let shown = (
            selected,
            self.entities.held().map(|s| ItemStack {
                count: 1,
                ..s.clone()
            }),
        );
        if self.highlight_slot.as_ref() != Some(&shown) {
            self.highlight = shown.1.as_ref().map(|stack| (stack.clone(), 0.0));
            self.highlight_slot = Some(shown);
        }
    }

    fn tick_hands(&mut self, input: &Input) {
        if self.using.is_some() {
            // `Minecraft.handleKeybinds`: while an item is in use, its
            // release is all the buttons do.
            self.attack_presses = 0;
            self.use_presses = 0;
            self.mining.reset();
            self.tick_using(input.use_item);
            return;
        }
        let held = self.entities.held().cloned();
        let eye = self.player.eye();
        let look = self.player.look();
        let target = crate::target::target(
            &self.world,
            self.player.eye(),
            self.player.look(),
            self.block_reach(),
        );
        let block_distance = target.as_ref().map_or(f64::INFINITY, |hit| hit.distance);
        let mob_distance = self
            .entities
            .mob_on_ray(eye, look, self.entity_reach())
            .filter(|&d| d < block_distance);
        // Attacks: a click hits the mob in reach, or starts on a block.
        let attacked = std::mem::take(&mut self.attack_presses) > 0 && self.eating.is_none();
        if attacked {
            self.start_swing();
            if mob_distance.is_some() {
                self.attack(held.as_ref(), eye, look);
            } else if let Some(hit) = target.as_ref() {
                self.entities.attack_block(&self.world.scene, hit.pos);
            }
        }
        // A click too quick for the button to be down at the tick still
        // starts on the block, which breaks one that breaks instantly.
        if (input.attack || attacked) && mob_distance.is_none() && self.eating.is_none() {
            let on_ground = self.player.on_ground || self.player.flying;
            let water = self.eyes_in_water();
            let modifiers = self.enchantment_modifiers();
            let swing = self.mining.tick(
                &self.world.registries,
                &self.world.scene,
                target.as_ref().map(|h| h.pos),
                held.as_ref(),
                on_ground,
                water,
                self.creative,
                attacked,
                &self.player.survival.effects,
                &modifiers,
            );
            if target.is_some() {
                self.start_swing();
            }
            if let Some((pos, block)) = swing.hit
                && let Some(kind) = self.world.scene.sound_type(&block)
            {
                self.play(
                    &kind.hit,
                    Some(center(pos)),
                    (kind.volume + 1.0) / 8.0,
                    kind.pitch * 0.5,
                );
            }
            if let Some(block) = swing.cracked
                && let Some(hit) = target.as_ref()
            {
                self.particles
                    .crack(&self.world, hit.pos, hit.face.offset(), &block);
            }
            if let Some(broken) = swing.broken {
                self.broke(broken);
            }
        } else {
            self.mining.reset();
        }
        // Using the held item: on a click, then every four ticks held.
        self.use_delay = self.use_delay.saturating_sub(1);
        let pressed = std::mem::take(&mut self.use_presses) > 0;
        if let Some(eating) = self.eating.as_mut() {
            if !input.use_item {
                self.eating = None;
            } else {
                let mode = if self.creative {
                    GameMode::Creative
                } else {
                    GameMode::Survival
                };
                let food = &mut self.player.survival.food;
                match eating.tick(&mut self.entities.inventory, food, mode) {
                    FoodUseTick::Continuing { emit_sound } => {
                        if emit_sound {
                            let sound = format!("minecraft:{}", eating.info.sound);
                            let crumbs = eating.info.particles.then(|| eating.stack.id.clone());
                            if let Some(id) = crumbs {
                                self.item_crumbs(&id, 5);
                            }
                            let pitch = (self.sounds.random() - self.sounds.random()) * 0.2 + 1.0;
                            let volume = 0.5 + 0.5 * self.sounds.random();
                            self.play(&sound, None, volume, pitch);
                        }
                    }
                    FoodUseTick::Finished { overflow } => {
                        let stack = eating.stack.clone();
                        let id = stack.id.clone();
                        let crumbs = eating.info.particles;
                        self.eating = None;
                        if crumbs {
                            self.item_crumbs(&id, 16);
                        }
                        let sounds = &mut self.sounds;
                        let teleport = minecraftoss_player::food::apply_consumed_effects(
                            &stack,
                            &mut self.player.survival,
                            self.creative,
                            || sounds.random(),
                        );
                        if teleport {
                            self.teleport_randomly();
                        }
                        self.throw(overflow.into_iter().collect());
                        // `FoodProperties.onConsume`'s burp, for food only;
                        // an ominous bottle's sound after it is drunk.
                        if minecraftoss_player::food::catalog().contains_key(&id) {
                            let pitch = self.sounds.random() * 0.1 + 0.9;
                            self.play("minecraft:entity.player.burp", None, 0.5, pitch);
                        } else if id == "minecraft:ominous_bottle" {
                            self.play("minecraft:item.ominous_bottle.dispose", None, 1.0, 1.0);
                        }
                    }
                    FoodUseTick::Cancelled => self.eating = None,
                }
            }
        } else if pressed || (input.use_item && self.use_delay == 0) {
            self.use_delay = 4;
            self.use_item(
                held.as_ref(),
                target,
                mob_distance.is_some(),
                pressed,
                input.sneak,
            );
        }
    }

    /// Chorus fruit's `TeleportRandomlyConsumeEffect`: up to sixteen tries
    /// within eight blocks each way.
    fn teleport_randomly(&mut self) {
        for _ in 0..16 {
            let mut offset = || (f64::from(self.sounds.random()) - 0.5) * 16.0;
            let (dx, dy, dz) = (offset(), offset(), offset());
            let p = self.player.pos;
            let target = DVec3::new(p.x + dx, (p.y + dy).clamp(-64.0, 319.0), p.z + dz);
            if self
                .player
                .try_random_teleport_target(&self.world.scene, target)
            {
                self.previous = self.player.pos;
                self.player.velocity = DVec3::ZERO;
                self.play("minecraft:item.chorus_fruit.teleport", None, 1.0, 1.0);
                return;
            }
        }
    }

    /// The attribute modifiers the worn and held items' enchantments give
    /// (`EnchantmentHelper.forEachModifier` as equipment changes).
    fn enchantment_modifiers(&self) -> Vec<minecraftoss_player::item_catalog::AttributeModifier> {
        let inventory = &self.entities.inventory;
        inventory.recipes.enchantments().equipment_modifiers(&inventory.equipment(self.entities.selected))
    }

    fn attack(&mut self, held: Option<&ItemStack>, eye: DVec3, look: DVec3) {
        let catalog = self.entities.inventory.recipes.item_catalog();
        let modifiers: Vec<_> = held
            .and_then(|stack| catalog.and_then(|c| c.get(&stack.id)))
            .map(|item| {
                item.attribute_modifiers
                    .iter()
                    .filter(|m| m.in_main_hand())
                    .cloned()
                    .collect()
            })
            .unwrap_or_else(Vec::new);
        // The item's modifiers, then the effects': strength and weakness
        // add to the damage, haste and mining fatigue scale the speed.
        let effects = &self.player.survival.effects;
        let damage = minecraftoss_player::item_catalog::attribute_value(
            1.0,
            modifiers
                .iter()
                .filter(|m| m.attribute == "minecraft:attack_damage"),
            (f64::MIN, f64::MAX),
        );
        let damage = effects.attribute(Attribute::AttackDamage, damage, &[]).clamp(0.0, 2048.0);
        let speed = minecraftoss_player::item_catalog::attribute_value(
            4.0,
            modifiers
                .iter()
                .filter(|m| m.attribute == "minecraft:attack_speed"),
            (f64::MIN, f64::MAX),
        );
        let speed = effects.attribute(Attribute::AttackSpeed, speed, &[]).clamp(0.0, 1024.0);
        let strength = self.player.attack_strength_scale(0.5, speed);
        let attack = PlayerAttack {
            player_id: 0,
            position: self.player.pos,
            yaw: self.player.yaw as f32,
            attack_damage: damage,
            strength,
            sprinting: self.player.sprinting,
            // `canCriticalAttack`: blindness (`isMobilityRestricted`)
            // spoils it too.
            can_critical: !self.player.on_ground
                && self.player.velocity.y < 0.0
                && !self.player.in_water
                && !self.player.sprinting
                && !self.player.survival.has_effect(MobEffect::Blindness),
            can_sweep: self.player.on_ground && held.is_some_and(|s| s.id.ends_with("_sword")),
            weapon: held.cloned(),
            fall_distance: f64::from(self.player.fall_distance),
            on_ground: self.player.on_ground,
            flying: self.player.flying,
            fall_flying: false,
            game_mode: if self.creative { "creative" } else { "survival" },
            food_level: i32::from(self.player.survival.food.level),
        };
        if self.entities.attack(eye, look, self.entity_reach(), attack) {
            self.player.attack_strength_ticker = 0;
            self.player.survival.food.add_exhaustion(0.1);
            if !self.creative
                && held.is_some_and(|stack| {
                    self.entities.inventory.recipes.durability(stack).is_some()
                })
            {
                let selected = self.entities.selected;
                if self.entities.inventory.wear_tool(selected, 1)
                    && let Some(stack) = held
                {
                    self.item_broke(&stack.id);
                }
            }
        }
    }

    fn use_item(
        &mut self,
        held: Option<&ItemStack>,
        target: Option<minecraftoss_player::Hit>,
        at_mob: bool,
        pressed: bool,
        sneaking: bool,
    ) {
        let eye = self.player.eye();
        let look = self.player.look();
        if at_mob {
            // The server answers whether the mob took it (see `frame`).
            self.entities
                .interact(eye, look, self.entity_reach(), self.creative);
            return;
        }
        // `performUseItemOn`: sneaking (`isSecondaryUseActive`) with
        // something in either hand uses the item, not the block.
        let hands = held.is_some() || self.entities.inventory.slots[40].is_some();
        if let Some(hit) = target.as_ref()
            && !(sneaking && hands)
            && let Some(block) = self.world.block(hit.pos).cloned()
        {
            if block.id.path == "crafting_table" {
                if pressed {
                    self.screen = Screen::Crafting;
                }
                return;
            }
            // `TntBlock.useItemOn`: flint and steel (worn by one) or a fire
            // charge (spent) primes the TNT, which the server turns into
            // an entity. The `tnt_explodes` rule that would refuse it
            // always holds here, so the item is spent at once.
            if block.id.path == "tnt"
                && let Some(stack) = held
                && matches!(
                    stack.id.as_str(),
                    "minecraft:flint_and_steel" | "minecraft:fire_charge"
                )
            {
                self.entities.ignite_tnt(hit.pos);
                if !self.creative {
                    let selected = self.entities.selected;
                    if stack.id == "minecraft:fire_charge" {
                        if let Some(held) = self.entities.inventory.slots[selected].as_mut() {
                            held.count -= 1;
                            if held.count == 0 {
                                self.entities.inventory.slots[selected] = None;
                            }
                        }
                    } else if self.entities.inventory.wear_tool(selected, 1) {
                        self.item_broke(&stack.id);
                    }
                }
                self.start_swing();
                return;
            }
            let (facing, _) = horizontal_facing(self.player.yaw);
            // A block with a menu: the server opens it, or says why not
            // (`useWithoutItem`'s `SUCCESS` on the client swings the arm,
            // and nothing is placed). Uses wait for its answer.
            if self.entities.opens_menu(&self.world.scene, hit.pos) {
                let player = self.player_context();
                self.entities.use_block_with(hit.pos, facing, player);
                self.use_pending = Some(hit.pos);
                self.start_swing();
                return;
            }
            if self.entities.use_block(&self.world.scene, hit.pos, facing) {
                // `LeverBlock.useWithoutItem` on the client: a speck as it
                // turns on.
                if block.id.path == "lever"
                    && block
                        .properties
                        .get("powered")
                        .is_some_and(|p| p == "false")
                {
                    let mut on = block.clone();
                    on.properties.insert("powered".into(), "true".into());
                    crate::ambient::lever_particle_at(
                        &mut self.particles,
                        &self.world,
                        hit.pos,
                        &on,
                        1.0,
                    );
                }
                self.start_swing();
                return;
            }
        }
        // `startUseItem` goes on to the offhand when the main hand has
        // nothing to use.
        let Some(stack) = held else {
            self.start_using(ranged::OFFHAND);
            return;
        };
        if stack.id == "minecraft:bucket" {
            if let Some(hit) = self
                .player
                .target_source_fluid(&self.world.scene, self.block_reach())
                && let Some(fluid) = self
                    .world
                    .block(hit.pos)
                    .filter(|b| matches!(b.id.path.as_str(), "water" | "lava"))
                    .cloned()
            {
                self.world.set_blocks(&[(hit.pos, None)]);
                self.entities.edited(&self.world.scene, hit.pos, false);
                let filled = self
                    .entities
                    .stack(&format!("minecraft:{}_bucket", fluid.id.path), 1);
                self.replace_held(filled);
                self.play(
                    &format!(
                        "minecraft:item.bucket.fill{}",
                        if fluid.id.path == "lava" { "_lava" } else { "" }
                    ),
                    Some(center(hit.pos)),
                    1.0,
                    1.0,
                );
                self.start_swing();
            }
            return;
        }
        if let Some(kind) = stack.id.strip_suffix("_spawn_egg")
            && let Some(hit) = target.as_ref()
            && let Some(kind) = kind.strip_prefix("minecraft:").or(Some(kind))
        {
            let (dx, dy, dz) = hit.face.offset();
            let at = [
                f64::from(hit.pos.0 + dx) + 0.5,
                f64::from(hit.pos.1 + dy),
                f64::from(hit.pos.2 + dz) + 0.5,
            ];
            self.entities.summon(kind, at);
            self.start_swing();
            if !self.creative {
                let selected = self.entities.selected;
                if let Some(held) = self.entities.inventory.slots[selected].as_mut() {
                    held.count -= 1;
                    if held.count == 0 {
                        self.entities.inventory.slots[selected] = None;
                    }
                }
            }
            return;
        }
        if matches!(
            stack.id.as_str(),
            "minecraft:flint_and_steel" | "minecraft:fire_charge"
        ) && let Some(hit) = target.as_ref()
        {
            // `FlintAndSteelItem.useOn`, `FireChargeItem.useOn`: the server
            // lights the block or sets fire on the face; the sound, the
            // swing and the wear follow when it did (`ignited`).
            let flint_and_steel = stack.id == "minecraft:flint_and_steel";
            self.entities
                .use_igniter(hit.pos, face_name(hit.face), flint_and_steel);
            return;
        }
        if stack.id == "minecraft:bone_meal"
            && let Some(hit) = target.as_ref()
        {
            // `BoneMealItem.useOn`: the level grows the block; the item and
            // sparkles follow when it took (`bone_meal_used`).
            self.entities.bone_meal(hit.pos, face_name(hit.face));
            self.start_swing();
            if !self.creative {
                let selected = self.entities.selected;
                if let Some(held) = self.entities.inventory.slots[selected].as_mut() {
                    held.count -= 1;
                    if held.count == 0 {
                        self.entities.inventory.slots[selected] = None;
                    }
                }
            }
            return;
        }
        if let Some(hit) = target.as_ref()
            && let Some((block, sound)) =
                crate::placement::tool_use(&self.world, &stack.id, hit.pos, hit.face)
        {
            self.world.set_blocks(&[(hit.pos, Some(block))]);
            self.entities.edited(&self.world.scene, hit.pos, true);
            self.play(sound, Some(center(hit.pos)), 1.0, 1.0);
            self.start_swing();
            if !self.creative {
                let selected = self.entities.selected;
                if self.entities.inventory.wear_tool(selected, 1) {
                    self.item_broke(&stack.id);
                }
            }
            return;
        }
        if target.is_some() && self.place(stack) {
            return;
        }
        if matches!(stack.id.as_str(), "minecraft:splash_potion" | "minecraft:lingering_potion") {
            self.throw_potion(stack);
            return;
        }
        if self.start_using(self.entities.selected) {
            return;
        }
        let food = &self.player.survival.food;
        if let Some(eating) = FoodUse::start(self.entities.selected, stack, food, self.mode()) {
            self.eating = Some(eating);
            return;
        }
        self.start_using(ranged::OFFHAND);
    }

    /// `ThrowablePotionItem.use`: the throw's sound, the potion in flight
    /// (the server's), one spent, and the arm swings.
    fn throw_potion(&mut self, stack: &ItemStack) {
        let lingering = stack.id == "minecraft:lingering_potion";
        let sound = if lingering {
            "minecraft:entity.lingering_potion.throw"
        } else {
            "minecraft:entity.splash_potion.throw"
        };
        let pitch = 0.4 / (self.sounds.random() * 0.4 + 0.8);
        self.play(sound, Some(self.player.pos), 0.5, pitch);
        let contents = minecraftoss_player::effects::PotionContents::from_components(stack.components.as_ref())
            .unwrap_or_else(minecraftoss_player::effects::PotionContents::default);
        // `getKnownMovement`: this tick's motion.
        let motion = self.player.pos - self.previous;
        let motion = if self.player.on_ground { DVec3::new(motion.x, 0.0, motion.z) } else { motion };
        let rotation = (self.player.pitch as f32, self.player.yaw as f32);
        self.entities.throw_potion(contents, lingering, self.player.eye().to_array(), rotation, motion.to_array());
        if !self.creative {
            let selected = self.entities.selected;
            if let Some(held) = self.entities.inventory.slots[selected].as_mut() {
                held.count -= 1;
                if held.count == 0 {
                    self.entities.inventory.slots[selected] = None;
                }
            }
        }
        self.start_swing();
    }

    /// Places the held item as a block; whether it did.
    fn place(&mut self, stack: &ItemStack) -> bool {
        let reach = self.block_reach();
        let Some(hit) =
            crate::target::target(&self.world, self.player.eye(), self.player.look(), reach)
        else {
            return false;
        };
        let Some(placed) = crate::placement::place(&mut self.world, &self.player, &stack.id, &hit)
        else {
            return false;
        };
        if placed.blocks.iter().any(|(pos, block)| {
            crate::placement::blocks_player(&self.world, &self.player, *pos, block)
        }) {
            return false;
        }
        let changes: Vec<(BlockPos, Option<Block>)> = placed
            .blocks
            .iter()
            .map(|(pos, block)| (*pos, Some(block.clone())))
            .collect();
        self.world.set_blocks(&changes);
        for (i, (pos, _)) in placed.blocks.iter().enumerate() {
            // The block the item places takes its components.
            if i == 0 {
                self.entities.placed(&self.world.scene, *pos, stack);
            } else {
                self.entities.edited(&self.world.scene, *pos, true);
            }
        }
        let (pos, block) = &placed.blocks[0];
        if let Some(kind) = self.world.scene.sound_type(block) {
            self.play(
                &kind.place,
                Some(center(*pos)),
                (kind.volume + 1.0) / 2.0,
                kind.pitch * 0.8,
            );
        }
        if !self.creative {
            let selected = self.entities.selected;
            if stack.id.ends_with("_bucket") {
                let bucket = self.entities.stack("minecraft:bucket", 1);
                self.replace_held(bucket);
            } else if let Some(held) = self.entities.inventory.slots[selected].as_mut() {
                held.count -= 1;
                if held.count == 0 {
                    self.entities.inventory.slots[selected] = None;
                }
            }
        }
        self.start_swing();
        true
    }

    fn replace_held(&mut self, stack: ItemStack) {
        let selected = self.entities.selected;
        let slot = &mut self.entities.inventory.slots[selected];
        match slot.as_mut() {
            Some(held) if held.count > 1 && !self.creative => {
                held.count -= 1;
                if let Some(rest) = self.entities.inventory.add_item(stack, selected) {
                    self.throw(vec![rest]);
                }
            }
            _ => *slot = Some(stack),
        }
    }

    fn broke(&mut self, broken: crate::mining::Broken) {
        let pos = broken.pos;
        // Level event 2001.
        self.particles.destroy(&self.world, pos, &broken.block);
        // The other half of a door, bed or tall plant goes with it.
        let mut gone = vec![(pos, None)];
        let props = &broken.block.properties;
        match (
            props.get("half").map(String::as_str),
            props.get("part").map(String::as_str),
        ) {
            (Some("lower"), _) => gone.push(((pos.0, pos.1 + 1, pos.2), None)),
            (Some("upper"), _) => gone.push(((pos.0, pos.1 - 1, pos.2), None)),
            (_, Some(part)) => {
                if let Some((_, (dx, dz))) = HORIZONTAL
                    .iter()
                    .find(|(name, _)| props.get("facing").is_some_and(|f| f == name))
                {
                    let sign = if part == "foot" { 1 } else { -1 };
                    gone.push(((pos.0 + dx * sign, pos.1, pos.2 + dz * sign), None));
                }
            }
            _ => {}
        }
        gone.retain(|(at, _)| {
            *at == pos
                || self
                    .world
                    .block(*at)
                    .is_some_and(|b| b.id == broken.block.id)
        });
        self.world.set_blocks(&gone);
        let selected = self.entities.selected;
        let breaker = minecraft_terrain::server::Breaker {
            tool: self.entities.inventory.slots[selected].clone(),
            harvests: broken.harvests,
            creative: self.creative,
        };
        let mut server_drops = false;
        for (at, _) in &gone {
            if *at == pos {
                server_drops =
                    self.entities
                        .broke(&self.world.scene, pos, &broken.block, breaker.clone());
            } else {
                self.entities.edited(&self.world.scene, *at, false);
            }
        }
        if let Some(kind) = self.world.scene.sound_type(&broken.block) {
            self.play(
                &kind.break_sound,
                Some(center(pos)),
                (kind.volume + 1.0) / 2.0,
                kind.pitch * 0.8,
            );
        }
        if self.creative {
            return;
        }
        if !server_drops {
            self.entities.drop_loot(pos, broken.drops);
        }
        self.player.survival.food.add_exhaustion(0.005);
        let selected = self.entities.selected;
        let tool = self.entities.inventory.slots[selected]
            .as_ref()
            .map(|stack| stack.id.clone());
        if self
            .entities
            .inventory
            .wear_tool_after_mining(selected, broken.hardness)
            && let Some(tool) = tool
        {
            self.item_broke(&tool);
        }
    }

    /// A use of flint and steel or a fire charge that lit something at
    /// `pos`: its sound (`FLINTANDSTEEL_USE` with a pitch of 0.8 to 1.2,
    /// `FIRECHARGE_USE` with a pitch of 0.8 to 1.2 from two draws), the
    /// swing, and the item: flint and steel wears by one, a fire charge is
    /// spent, unless in creative.
    fn ignited(&mut self, pos: BlockPos, flint_and_steel: bool) {
        let (sound, pitch) = if flint_and_steel {
            (
                "minecraft:item.flintandsteel.use",
                self.sounds.random() * 0.4 + 0.8,
            )
        } else {
            let pitch = (self.sounds.random() - self.sounds.random()) * 0.2 + 1.0;
            ("minecraft:item.firecharge.use", pitch)
        };
        self.play(sound, Some(center(pos)), 1.0, pitch);
        self.start_swing();
        let item = if flint_and_steel {
            "minecraft:flint_and_steel"
        } else {
            "minecraft:fire_charge"
        };
        let selected = self.entities.selected;
        if self.creative
            || !self.entities.inventory.slots[selected]
                .as_ref()
                .is_some_and(|held| held.id == item)
        {
            return;
        }
        if flint_and_steel {
            if self.entities.inventory.wear_tool(selected, 1) {
                self.item_broke(item);
            }
        } else if let Some(held) = self.entities.inventory.slots[selected].as_mut() {
            held.count -= 1;
            if held.count == 0 {
                self.entities.inventory.slots[selected] = None;
            }
        }
    }

    /// What a landed hit does to the player who made it: extra knockback
    /// slows it and ends its sprint (`causeExtraKnockback`), a mace's smash
    /// ends its fall (`MaceItem.hurtEnemy`, `postHurtEnemy`), and the
    /// weapon's post-attack effects on it land.
    fn attack_landed(&mut self, result: &minecraftoss_entities::world::AttackResult) {
        if result.extra_knockback {
            self.player.velocity.x *= 0.6;
            self.player.velocity.z *= 0.6;
            self.player.sprinting = false;
        }
        if result.smash.is_some() {
            self.player.velocity.y = f64::from(0.01_f32);
            self.player.fall_distance = 0.0;
        }
        for effect in &result.attacker_effects {
            self.enchantment_effect_on_player(effect, minecraftoss_player::enchantment::Slot::MainHand);
        }
    }

    /// An enchantment's entity effect on the player, from the item in
    /// `slot`: its wear (`ChangeItemDamage`), status effects, fire, food
    /// exhaustion, damage and sounds.
    fn enchantment_effect_on_player(&mut self, effect: &minecraftoss_player::enchantment::EntityEffect, slot: minecraftoss_player::enchantment::Slot) {
        use minecraftoss_player::enchantment::EntityEffect;
        match effect {
            EntityEffect::ItemDamage { amount } if !self.creative => {
                let index = minecraftoss_player::inventory::Inventory::equipment_index(slot, self.entities.selected);
                let id = self.entities.inventory.slots.get(index).and_then(Option::as_ref).map(|s| s.id.clone());
                if let (Ok(amount), Some(id)) = (u32::try_from(*amount), id)
                    && self.entities.inventory.wear_tool(index, amount)
                {
                    self.item_broke(&id);
                }
            }
            EntityEffect::MobEffect { effect, duration, amplifier } => {
                if let Some(kind) = MobEffect::from_id(effect) {
                    self.player.survival.add_effect_instance(EffectInstance::new(kind, *duration, *amplifier));
                }
            }
            EntityEffect::Ignite { seconds } => self.player.survival.ignite_for_ticks((seconds * 20.0).floor() as i32),
            // A mob's thorns striking back at the player's hit (`thorns`,
            // through armour, scaled for a living non-player cause; the
            // push from the mob is not given).
            EntityEffect::Damage { damage_type, amount } if !self.creative && self.alive() => {
                let inventory = &self.entities.inventory;
                let (toughness, resistance) = inventory.armor_toughness();
                let value = Some((f32::from(inventory.armor_value()), toughness as f32));
                let me = self.player.facts(&self.world.scene, self.player.velocity);
                let source = minecraftoss_player::enchantment::DamageFacts { damage_type, attacker: None, direct: None, is_direct: true };
                let protection = self.player.equipped.protection(&me, &source);
                let incoming = IncomingHit { damage: *amount, from: HitFrom::Explosion, scales_with_difficulty: true, exhaustion: 0.1 };
                if self.player.hurt_by(&incoming, Difficulty::Normal, Armor { value, protection }, resistance) {
                    self.play("minecraft:enchant.thorns.hit", None, 1.0, 1.0);
                }
            }
            EntityEffect::Exhaustion { amount } => self.player.survival.food.add_exhaustion(*amount),
            EntityEffect::Sound { event, position, volume, pitch } => self.play(event, Some(*position), *volume, *pitch),
            // Wind burst's blast where the player stands (`ServerExplosion`
            // with no damage): straight up from its centre by the knockback
            // multiplier, less `EXPLOSION_KNOCKBACK_RESISTANCE`, and the
            // impulse spares the fall (`currentImpulseImpactPos`). The gust
            // particles are not drawn, nor are mobs nearby pushed.
            EntityEffect::Explode { position, radius, knockback_multiplier, definition, .. } => {
                let distance = self.player.pos.distance(*position) / f64::from(radius * 2.0);
                if *radius >= 1.0e-5 && distance <= 1.0 && !(self.creative && self.player.flying) {
                    let resistance = self.player.equipped.attribute("minecraft:explosion_knockback_resistance", 0.0, (0.0, 1.0));
                    let direction = (self.player.eye() - *position).normalize_or_zero();
                    let power = (1.0 - distance) * f64::from(knockback_multiplier.unwrap_or(1.0)) * (1.0 - resistance);
                    self.player.velocity += direction * power;
                    self.player.fall_distance = 0.0;
                }
                if let Some(sound) = definition["sound"].as_str().or_else(|| definition["sound"]["sound_id"].as_str()) {
                    let pitch = (1.0 + (self.sounds.random() - self.sounds.random()) * 0.2) * 0.7;
                    self.play(sound, Some(*position), 4.0, pitch);
                }
            }
            _ => {}
        }
    }

    /// What the equipment's enchantments did this tick (`tickEffects` and
    /// `runLocationChangedEffects`): soul speed's souls, sounds and wear,
    /// frost walker's ice.
    fn enchantment_effects(&mut self) {
        use minecraftoss_player::enchantment::EntityEffect;
        for (slot, effect) in std::mem::take(&mut self.player.equipped.pending) {
            match &effect {
                EntityEffect::Particle { particle, position, velocity } => {
                    // The only particle the vanilla enchantments send.
                    if particle["type"] == "minecraft:soul" {
                        let kind = crate::particles::Type::Soul.into();
                        crate::emitters::send(&mut self.particles, &self.world, &kind, *position, 0, *velocity, 1.0);
                    }
                }
                EntityEffect::ReplaceDisk { position, offset, radius, height, predicate, block, .. } => {
                    self.replace_disk(*position, *offset, *radius, *height, predicate.as_ref(), block);
                }
                _ => self.enchantment_effect_on_player(&effect, slot),
            }
        }
    }

    /// `ReplaceDisk.apply` (frost walker): each block of the disk the
    /// predicate accepts becomes the state, in the client's scene and the
    /// server's level.
    fn replace_disk(&mut self, position: DVec3, offset: [i32; 3], radius: i32, height: i32, predicate: Option<&serde_json::Value>, block: &serde_json::Value) {
        let Some(id) = block["id"].as_str().or_else(|| block["state"]["Name"].as_str()) else { return };
        let mut state = Block::new(id);
        for (key, value) in block["properties"].as_object().into_iter().flatten() {
            if let Some(value) = value.as_str() {
                state = state.with(key, value);
            }
        }
        let effects = self.entities.inventory.recipes.enchantments().clone();
        let scene = &self.world.scene;
        let at = |pos| minecraftoss_player::World::block(scene, pos);
        let changed: Vec<BlockPos> = minecraftoss_player::enchantment::disk(position, offset, radius, height)
            .into_iter()
            .filter(|&pos| predicate.is_none_or(|p| effects.block_predicate(p, pos, &at)))
            .collect();
        for pos in changed {
            self.world.set_blocks(&[(pos, Some(state.clone()))]);
            self.entities.edited(&self.world.scene, pos, true);
        }
    }

    /// `LivingEntity.breakItem`: the snap, and crumbs of the item.
    fn item_broke(&mut self, item: &str) {
        let eye = self.player.eye();
        let pitch = 0.8 + self.sounds.random() * 0.4;
        self.play("minecraft:entity.item.break", Some(eye), 0.8, pitch);
        self.item_crumbs(item, 5);
    }

    /// `LivingEntity.spawnItemParticles` from the player's mouth.
    fn item_crumbs(&mut self, item: &str, count: usize) {
        crate::emitters::item_crumbs(
            &mut self.particles,
            &self.world,
            item,
            count,
            self.player.pos,
            self.player.eye_height(),
            self.player.yaw,
            self.player.pitch,
        );
    }

    /// The particles of the player's hit on a mob: a crit's sparks
    /// (`ClientboundAnimatePacket` 4), the sweep's arc and the damage
    /// hearts (`Player.attack`'s `sendParticles`).
    fn attack_particles(&mut self, id: u64, result: &minecraftoss_entities::world::AttackResult) {
        let bounds = self.entities.mob_bounds(id);
        if result.critical {
            self.emitters.track(
                &mut self.particles,
                &self.world,
                id,
                bounds,
                crate::particles::Type::Crit,
            );
        }
        // `magicCrit`: the enchantments' sparks.
        if result.magic {
            self.emitters.track(
                &mut self.particles,
                &self.world,
                id,
                bounds,
                crate::particles::Type::EnchantedHit,
            );
        }
        if result.sweep {
            let yaw = (self.player.yaw as f32).to_radians();
            let (dx, dz) = (-f64::from(yaw.sin()), f64::from(yaw.cos()));
            let height = if self.player.crouching { 1.5 } else { 1.8 };
            let at = self.player.pos + DVec3::new(dx, height * 0.5, dz);
            crate::emitters::send(
                &mut self.particles,
                &self.world,
                &crate::particles::Type::SweepAttack.into(),
                at,
                0,
                DVec3::new(dx, 0.0, dz),
                0.0,
            );
        }
        if result.damage_dealt > 2.0
            && let Some((feet, _, height)) = bounds
        {
            crate::emitters::send(
                &mut self.particles,
                &self.world,
                &crate::particles::Type::DamageIndicator.into(),
                feet + DVec3::new(0.0, f64::from(height) * 0.5, 0.0),
                (result.damage_dealt * 0.5) as i32,
                DVec3::new(0.1, 0.0, 0.1),
                0.2,
            );
        }
    }

    fn tick_movement(&mut self, input: Option<&Input>) {
        let held = self.entities.held().cloned();
        let mut movement = minecraftoss_player::Input::default();
        if let Some(input) = input {
            movement.forward = f64::from(i8::from(input.forward) - i8::from(input.back));
            movement.strafe = f64::from(i8::from(input.left) - i8::from(input.right));
            movement.jump = input.jump || self.jump_taps > 0;
            // A tap is held for one tick, then let go for the next, so two
            // taps read as two presses.
            if self.jump_taps > 0 && !self.jump_latched {
                self.jump_latched = true;
            } else if self.jump_latched {
                self.jump_latched = false;
                self.jump_taps = self.jump_taps.saturating_sub(1);
                movement.jump = input.jump;
            }
            movement.crouch = input.sneak;
            if !input.forward {
                self.tap_sprint = false;
            }
            // `isSprintingPossible`: fed enough, and not blind.
            let fed = (self.creative || self.player.survival.food.level > 6)
                && !self.player.survival.has_effect(MobEffect::Blindness);
            // `isSlowDueToUsingItem` keeps the player from sprinting.
            let using = self.eating.is_some() || self.using.is_some();
            movement.sprint = (input.sprint || self.tap_sprint) && fed && !using;
            // `LocalPlayer.aiStep`: too hungry (or blind) to keep running.
            if !fed {
                self.player.sprinting = false;
                self.tap_sprint = false;
            }
            if using {
                // Using an item slows the player to a fifth
                // (`UseEffects.speedMultiplier`).
                movement.forward *= 0.2;
                movement.strafe *= 0.2;
                self.player.sprinting = false;
            }
        }
        let before = self.player.pos;
        let fall = self.player.fall_distance;
        let was_in_water = self.player.in_water;
        let health = self.player.survival.health;
        // What the survival tick's fire needs: the armor, the rain where
        // the player stands, and the level random's draw for fire blocks.
        let inventory = &self.entities.inventory;
        let toughness = inventory.armor_toughness().0 as f32;
        self.player.armor = Armor { value: Some((f32::from(inventory.armor_value()), toughness)), protection: 0.0 };
        // What the enchantments of the equipment see.
        let equipment = inventory
            .equipment(self.entities.selected)
            .into_iter()
            .map(|(slot, stack)| (slot, stack.clone()))
            .collect();
        let effects = inventory.recipes.enchantments().clone();
        self.player.set_equipment(&self.world.scene, &effects, equipment);
        self.player.in_rain = self.raining_on_player();
        self.player.fire_spread = 1 + i32::from(self.sounds.random() < 0.5);
        if self.creative {
            // Effects run out for creative players too, harmlessly.
            self.player.survival.tick_effects_as(true);
            self.player.tick(&self.world.scene, movement);
        } else {
            self.player.tick_survival_with_rules(
                &self.world.scene,
                movement,
                true,
                Difficulty::Normal,
            );
        }
        self.player.tick_attack_strength(held.as_ref());
        self.enchantment_effects();
        let motion = self.player.pos - before;
        if self.player.extinguished {
            let pitch = 1.6 + (self.sounds.random() - self.sounds.random()) * 0.4;
            self.play("minecraft:entity.generic.extinguish_fire", Some(self.player.pos), 0.7, pitch);
        }
        // Fall and other damage the survival tick dealt.
        if self.player.survival.health < health {
            let event = if self.player.survival.burned {
                "minecraft:entity.player.hurt_on_fire"
            } else if self.player.on_ground && fall > 3.0 {
                if fall > 7.0 {
                    "minecraft:entity.player.big_fall"
                } else {
                    "minecraft:entity.player.small_fall"
                }
            } else {
                "minecraft:entity.player.hurt"
            };
            self.play(event, None, 1.0, 1.0);
            self.player.hurt_time = 10;
        }
        self.health_before = self.player.survival.health;
        // Footsteps (`Entity.applyMovementEmissionAndPlaySound`).
        let horizontal = (motion.x.hypot(motion.z) * 0.6) as f32;
        self.walk.dist += horizontal;
        // `LivingEntity.calculateEntityAnimation`.
        let target = (motion.x.hypot(motion.z) as f32 * 4.0).min(1.0);
        self.walk.animation_speed += (target - self.walk.animation_speed) * 0.4;
        self.walk.animation_pos += self.walk.animation_speed;
        let feet = self.player.pos;
        let block_at = |dy: f64| {
            let pos = (
                feet.x.floor() as i32,
                (feet.y - dy).floor() as i32,
                feet.z.floor() as i32,
            );
            Scene::block(&self.world.scene, pos)
                .cloned()
                .map(|b| (pos, b))
        };
        let under = block_at(0.2);
        // `Entity.baseTick`'s sprint dust, `doWaterSplashEffect` and
        // `LivingEntity.checkFallDamage`'s landing dust.
        let width = 0.6;
        if self.player.sprinting
            && !self.player.in_water
            && !self.player.crouching
            && self.alive()
            && let Some((pos, block)) = under.as_ref()
        {
            crate::emitters::sprint(
                &mut self.particles,
                &self.world,
                feet,
                self.player.velocity,
                width,
                (*pos, block),
            );
        }
        if self.player.on_ground
            && fall > 0.0
            && let Some((pos, block)) = under.as_ref()
        {
            crate::emitters::landing(&mut self.particles, &self.world, feet, fall, (*pos, block));
        }
        if self.player.on_ground && !self.player.crouching && horizontal < 2.0 {
            self.walk.move_dist += horizontal;
            if self.walk.move_dist > self.walk.next_step
                && let Some((pos, block)) = under.clone()
            {
                self.walk.next_step = self.walk.move_dist as i32 as f32 + 1.0;
                // Snow layers and carpets sound instead of what they lie on.
                let inside = block_at(-0.01).filter(|(_, b)| {
                    let p = b.id.path.as_str();
                    p == "snow" || p.ends_with("_carpet")
                });
                let (pos, block) = inside.unwrap_or((pos, block));
                if let Some(kind) = self.world.scene.sound_type(&block) {
                    let at = DVec3::new(
                        f64::from(pos.0) + 0.5,
                        f64::from(pos.1) + 1.0,
                        f64::from(pos.2) + 0.5,
                    );
                    self.play(&kind.step, Some(at), kind.volume * 0.15, kind.pitch);
                }
            }
        }
        if self.player.on_ground
            && fall > 3.0
            && let Some((pos, block)) = under
            && let Some(kind) = self.world.scene.sound_type(&block)
        {
            self.play(
                &kind.fall,
                Some(center(pos)),
                kind.volume * 0.5,
                kind.pitch * 0.75,
            );
        }
        if self.player.in_water && !was_in_water && !self.player.swimming {
            let v = self.player.velocity;
            let speed = ((v.x * v.x * 0.2 + v.y * v.y + v.z * v.z * 0.2).sqrt() as f32 * 0.2).min(1.0);
            let event = if speed < 0.25 {
                "minecraft:entity.player.splash"
            } else {
                "minecraft:entity.player.splash.high_speed"
            };
            let pitch = 1.0 + (self.sounds.random() - self.sounds.random()) * 0.4;
            self.play(event, Some(feet), speed, pitch);
            crate::emitters::splash(&mut self.particles, &self.world, feet, v, width);
        }
        // View bobbing (`Player.aiStep`'s bob).
        let speed = if self.player.on_ground && self.alive() {
            (self.player.velocity.x.hypot(self.player.velocity.z) as f32).min(0.1)
        } else {
            0.0
        };
        self.walk.bob += (speed - self.walk.bob) * 0.4;
        // The eye lowers as the player crouches.
        self.eye_height += (self.player.eye_height() - self.eye_height) * 0.5;
        // `AbstractClientPlayer.getFieldOfViewModifier`.
        let mut target = 1.0;
        if self.player.flying {
            target *= 1.1;
        }
        target *= (self.player.movement_speed() / 0.1 + 1.0) / 2.0;
        target *= self.use_fov();
        self.fov += (target - self.fov) * 0.5;
        self.fov = self.fov.clamp(0.1, 1.5);
    }

    fn hurt_by_mob(&mut self, hit: minecraftoss_entities::world::PlayerHit) {
        if self.creative || !self.alive() {
            return;
        }
        // What lands with the hit: a husk's hunger (`Husk.doHurtTarget`)
        // or a tipped arrow's effect (`Arrow.doPostHurtEffects`), and what
        // a mob's weapon's enchantments add.
        let mut after = None;
        let mut melee_weapon = None;
        let (from, kind) = match hit.kind {
            PlayerHitKind::Melee { attacker, lift, hunger_ticks, weapon } => {
                self.player.velocity.y += f64::from(lift);
                if hunger_ticks > 0 {
                    after = Some(EffectInstance::new(MobEffect::Hunger, hunger_ticks, 0));
                }
                melee_weapon = Some(weapon);
                (HitFrom::Position(attacker), "mob_attack")
            }
            PlayerHitKind::Arrow { velocity, effect, .. } => {
                after = effect.map(|(kind, ticks)| EffectInstance::new(kind, ticks as i32, 0));
                (HitFrom::Projectile(velocity), "arrow")
            }
            PlayerHitKind::Explosion { knockback } => {
                // `EXPLOSION_KNOCKBACK_RESISTANCE` (blast protection).
                let resistance = self.player.equipped.attribute("minecraft:explosion_knockback_resistance", 0.0, (0.0, 1.0));
                self.player.velocity += knockback * (1.0 - resistance);
                (HitFrom::Explosion, "player_explosion")
            }
            // `lightning_bolt`: `#no_knockback`, like a blast's own push;
            // `thunderHit` sets the player alight first.
            PlayerHitKind::Lightning => {
                self.player.survival.thunder_ignite();
                (HitFrom::Explosion, "lightning_bolt")
            }
        };
        let inventory = &self.entities.inventory;
        let (toughness, resistance) = inventory.armor_toughness();
        let value = Some((f32::from(inventory.armor_value()), toughness as f32));
        // `getDamageProtection` against the hit, from the mob that dealt it.
        let damage_type = format!("minecraft:{kind}");
        let me = self.player.facts(&self.world.scene, self.player.velocity);
        let attacker = minecraftoss_player::enchantment::EntityFacts::default();
        let source = minecraftoss_player::enchantment::DamageFacts {
            damage_type: &damage_type,
            attacker: hit.source.map(|_| &attacker),
            direct: hit.source.map(|_| &attacker),
            is_direct: !matches!(hit.kind, PlayerHitKind::Arrow { .. }),
        };
        let protection = self.player.equipped.protection(&me, &source);
        let armor = Armor { value, protection };
        let incoming = IncomingHit {
            damage: hit.damage,
            from,
            // `DamageScaling.WHEN_CAUSED_BY_LIVING_NON_PLAYER`: a bolt has
            // no living cause.
            scales_with_difficulty: !matches!(hit.kind, PlayerHitKind::Lightning),
            exhaustion: 0.1,
        };
        if self
            .player
            .hurt_by(&incoming, Difficulty::Normal, armor, resistance)
        {
            self.hurts.push((hit.source, kind));
            self.play("minecraft:entity.player.hurt", None, 1.0, 1.0);
            if let Some(effect) = after {
                self.player.survival.add_effect_instance(effect);
            }
            // `Mob.doHurtTarget`: the weapon's extra knockback along the
            // mob's facing (`LivingEntity.knockback`, less the player's
            // resistance) and its fire.
            if let Some(weapon) = melee_weapon {
                let power = f64::from(weapon.knockback) * (1.0 - resistance);
                if power > 0.0 {
                    let (sin, cos) = minecraftoss_player::minecraft_sin_cos(f64::from(weapon.yaw));
                    let v = self.player.velocity;
                    let y = if self.player.on_ground { (v.y / 2.0 + power).min(0.4) } else { v.y };
                    self.player.velocity = DVec3::new(v.x / 2.0 - sin * power, y, v.z / 2.0 + cos * power);
                }
                if weapon.fire_seconds > 0.0 {
                    self.player.survival.ignite_for_ticks((weapon.fire_seconds * 20.0).floor() as i32);
                }
            }
            // `Player.hurtArmor` (`doHurtEquipment`): each worn piece
            // loses a point for each four of damage, at least one.
            let wear = (hit.damage / 4.0).max(1.0) as u32;
            for index in 36..40 {
                let id = self.entities.inventory.slots[index].as_ref().map(|s| s.id.clone());
                if let Some(id) = id
                    && id != "minecraft:elytra"
                    && self.entities.inventory.wear_tool(index, wear)
                {
                    self.item_broke(&id);
                }
            }
            // The worn enchantments' post-attack effects (thorns striking
            // back at the attacker, and wearing their piece).
            for after in self.player.equipped.post_attack(&me, &source) {
                use minecraftoss_player::enchantment::{EntityEffect, Target};
                match (after.affected, &after.effect) {
                    (Target::Attacker | Target::DamagingEntity, EntityEffect::Damage { amount, .. }) => {
                        if let Some(id) = hit.source {
                            self.entities.hurt_mob(id, *amount, self.player.pos);
                            let at = self.entities.mob_bounds(id).map(|(feet, _, _)| feet);
                            self.play("minecraft:enchant.thorns.hit", at, 1.0, 1.0);
                        }
                    }
                    (_, EntityEffect::ItemDamage { .. }) | (Target::Victim, _) => self.enchantment_effect_on_player(&after.effect, after.slot),
                    _ => {}
                }
            }
        }
    }

    /// `Hud.extractPlayerHealth`'s blink: a hit in its cooldown blinks the
    /// hearts for a second, healing in it for half; the hearts lost show
    /// blinking until a second passes.
    fn hearts(&mut self) -> crate::gui::Hearts {
        let status = &self.player.survival;
        let health = status.health.ceil() as i32;
        let (last, blink_until, display, last_time) = &mut self.health_blink;
        let tick = self.ticks;
        let now = Instant::now();
        if health < *last && status.damage_cooldown() > 0 {
            *last_time = now;
            *blink_until = tick + 20;
        } else if health > *last && status.damage_cooldown() > 0 {
            *last_time = now;
            *blink_until = tick + 10;
        }
        if now.duration_since(*last_time).as_millis() > 1000 {
            *display = health;
            *last_time = now;
        }
        *last = health;
        let blink = *blink_until > tick && (*blink_until - tick) / 3 % 2 == 1;
        crate::gui::Hearts { tick, blink, display_health: *display }
    }

    /// `Entity.isInRain`: rain falls on the player's block or the one at
    /// the top of its box.
    fn raining_on_player(&self) -> bool {
        let p = self.player.pos;
        let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
        let top = (p.y + if self.player.swimming { 0.6 } else if self.player.crouching { 1.5 } else { 1.8 }).floor() as i32;
        self.weather.raining_at(&self.world, (x, p.y.floor() as i32, z))
            || self.weather.raining_at(&self.world, (x, top, z))
    }

    /// `LivingEntity.tickDeath`'s removal twenty ticks after death and
    /// `triggerOnDeathMobEffects`: oozing leaves two slimes of size 2 half
    /// a block up (`OozingMobEffect.onMobRemoved`), weaving two or three
    /// cobwebs on sturdy ground about the player
    /// (`WeavingMobEffect.spawnCobwebsRandomlyAround`), and the effects end.
    /// (Wind charged's burst and infested's silverfish have no entities
    /// here.)
    fn death_effects(&mut self) {
        let effects = std::mem::take(&mut self.player.survival.effects);
        let p = self.player.pos;
        if effects.has(MobEffect::Oozing) {
            for _ in 0..2 {
                self.entities.summon_slime([p.x, p.y + 0.5, p.z], 2);
            }
        }
        if effects.has(MobEffect::Weaving) {
            let count = 2 + usize::from(self.sounds.random() < 0.5);
            let center = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let mut webs: Vec<BlockPos> = Vec::new();
            for _ in 0..15 {
                let mut offset = || (self.sounds.random() * 3.0).floor() as i32 - 1;
                let pos = (center.0 + offset(), center.1 + offset(), center.2 + offset());
                let below = (pos.0, pos.1 - 1, pos.2);
                let replaceable = self.world.block(pos).is_none_or(|b| {
                    matches!(
                        b.id.path.as_str(),
                        "air" | "cave_air" | "void_air" | "water" | "lava" | "short_grass" | "tall_grass" | "fern"
                            | "large_fern" | "dead_bush" | "vine" | "fire" | "soul_fire" | "seagrass" | "tall_seagrass"
                            | "light" | "structure_void"
                    )
                });
                let sturdy = minecraftoss_player::collision_boxes_at(&self.world.scene, below).iter().any(|(min, max)| {
                    let (min, max) = (*min - below_offset(below), *max - below_offset(below));
                    min.x <= 0.0 && min.z <= 0.0 && max.x >= 1.0 && max.z >= 1.0 && max.y >= 1.0
                });
                if !webs.contains(&pos) && replaceable && sturdy {
                    webs.push(pos);
                    if webs.len() >= count {
                        break;
                    }
                }
            }
            for pos in webs {
                let web = minecraft_terrain::scene::Block::new("minecraft:cobweb");
                self.world.set_blocks(&[(pos, Some(web))]);
                self.entities.edited(&self.world.scene, pos, true);
            }
        }
        self.player.survival.refresh_attributes();
    }

    /// `DeathScreen`'s buttons wait twenty ticks.
    fn death_buttons_ready(&self) -> bool {
        self.ticks.saturating_sub(self.died_at) >= 20
    }

    fn check_death(&mut self) {
        if self.creative || self.screen == Screen::Dead || self.player.survival.health > 0.0 {
            return;
        }
        self.play("minecraft:entity.player.death", None, 1.0, 1.0);
        // `AbstractContainerScreen.tick` closes the menu of a dead player;
        // its carried stack comes back, to drop with the rest.
        self.settle_menu();
        let inventory = &mut self.entities.inventory;
        inventory.destroy_vanishing_cursed_items();
        let mut drops = inventory.drain_on_death();
        drops.extend(inventory.settle_crafting());
        drops.extend(inventory.settle_workbench());
        let eye = self.player.eye();
        for stack in drops {
            self.entities.world_items.drop_on_death(stack, eye);
        }
        self.eating = None;
        self.drag = None;
        self.died_at = self.ticks;
        self.screen = Screen::Dead;
    }

    fn respawn(&mut self) {
        self.settle_menu();
        let (x, y, z) = self.world.stream.respawn_position();
        let mut player = Player::new(DVec3::new(x, y, z));
        player.yaw = self.player.yaw;
        player.set_game_mode(self.mode());
        self.player = player;
        self.previous = self.player.pos;
        self.landed = false;
        self.score = 0;
        self.screen = Screen::Playing;
    }

    /// `GameRenderer.renderLevel`'s spinning effect: the view squeezed
    /// across a tilted axis that turns, the stronger the nausea.
    fn nausea_matrix(&self, partial: f32) -> Mat4 {
        let intensity = self.player.survival.effect(MobEffect::Nausea).map_or(0.0, |e| e.blend_factor(partial));
        if intensity <= 0.0 {
            return Mat4::IDENTITY;
        }
        let skew = 5.0 / (intensity * intensity + 5.0) - intensity * 0.04;
        let skew = skew * skew;
        let axis = Vec3::new(0.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
        let angle = (self.spin.0 + partial * self.spin.1).to_radians();
        Mat4::from_axis_angle(axis, angle) * Mat4::from_scale(Vec3::new(1.0 / skew, 1.0, 1.0)) * Mat4::from_axis_angle(axis, -angle)
    }

    /// What the player's effects do to the view (`FogRenderer.setupFog`
    /// and `computeFogColor`, `LightmapRenderStateExtractor.extract`):
    /// blindness's fog closing in to five blocks, darkness's to fifteen as
    /// it fades in, both hiding the sky; night vision's light, and
    /// darkness's pulse.
    fn vision(&self, partial: f32) -> crate::world::Vision {
        let effects = &self.player.survival.effects;
        let render_distance = self.world.view_distance as f32 * 16.0;
        let mut vision = crate::world::Vision { brightness: 0.5, ..Default::default() };
        let blindness = effects.get(MobEffect::Blindness);
        let darkness = effects.get(MobEffect::Darkness);
        if let Some(effect) = blindness {
            let distance = if effect.infinite() {
                5.0
            } else {
                render_distance + ((effect.duration as f32 / 20.0).min(1.0)) * (5.0 - render_distance)
            };
            vision.fog = Some((distance * 0.25, distance, distance * 0.8));
            vision.darkness = if effect.ends_within(19) { effect.duration as f32 / 20.0 } else { 1.0 };
        } else if let Some(effect) = darkness {
            let blend = effect.blend_factor(partial);
            let distance = render_distance + blend * (15.0 - render_distance);
            vision.fog = Some((distance * 0.75, distance, distance));
            vision.darkness = blend;
        }
        vision.sky_blocked = blindness.is_some() || darkness.is_some();
        if let Some(effect) = effects.get(MobEffect::NightVision) {
            // `GameRenderer.nightVisionScale`: flickering out over its
            // last ten seconds.
            let scale = if !effect.ends_within(200) {
                1.0
            } else {
                0.7 + minecraftoss_player::mth::sin(f64::from((effect.duration as f32 - partial) * std::f32::consts::PI * 0.2)) * 0.3
            };
            vision.night_vision = scale;
            if darkness.is_none() {
                vision.brighten = scale;
            }
        }
        // `calculateDarknessScale`, at the default darkness option of 1.
        let gloom = darkness.map_or(0.0, |e| e.blend_factor(partial));
        vision.brightness = (0.5 - gloom).max(0.0);
        let pulse = minecraftoss_player::mth::cos(f64::from((self.ticks as f32 - partial) * std::f32::consts::PI * 0.025));
        vision.darkness_scale = (pulse * 0.45 * gloom).max(0.0);
        vision
    }

    /// Vanilla's `bobHurt` and `bobView`, in view space.
    fn bob_matrix(&self, partial: f32) -> Mat4 {
        let mut matrix = Mat4::IDENTITY;
        let hurt = self.player.hurt_time as f32 - partial;
        if hurt > 0.0 {
            let t = hurt / 10.0;
            let tilt = (t * t * t * t * std::f32::consts::PI).sin();
            let dir = self.player.hurt_dir.to_radians();
            matrix = Mat4::from_rotation_y(-dir)
                * Mat4::from_rotation_z((-tilt * 14.0).to_radians())
                * Mat4::from_rotation_y(dir);
        }
        if !self.player.flying {
            let walked = self.walk.dist - self.walk.previous_dist;
            let h = -(self.walk.dist + walked * partial);
            let i = self.walk.previous_bob + (self.walk.bob - self.walk.previous_bob) * partial;
            let pi = std::f32::consts::PI;
            matrix = matrix
                * Mat4::from_translation(Vec3::new(
                    (h * pi).sin() * i * 0.5,
                    -((h * pi).cos() * i).abs(),
                    0.0,
                ))
                * Mat4::from_rotation_z(((h * pi).sin() * i * 3.0).to_radians())
                * Mat4::from_rotation_x((((h * pi - 0.2).cos() * i).abs() * 5.0).to_radians());
        }
        matrix
    }

    /// The held item, or the bare arm, in view space.
    fn hand_mesh(&mut self, partial: f32, eye: DVec3) -> (Vec<SectionVertex>, Vec<u32>) {
        if !self.alive() {
            return Default::default();
        }
        let swing = self.swing.progress(partial);
        let equip = 1.0 - (self.previous_equip + (self.equip - self.previous_equip) * partial);
        let eye_block = (
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        );
        let light = [
            self.world.light.get(eye_block),
            self.world.light.get_block(eye_block),
        ];
        let bob = self.bob_matrix(partial);
        let held = self
            .equipped
            .clone()
            .and_then(|id| self.entities.held().filter(|s| s.id == id).cloned());
        let light_at = eye.as_vec3();
        let selected = self.entities.selected;
        let (main_shown, offhand_shown) = self.hands_shown();
        let (mut vertices, mut indices) = if !main_shown {
            Default::default()
        } else if let Some(stack) = held {
            // Eating holds the item up to the mouth, bobbing.
            let eating = self.eating.as_ref().map(|eating| {
                let left = eating.remaining_ticks() as f32 - partial + 1.0;
                let bob = if left / (eating.info.consume_ticks as f32) < 0.8 {
                    ((left / 4.0 * std::f32::consts::PI).cos() * 0.1).abs()
                } else {
                    0.0
                };
                Mat4::from_translation(Vec3::new(-0.3, bob - 0.1, 0.0))
                    * Mat4::from_rotation_y(-0.6)
            });
            let pose = match self.ranged_hand_pose(selected, swing, equip, false, partial) {
                Some(pose) if eating.is_none() => bob * pose,
                _ => bob * eating.unwrap_or(Mat4::IDENTITY) * crate::hand::item_pose(swing, equip, false),
            };
            let state = self.held_state(selected);
            let mesh = self.entities.held_item_mesh(
                &stack,
                pose,
                false,
                state,
                light_at,
                &self.world.packs,
                &self.world.atlas,
                &self.world.light,
            );
            let vertices: Vec<SectionVertex> =
                mesh.vertices.iter().map(SectionVertex::from_vertex).collect();
            (vertices, mesh.indices)
        } else {
            let look = self.player.look().as_vec3().normalize_or(Vec3::Z);
            let world_from_view =
                Mat3::from_mat4(Mat4::look_to_rh(Vec3::ZERO, look, Vec3::Y)).transpose();
            let (arm, arm_indices) =
                crate::hand::arm_mesh(&self.world.atlas, swing, equip, light, world_from_view);
            let vertices: Vec<SectionVertex> = arm
                .into_iter()
                .map(|mut vertex| {
                    vertex.position = bob.transform_point3(Vec3::from(vertex.position)).to_array();
                    vertex
                })
                .collect();
            (vertices, arm_indices)
        };
        // The offhand's item in the left hand (`renderArmWithItem` for
        // `OFF_HAND`); the main hand's swing leaves it still.
        if let Some(stack) = self.entities.inventory.slots[40].clone().filter(|_| offhand_shown) {
            let pose = bob * self.ranged_hand_pose(ranged::OFFHAND, 0.0, 0.0, true, partial).unwrap_or_else(|| crate::hand::item_pose(0.0, 0.0, true));
            let state = self.held_state(ranged::OFFHAND);
            let mesh = self.entities.held_item_mesh(
                &stack,
                pose,
                true,
                state,
                light_at,
                &self.world.packs,
                &self.world.atlas,
                &self.world.light,
            );
            let start = vertices.len() as u32;
            vertices.extend(mesh.vertices.iter().map(SectionVertex::from_vertex));
            indices.extend(mesh.indices.iter().map(|index| index + start));
        }
        (vertices, indices)
    }

    /// `ScreenEffectRenderer.submitFire`: a burning player sees two sheets
    /// of `fire_1`, at 90% white, half a block ahead and turned ten degrees
    /// in toward the middle from either side, low on the screen.
    fn screen_fire(&self) -> (Vec<SectionVertex>, Vec<u32>) {
        if !self.alive() || !self.player.survival.on_fire() {
            return Default::default();
        }
        let Ok(sprite) = minecraft_terrain::pack::ResourceId::parse("minecraft:block/fire_1") else {
            return Default::default();
        };
        if !self.world.atlas.contains(&sprite) {
            return Default::default();
        }
        let [u0, v0, u1, v1] = self.world.atlas.region(&sprite);
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        for side in [1.0f32, -1.0] {
            let pose = Mat4::from_translation(Vec3::new(0.24 * side, -0.3, 0.0))
                * Mat4::from_rotation_y(-std::f32::consts::PI / 18.0 * side);
            let start = vertices.len() as u32;
            // `buildSpriteQuad`: the sprite's corners flipped both ways.
            for (x, y, u, v) in [(-0.5, -0.5, u1, v1), (0.5, -0.5, u0, v1), (0.5, 0.5, u0, v0), (-0.5, 0.5, u1, v0)] {
                vertices.push(SectionVertex {
                    position: pose.transform_point3(Vec3::new(x, y, -0.5)).to_array(),
                    uv: [u, v],
                    color: [255, 255, 255, 229],
                    light: [240, 240],
                    pad: [0; 2],
                });
            }
            indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        }
        (vertices, indices)
    }

    /// The outline of the targeted block's shape, as line segments.
    fn outline(&self) -> Vec<[f32; 3]> {
        let Some(hit) = crate::target::target(
            &self.world,
            self.player.eye(),
            self.player.look(),
            self.block_reach(),
        ) else {
            return Vec::new();
        };
        let mut boxes = self.world.collision_boxes(hit.pos);
        if boxes.is_empty() {
            boxes.push([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        }
        let (x, y, z) = (hit.pos.0 as f32, hit.pos.1 as f32, hit.pos.2 as f32);
        let mut lines = Vec::new();
        for b in boxes {
            let g = 0.002;
            let lo = [
                x + b[0] as f32 - g,
                y + b[1] as f32 - g,
                z + b[2] as f32 - g,
            ];
            let hi = [
                x + b[3] as f32 + g,
                y + b[4] as f32 + g,
                z + b[5] as f32 + g,
            ];
            let c = |i: u8| {
                [
                    if i & 1 != 0 { hi[0] } else { lo[0] },
                    if i & 2 != 0 { hi[1] } else { lo[1] },
                    if i & 4 != 0 { hi[2] } else { lo[2] },
                ]
            };
            for (a, b) in [
                (0, 1),
                (2, 3),
                (4, 5),
                (6, 7),
                (0, 2),
                (1, 3),
                (4, 6),
                (5, 7),
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 7),
            ] {
                lines.push(c(a));
                lines.push(c(b));
            }
        }
        lines
    }

    fn ui(&mut self, gui: &mut Gui, _dt: f64, sections: usize) -> UiList {
        let mut ui = UiList::default();
        let hearts = self.hearts();
        let packs = &self.world.packs;
        if !self.landed && self.screen != Screen::Dead {
            gui.loading_screen(&mut ui, "Loading terrain...", None);
            return ui;
        }
        if !self.hide_hud && self.alive() {
            let selected_name = self.highlight.as_ref().map(|(stack, since)| {
                let left = 2.0 - since;
                (gui.stack_name(stack), (left / 0.5).clamp(0.0, 1.0) as f32)
            });
            let debug = self.debug.then(|| self.debug_lines(sections));
            // `extractOverlayMessage`'s fade over its last second.
            let partial = (self.clock / TICK_SECONDS) as f32;
            let overlay = self.overlay.as_ref().map(|(message, left)| {
                let alpha = ((*left as f32 - partial) * 255.0 / 20.0).min(255.0).floor();
                (
                    crate::creative::text(gui.language(), message),
                    alpha / 255.0,
                )
            });
            let screen_lists_effects = gui.effects_origin(self.screen).is_some();
            let hud = Hud {
                effects: &self.player.survival.effects,
                hearts,
                screen_lists_effects,
                inventory: self.shown_inventory(),
                selected: self.entities.selected,
                survival: (!self.creative).then_some(&self.player.survival),
                armor: self.entities.inventory.armor_value(),
                eyes_in_water: self.eyes_in_water(),
                selected_name,
                debug,
                overlay,
            };
            gui.hud(&mut ui, packs, &hud);
        }
        if let Some((rect, size)) = gui.player_box(self.screen, self.creative_inventory_tab())
            && let Some(region) = crate::inventory_player::skin(&self.world.atlas)
        {
            let inventory = &self.entities.inventory;
            let pose = crate::inventory_player::Pose {
                crouching: self.player.crouching,
                holding: [
                    inventory.slots[self.entities.selected].is_some(),
                    inventory.slots[40].is_some(),
                ],
                age: self.ticks as f32,
                walk: (self.walk.animation_pos, self.walk.animation_speed),
                armor: crate::inventory_player::armor_layers(
                    &self.world.atlas,
                    [39, 38, 37, 36].map(|slot| inventory.slots[slot].as_ref()),
                ),
            };
            let (mut model, hands) = crate::inventory_player::model(
                region, rect, size, 0.0625, gui.mouse, gui.scale, &pose,
            );
            // The main hand is the right; the offhand the left.
            let held = [
                self.entities.inventory.slots[self.entities.selected].clone(),
                self.entities.inventory.slots[40].clone(),
            ];
            let mut items = minecraft_terrain::mesh::ChunkMesh::default();
            for ((stack, hand), left) in held.iter().zip(hands).zip([false, true]) {
                if let Some(stack) = stack {
                    self.entities.hand_item_mesh(
                        &mut items,
                        stack,
                        hand,
                        left,
                        &crate::inventory_player::shade,
                        &self.world.packs,
                        &self.world.atlas,
                    );
                }
            }
            let base = model.vertices.len() as u32;
            model.vertices.extend(items.vertices);
            model.indices.extend(items.indices.iter().map(|i| i + base));
            ui.model = Some(model);
        }
        match self.screen {
            Screen::Playing => {}
            Screen::Inventory => {
                gui.container_screen(&mut ui, packs, &self.entities.inventory, false)
            }
            Screen::Crafting => {
                gui.container_screen(&mut ui, packs, &self.entities.inventory, true)
            }
            Screen::Creative => {
                let (tabs, mut view) = self.creative_view();
                view.tabs = &tabs;
                gui.creative_screen(&mut ui, packs, &view);
            }
            _ => {}
        }
        if let Some(origin) = gui.effects_origin(self.screen) {
            gui.effects_in_inventory(&mut ui, &self.player.survival.effects, origin);
        }
        match self.screen {
            Screen::Playing | Screen::Inventory | Screen::Crafting | Screen::Creative => {}
            Screen::Menu => {
                let partial = (self.clock / TICK_SECONDS) as f32;
                if let Some(book) = self.enchanting_book(gui, partial) {
                    ui.model = Some(book);
                }
                if let Some(view) = self.menu_view(gui) {
                    gui.menu_screen(&mut ui, packs, &view);
                }
            }
            Screen::Paused => gui.pause_screen(&mut ui),
            Screen::Dead => gui.death_screen(&mut ui, self.score, self.death_buttons_ready()),
        }
        ui
    }

    fn debug_lines(&self, sections: usize) -> Vec<String> {
        let p = self.player.pos;
        let (bx, by, bz) = self.feet_block();
        let (facing, _) = horizontal_facing(self.player.yaw);
        let biome = self
            .world
            .scene
            .noise_biome((bx >> 2, by >> 2, bz >> 2))
            .map(|id| self.world.registries.biomes.get(id).name.to_string())
            .unwrap_or_else(|| "unknown".into());
        let light = (
            self.world.light.get((bx, by, bz)),
            self.world.light.get_block((bx, by, bz)),
        );
        let time = self.world.day.time();
        vec![
            format!("Minecraft {} (MinecraftOSS)", crate::setup::VERSION),
            format!("{} fps, {} sections drawn", self.frames.2, sections),
            format!("XYZ: {:.3} / {:.5} / {:.3}", p.x, p.y, p.z),
            format!("Block: {bx} {by} {bz}"),
            format!("Chunk: {} {} in {} {}", bx & 15, bz & 15, bx >> 4, bz >> 4),
            format!(
                "Facing: {facing} ({:.1} / {:.1})",
                self.player.yaw.rem_euclid(360.0) - 180.0,
                self.player.pitch
            ),
            format!("Biome: {biome}"),
            format!("Light: {} sky, {} block", light.0, light.1),
            format!("Day {}, time {:.0}", self.world.day.day_count(), time),
            format!("Seed: {}", self.world.seed),
            format!(
                "Mode: {}",
                if self.creative {
                    "creative"
                } else {
                    "survival"
                }
            ),
        ]
    }
}

impl Drop for Game {
    /// A crash still saves the player, as its unwinding saves the chunks.
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.save();
        }
    }
}

const HORIZONTAL: [(&str, (i32, i32)); 4] = [
    ("south", (0, 1)),
    ("west", (-1, 0)),
    ("north", (0, -1)),
    ("east", (1, 0)),
];

fn face_name(face: minecraftoss_player::Face) -> &'static str {
    use minecraftoss_player::Face;
    match face {
        Face::Down => "down",
        Face::Up => "up",
        Face::North => "north",
        Face::South => "south",
        Face::West => "west",
        Face::East => "east",
    }
}

fn horizontal_facing(yaw: f64) -> (&'static str, (i32, i32)) {
    HORIZONTAL[((yaw / 90.0 + 0.5).floor() as i32).rem_euclid(4) as usize]
}

/// The item a block is picked as (`Block.getCloneItemStack`): itself for
/// most blocks, the item that places it for the rest, nothing for fluids,
/// fire and portals.
fn block_item(block: &Block) -> Option<String> {
    let path = block.id.path.as_str();
    let item = match path {
        "water" | "lava" | "fire" | "soul_fire" | "bubble_column" | "nether_portal"
        | "end_portal" | "end_gateway" | "moving_piston" | "frosted_ice" | "air" | "cave_air"
        | "void_air" => return None,
        "redstone_wire" => "redstone",
        "tripwire" => "string",
        "kelp_plant" => "kelp",
        "tall_seagrass" => "seagrass",
        "cave_vines" | "cave_vines_plant" => "glow_berries",
        "twisting_vines_plant" => "twisting_vines",
        "weeping_vines_plant" => "weeping_vines",
        "big_dripleaf_stem" => "big_dripleaf",
        "bamboo_sapling" => "bamboo",
        "carrots" => "carrot",
        "potatoes" => "potato",
        "beetroots" => "beetroot_seeds",
        "wheat" => "wheat_seeds",
        "melon_stem" | "attached_melon_stem" => "melon_seeds",
        "pumpkin_stem" | "attached_pumpkin_stem" => "pumpkin_seeds",
        "torchflower_crop" => "torchflower_seeds",
        "pitcher_crop" => "pitcher_pod",
        "sweet_berry_bush" => "sweet_berries",
        "cocoa" => "cocoa_beans",
        "powder_snow" => "powder_snow_bucket",
        "piston_head" => {
            if block.properties.get("type").is_some_and(|t| t == "sticky") {
                "sticky_piston"
            } else {
                "piston"
            }
        }
        "flower_pot" => "flower_pot",
        _ => {
            let item = if let Some(plant) = path.strip_prefix("potted_") {
                plant.to_owned()
            } else if path.ends_with("_candle_cake") {
                "cake".to_owned()
            } else {
                path.replace("_wall_hanging_sign", "_hanging_sign")
                    .replace("_wall_sign", "_sign")
                    .replace("_wall_banner", "_banner")
                    .replace("_wall_head", "_head")
                    .replace("_wall_skull", "_skull")
                    .replace("_wall_fan", "_fan")
                    .replace("wall_torch", "torch")
            };
            return Some(format!("minecraft:{item}"));
        }
    };
    Some(format!("minecraft:{item}"))
}

/// Whether a drag can share `carried` into a slot: one that is empty or
/// holds the same item with room, and for armour, a piece that goes there.
fn drag_accepts(
    inventory: &minecraftoss_player::inventory::Inventory,
    slot: Slot,
    carried: &ItemStack,
) -> bool {
    let held = match slot {
        Slot::Inventory(index) => {
            let part = match index {
                39 => Some("head"),
                38 => Some("chest"),
                37 => Some("legs"),
                36 => Some("feet"),
                _ => None,
            };
            if part.is_some_and(|part| inventory.recipes.equipment_slot(carried) != Some(part)) {
                return false;
            }
            inventory.slots.get(index)
        }
        Slot::Crafting(index) => inventory.crafting.get(index),
        Slot::Workbench(index) => inventory.workbench.get(index),
        _ => None,
    };
    held.is_some_and(|held| {
        held.as_ref()
            .is_none_or(|stack| stack.same_item(carried) && stack.count < stack.max)
    })
}

/// Whole wheel notches scrolled, up positive, keeping the fraction for later.
fn take_notches(scroll: &mut f32) -> i32 {
    let notches = scroll.trunc();
    *scroll -= notches;
    notches as i32
}

/// A block's corner, to take block-relative boxes from world ones.
fn below_offset(pos: BlockPos) -> DVec3 {
    DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2))
}

fn center(pos: BlockPos) -> DVec3 {
    DVec3::new(
        f64::from(pos.0) + 0.5,
        f64::from(pos.1) + 0.5,
        f64::from(pos.2) + 0.5,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use minecraftoss_player::inventory::Inventory;

    fn stack(id: &str, count: u8) -> ItemStack {
        ItemStack::new(id, count)
    }

    #[test]
    fn a_drag_over_more_slots_than_items_makes_no_empty_stacks() {
        let mut inventory = Inventory::default();
        inventory.cursor = Some(stack("minecraft:oak_planks", 1));
        inventory.distribute_crafting(&[9, 10, 11], false, false);
        // Nothing to share out: the item stays on the cursor.
        assert!(inventory.slots.iter().all(Option::is_none));
        assert_eq!(inventory.cursor.as_ref().map(|s| s.count), Some(1));
        inventory.cursor = Some(stack("minecraft:oak_planks", 5));
        inventory.distribute_crafting(&[9, 10], false, false);
        let counts: Vec<u8> = inventory.slots.iter().flatten().map(|s| s.count).collect();
        assert_eq!(counts, [2, 2]);
        assert_eq!(inventory.cursor.as_ref().map(|s| s.count), Some(1));
    }

    #[test]
    fn a_drag_takes_only_as_many_slots_as_items() {
        let mut inventory = Inventory::default();
        let carried = stack("minecraft:oak_planks", 2);
        assert!(drag_accepts(&inventory, Slot::Inventory(9), &carried));
        assert!(drag_accepts(&inventory, Slot::Workbench(4), &carried));
        // Only boots go on the feet.
        assert!(!drag_accepts(&inventory, Slot::Inventory(36), &carried));
        inventory.slots[10] = Some(stack("minecraft:stone", 3));
        assert!(!drag_accepts(&inventory, Slot::Inventory(10), &carried));
    }

    #[test]
    fn the_crafting_table_shift_click_fills_its_grid() {
        let mut inventory = Inventory::default();
        inventory.slots[12] = Some(stack("minecraft:oak_planks", 5));
        inventory.quick_move_to_workbench(12);
        assert!(inventory.slots[12].is_none());
        assert_eq!(inventory.workbench[0].as_ref().map(|s| s.count), Some(5));
        // With the grid full, a main slot goes to the hotbar.
        for cell in &mut inventory.workbench {
            *cell = Some(stack("minecraft:stone", 64));
        }
        inventory.slots[20] = Some(stack("minecraft:dirt", 7));
        inventory.quick_move_to_workbench(20);
        assert_eq!(inventory.slots[0].as_ref().map(|s| s.count), Some(7));
    }

    #[test]
    fn picked_blocks_name_their_items() {
        let item = |id: &str| block_item(&Block::new(id));
        assert_eq!(item("minecraft:stone").as_deref(), Some("minecraft:stone"));
        assert_eq!(
            item("minecraft:carrots").as_deref(),
            Some("minecraft:carrot")
        );
        assert_eq!(
            item("minecraft:oak_wall_sign").as_deref(),
            Some("minecraft:oak_sign")
        );
        assert_eq!(
            item("minecraft:redstone_wall_torch").as_deref(),
            Some("minecraft:redstone_torch")
        );
        assert_eq!(
            item("minecraft:potted_poppy").as_deref(),
            Some("minecraft:poppy")
        );
        assert_eq!(item("minecraft:water"), None);
    }

    #[test]
    fn wheel_notches_keep_their_fraction() {
        let mut scroll = 0.6;
        assert_eq!(take_notches(&mut scroll), 0);
        scroll += 0.6;
        assert_eq!(take_notches(&mut scroll), 1);
        assert!((scroll - 0.2).abs() < 1e-5);
        scroll = -2.5;
        assert_eq!(take_notches(&mut scroll), -2);
    }
}
