//! The game: one player in a MinecraftOSS world. Movement, survival and the
//! inventory run on vanilla's 20 Hz tick through MinecraftOSS's player crate;
//! the world streams, mobs move and the frame is drawn every frame.
use std::path::PathBuf;
use std::time::Instant;

use glam::{DVec3, Mat3, Mat4, Vec3};
use minecraft_terrain::mesh::SectionVertex;
use minecraft_terrain::pack::ResourceId;
use minecraft_terrain::scene::{Block, BlockPos, Scene};
use minecraft_terrain::sections::CullCamera;
use minecraftoss_entities::world::{PlayerAttack, PlayerHitKind};
use minecraftoss_player::food::{FoodUse, FoodUseTick};
use minecraftoss_player::inventory::ItemStack;
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
    ambient: crate::ambient::Ambient,
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
        if let Some(saved) = saved.as_ref() {
            creative = saved.creative || options.creative;
            player.pos = DVec3::from_array(saved.position);
            player.yaw = saved.yaw;
            player.pitch = saved.pitch;
            player.survival.health = saved.health.clamp(1.0, 20.0);
            player.survival.food.level = saved.food;
            player.survival.food.saturation = saved.saturation;
            (
                player.survival.experience_level,
                player.survival.experience_progress,
                player.survival.total_experience,
            ) = saved.experience;
            world.day.set(saved.day_ticks);
            entities.selected = saved.selected;
            for (slot, stack) in entities.inventory.slots.iter_mut().zip(&saved.slots) {
                *slot = stack.clone().map(|mut stack| {
                    // The item's own stack size, which worn tools keep.
                    stack.max = stack
                        .components
                        .as_ref()
                        .and_then(|c| c.get("minecraft:max_stack_size")?.as_u64())
                        .map_or_else(
                            || entities.inventory.recipes.max_stack(&stack.id),
                            |max| max.clamp(1, 99) as u8,
                        );
                    stack
                });
            }
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
            ambient: crate::ambient::Ambient::default(),
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
        self.swing.start();
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
        // Leftovers that didn't fit are items on the ground: they save
        // with the server's entities.
        self.entities.hand_over_drops();
        if self.save_dir.is_some() {
            self.world.stream.save_edited();
        }
        self.save();
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
            flying: self.player.flying,
            selected: self.entities.selected,
            slots: self.entities.inventory.slots.clone(),
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
            if input.middle_click {
                self.pick_block();
            }
        }
        // Notches no screen used are gone; part of one waits for the next.
        input.scroll = input.scroll.fract();
        input.clicks.clear();
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
        };
        let bright = self.world.sky_light_level() > 11.0;
        let events = self
            .entities
            .tick(world_dt, self.world.day.ticks as i64, bright, &mut view);
        self.hurts = view.hurts;
        self.world.set_blocks(&events.changes);
        if events.experience > 0 && !self.creative {
            self.player
                .survival
                .give_experience_points(events.experience);
            self.score += events.experience;
            let pitch = (self.sounds.random() - self.sounds.random()) * 0.35 + 0.9;
            self.play("minecraft:entity.experience_orb.pickup", None, 0.1, pitch);
        }
        for hit in events.hits {
            self.hurt_by_mob(hit);
        }
        for pos in events.bone_meal_used {
            let mut sounds = Vec::new();
            crate::ambient::bone_meal(&mut self.particles, &self.world, pos, &mut sounds);
            for (event, at, volume, pitch) in sounds {
                self.play(event, Some(at), volume, pitch);
            }
        }
        if events.use_taken {
            self.start_swing();
        }
        // A mob with nothing to do with the held item: it's used in the
        // air, as food is eaten (`MultiPlayerGameMode.useItem`).
        if events.use_passed
            && input.use_item
            && self.eating.is_none()
            && self.captures_mouse()
            && self.alive()
            && let Some(stack) = self.entities.held().cloned()
        {
            let food = &self.player.survival.food;
            self.eating = FoodUse::start(self.entities.selected, &stack, food, self.mode());
        }
        for (event, at, volume, pitch) in std::mem::take(&mut self.entities.sounds) {
            self.play(&event, Some(at), volume, pitch);
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
        let view_matrix = bob * Mat4::look_to_rh(Vec3::ZERO, forward, up);
        let projection = Mat4::perspective_infinite_reverse_rh(fov.to_radians(), aspect, 0.05);
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
            outline,
            hand,
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

    fn keys(&mut self, input: &mut Input, gui: &Gui) {
        for key in std::mem::take(&mut input.keys) {
            if self.screen == Screen::Creative && self.creative_key(key, gui, input.ctrl) {
                continue;
            }
            match (key, self.screen) {
                (Key::Escape, Screen::Playing) => self.pause(),
                (Key::Escape, Screen::Paused) => self.screen = Screen::Playing,
                (
                    Key::Escape | Key::Inventory,
                    Screen::Inventory | Screen::Crafting | Screen::Creative,
                ) => self.close_container(),
                (Key::Inventory, Screen::Playing) if self.alive() && self.landed => {
                    if self.creative {
                        self.open_creative(gui);
                        self.screen = Screen::Creative;
                    } else {
                        self.screen = Screen::Inventory;
                    }
                }
                (Key::Hotbar(slot), Screen::Playing) => {
                    if !self.hotbar_keys(slot, input) {
                        self.entities.selected = slot;
                    }
                }
                (Key::Hotbar(slot), Screen::Inventory | Screen::Crafting) => {
                    if let (Some(Slot::Inventory(index)), _) =
                        gui.slot_at(self.screen == Screen::Crafting)
                    {
                        self.entities.inventory.number_swap(index, slot);
                    }
                }
                // `THROW` over a stack: one of it, or all with control.
                (Key::Drop, Screen::Inventory | Screen::Crafting) => {
                    if let (Some(Slot::Inventory(index)), _) =
                        gui.slot_at(self.screen == Screen::Crafting)
                        && self.entities.inventory.cursor.is_none()
                    {
                        let dropped = self.entities.inventory.drop_selected(index, input.ctrl);
                        self.throw(dropped.into_iter().collect());
                    }
                }
                (Key::Drop, Screen::Playing) if self.alive() => {
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
        // `MinecraftServer.autoSave`: every five minutes.
        if self.ticks.is_multiple_of(6000) && self.save_dir.is_some() {
            self.world.stream.save_edited();
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
        let playing = self.screen == Screen::Playing && self.alive() && self.landed;
        if playing {
            self.tick_hands(input);
        } else {
            self.attack_presses = 0;
            self.use_presses = 0;
            self.mining.reset();
            self.eating = None;
        }
        self.swing.tick();
        if self.landed && self.alive() && self.world.chunk_ready(feet) {
            self.tick_movement(if self.screen == Screen::Playing {
                Some(input)
            } else {
                None
            });
        }
        self.entities.tick_scene(&self.world.scene);
        // `ClientLevel.animateTick`, then `ParticleEngine.tick`.
        let mut sounds = Vec::new();
        if self.landed {
            let eye = self.player.eye().floor();
            let around = (eye.x as i32, eye.y as i32, eye.z as i32);
            self.ambient
                .tick(&self.world, &mut self.particles, around, &mut sounds);
        }
        self.particles.player = Some((self.player.pos, self.player.velocity.y));
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
            let swing = self.mining.tick(
                &self.world.registries,
                &self.world.scene,
                target.as_ref().map(|h| h.pos),
                held.as_ref(),
                on_ground,
                water,
                self.creative,
                attacked,
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
                            let pitch = (self.sounds.random() - self.sounds.random()) * 0.2 + 1.0;
                            let volume = 0.5 + 0.5 * self.sounds.random();
                            self.play(&sound, None, volume, pitch);
                        }
                    }
                    FoodUseTick::Finished { overflow } => {
                        let id = eating.stack.id.clone();
                        self.eating = None;
                        let sounds = &mut self.sounds;
                        let teleport = minecraftoss_player::food::apply_consumed_food_effects(
                            &id,
                            &mut self.player.survival,
                            || sounds.random(),
                        );
                        if teleport {
                            self.teleport_randomly();
                        }
                        self.throw(overflow.into_iter().collect());
                        let pitch = self.sounds.random() * 0.1 + 0.9;
                        self.play("minecraft:entity.player.burp", None, 0.5, pitch);
                    }
                    FoodUseTick::Cancelled => self.eating = None,
                }
            }
        } else if pressed || (input.use_item && self.use_delay == 0) {
            self.use_delay = 4;
            self.use_item(held.as_ref(), target, mob_distance.is_some(), pressed);
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
        let damage = minecraftoss_player::item_catalog::attribute_value(
            1.0,
            modifiers
                .iter()
                .filter(|m| m.attribute == "minecraft:attack_damage"),
            (0.0, 2048.0),
        );
        let speed = minecraftoss_player::item_catalog::attribute_value(
            4.0,
            modifiers
                .iter()
                .filter(|m| m.attribute == "minecraft:attack_speed"),
            (0.0, 1024.0),
        );
        let strength = self.player.attack_strength_scale(0.5, speed);
        let attack = PlayerAttack {
            player_id: 0,
            position: self.player.pos,
            yaw: self.player.yaw as f32,
            attack_damage: damage,
            strength,
            sprinting: self.player.sprinting,
            can_critical: !self.player.on_ground
                && self.player.velocity.y < 0.0
                && !self.player.in_water
                && !self.player.sprinting,
            can_sweep: self.player.on_ground && held.is_some_and(|s| s.id.ends_with("_sword")),
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
                if self.entities.inventory.wear_tool(selected, 1) {
                    let pitch = 0.8 + self.sounds.random() * 0.4;
                    self.play("minecraft:entity.item.break", Some(eye), 0.8, pitch);
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
    ) {
        let eye = self.player.eye();
        let look = self.player.look();
        if at_mob {
            // The server answers whether the mob took it (see `frame`).
            self.entities
                .interact(eye, look, self.entity_reach(), self.creative);
            return;
        }
        if let Some(hit) = target.as_ref()
            && !self.player.crouching
            && let Some(block) = self.world.block(hit.pos).cloned()
        {
            if block.id.path == "crafting_table" {
                if pressed {
                    self.screen = Screen::Crafting;
                }
                return;
            }
            let (facing, _) = horizontal_facing(self.player.yaw);
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
        let Some(stack) = held else { return };
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
                    self.play("minecraft:entity.item.break", Some(eye), 0.8, 1.0);
                }
            }
            return;
        }
        if target.is_some() && self.place(stack) {
            return;
        }
        let food = &self.player.survival.food;
        if let Some(eating) = FoodUse::start(self.entities.selected, stack, food, self.mode()) {
            self.eating = Some(eating);
        }
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
        for (pos, _) in &placed.blocks {
            self.entities.edited(&self.world.scene, *pos, true);
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
        for (at, _) in &gone {
            self.entities.edited(&self.world.scene, *at, false);
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
        self.entities.drop_loot(pos, broken.drops);
        self.player.survival.food.add_exhaustion(0.005);
        let selected = self.entities.selected;
        if self
            .entities
            .inventory
            .wear_tool_after_mining(selected, broken.hardness)
        {
            let eye = self.player.eye();
            let pitch = 0.8 + self.sounds.random() * 0.4;
            self.play("minecraft:entity.item.break", Some(eye), 0.8, pitch);
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
            let fed = self.creative || self.player.survival.food.level > 6;
            movement.sprint = (input.sprint || self.tap_sprint) && fed && self.eating.is_none();
            // `LocalPlayer.aiStep`: too hungry to keep running.
            if !fed {
                self.player.sprinting = false;
                self.tap_sprint = false;
            }
            if self.eating.is_some() {
                // Eating slows the player to a fifth.
                movement.forward *= 0.2;
                movement.strafe *= 0.2;
            }
        }
        let before = self.player.pos;
        let fall = self.player.fall_distance;
        let health = self.player.survival.health;
        if self.creative {
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
        let motion = self.player.pos - before;
        // Fall and other damage the survival tick dealt.
        if self.player.survival.health < health {
            let event = if self.player.on_ground && fall > 3.0 {
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
        self.fov += (target - self.fov) * 0.5;
        self.fov = self.fov.clamp(0.1, 1.5);
    }

    fn hurt_by_mob(&mut self, hit: minecraftoss_entities::world::PlayerHit) {
        if self.creative || !self.alive() {
            return;
        }
        let (from, kind) = match hit.kind {
            PlayerHitKind::Melee { attacker, lift, .. } => {
                self.player.velocity.y += f64::from(lift);
                (HitFrom::Position(attacker), "mob_attack")
            }
            PlayerHitKind::Arrow { velocity, .. } => (HitFrom::Projectile(velocity), "arrow"),
            PlayerHitKind::Explosion { knockback } => {
                self.player.velocity += knockback;
                (HitFrom::Explosion, "player_explosion")
            }
        };
        let inventory = &self.entities.inventory;
        let (toughness, resistance) = inventory.armor_toughness();
        let armor = Armor {
            value: Some((f32::from(inventory.armor_value()), toughness as f32)),
        };
        let incoming = IncomingHit {
            damage: hit.damage,
            from,
            scales_with_difficulty: true,
            exhaustion: 0.1,
        };
        if self
            .player
            .hurt_by(&incoming, Difficulty::Normal, armor, resistance)
        {
            self.hurts.push((hit.source, kind));
            self.play("minecraft:entity.player.hurt", None, 1.0, 1.0);
        }
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
        let inventory = &mut self.entities.inventory;
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
        let Some(stack) = held else {
            let look = self.player.look().as_vec3().normalize_or(Vec3::Z);
            let world_from_view =
                Mat3::from_mat4(Mat4::look_to_rh(Vec3::ZERO, look, Vec3::Y)).transpose();
            let (mut vertices, indices) =
                crate::hand::arm_mesh(&self.world.atlas, swing, equip, light, world_from_view);
            for vertex in &mut vertices {
                vertex.position = bob.transform_point3(Vec3::from(vertex.position)).to_array();
            }
            return (vertices, indices);
        };
        let display = ResourceId::parse(&stack.id)
            .ok()
            .and_then(|id| {
                minecraft_terrain::model::item_first_person_transform(&self.world.packs, &id).ok()
            })
            .unwrap_or(Mat4::IDENTITY);
        // Eating holds the item up to the mouth, bobbing.
        let eating = self.eating.as_ref().map(|eating| {
            let left = eating.remaining_ticks() as f32 - partial + 1.0;
            let bob = if left / (eating.info.consume_ticks as f32) < 0.8 {
                ((left / 4.0 * std::f32::consts::PI).cos() * 0.1).abs()
            } else {
                0.0
            };
            Mat4::from_translation(Vec3::new(-0.3, bob - 0.1, 0.0)) * Mat4::from_rotation_y(-0.6)
        });
        let pose =
            bob * eating.unwrap_or(Mat4::IDENTITY) * crate::hand::item_pose(display, swing, equip);
        let light_at = eye.as_vec3();
        let mesh = self.entities.held_item_mesh(
            &stack.id,
            pose,
            light_at,
            &self.world.packs,
            &self.world.atlas,
            &self.world.light,
        );
        (
            mesh.vertices
                .iter()
                .map(SectionVertex::from_vertex)
                .collect(),
            mesh.indices,
        )
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
            let hud = Hud {
                inventory: &self.entities.inventory,
                selected: self.entities.selected,
                survival: (!self.creative).then_some(&self.player.survival),
                armor: self.entities.inventory.armor_value(),
                eyes_in_water: self.eyes_in_water(),
                selected_name,
                shake: self.ticks,
                debug,
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
