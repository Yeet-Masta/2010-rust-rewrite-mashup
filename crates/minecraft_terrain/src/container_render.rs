//! The containers' block entity renderers: `ChestRenderer` (chests, trapped
//! and copper chests, ender chests; single or either half of a double) and
//! `ShulkerBoxRenderer`, with their lids as the client's block entities
//! pose them: `ChestLidController` and `ShulkerBoxBlockEntity`'s progress,
//! both moved by the server's block event 1 and ticked by the client level.
//! Their blocks' own models have no elements, so this is all there is of
//! them in the world. Also `EnchantTableRenderer`: the book floating over
//! an enchanting table, turning to the nearest player and opening for them
//! as `EnchantingTableBlockEntity.bookAnimationTick` moves it.
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use glam::{DVec3, Mat4, Quat, Vec3};

use crate::cow_render::entity_shade;
use crate::lighting::SkyLight;
use crate::mesh::{Atlas, ChunkMesh};
use crate::pack::ResourceId;
use crate::scene::{Block, BlockPos, Scene};
use crate::special_icon::{book_model, chest_model, emit_part, shulker_box_model};
use minecraftoss_player::rng::LegacyRandom;

/// `ChestLidController`: whether the lid should be open, and its openness
/// this tick and the last.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChestLidController {
    should_be_open: bool,
    openness: f32,
    o_openness: f32,
}

impl ChestLidController {
    /// `tickLid`: a tenth of the way toward open or shut each tick.
    pub fn tick_lid(&mut self) {
        self.o_openness = self.openness;
        if !self.should_be_open && self.openness > 0.0 {
            self.openness = (self.openness - 0.1).max(0.0);
        } else if self.should_be_open && self.openness < 1.0 {
            self.openness = (self.openness + 0.1).min(1.0);
        }
    }

    /// `getOpenness`: between the last tick's and this tick's.
    pub fn openness(&self, partial: f32) -> f32 {
        self.o_openness + (self.openness - self.o_openness) * partial
    }

    /// `shouldBeOpen`.
    pub fn should_be_open(&mut self, open: bool) {
        self.should_be_open = open;
    }

    /// Shut and still: as a new block entity's.
    fn at_rest(&self) -> bool {
        *self == Self::default()
    }
}

/// `ShulkerBoxBlockEntity.AnimationStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShulkerAnimation {
    #[default]
    Closed,
    Opening,
    Opened,
    Closing,
}

/// A shulker box's lid on the client: `ShulkerBoxBlockEntity`'s animation
/// status and its progress this tick and the last.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShulkerLid {
    pub status: ShulkerAnimation,
    progress: f32,
    progress_old: f32,
}

impl ShulkerLid {
    /// `triggerEvent(1, count)`: the lid closes at no openers and opens at
    /// the first.
    pub fn trigger(&mut self, count: i32) {
        if count == 0 {
            self.status = ShulkerAnimation::Closing;
        }
        if count == 1 {
            self.status = ShulkerAnimation::Opening;
        }
    }

    /// `updateAnimation`: a tenth of the way a tick while it moves.
    pub fn tick(&mut self) {
        self.progress_old = self.progress;
        match self.status {
            ShulkerAnimation::Closed => self.progress = 0.0,
            ShulkerAnimation::Opening => {
                self.progress += 0.1;
                if self.progress >= 1.0 {
                    self.status = ShulkerAnimation::Opened;
                    self.progress = 1.0;
                }
            }
            ShulkerAnimation::Opened => self.progress = 1.0,
            ShulkerAnimation::Closing => {
                self.progress -= 0.1;
                if self.progress <= 0.0 {
                    self.status = ShulkerAnimation::Closed;
                    self.progress = 0.0;
                }
            }
        }
    }

    /// `getProgress`: between the last tick's and this tick's.
    pub fn progress(&self, partial: f32) -> f32 {
        self.progress_old + (self.progress - self.progress_old) * partial
    }
}

/// One container's lid.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lid {
    Chest(ChestLidController),
    Shulker(ShulkerLid),
}

/// `EnchantingTableBlockEntity`'s book: its time, leafing (`flip`, its
/// target and speed), opening and turn (`rot`, toward `tRot`), with the
/// last tick's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TableBook {
    time: i32,
    flip: f32,
    o_flip: f32,
    flip_t: f32,
    flip_a: f32,
    open: f32,
    o_open: f32,
    rot: f32,
    o_rot: f32,
    t_rot: f32,
}

/// An angle into `[-pi, pi)`, as the ticks' loops wrap it.
fn wrap(mut angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    while angle >= PI {
        angle -= TAU;
    }
    while angle < -PI {
        angle += TAU;
    }
    angle
}

impl TableBook {
    /// `bookAnimationTick`: with a player within 3 blocks of the table's
    /// middle the book turns to face them, opens, and leafs (on opening,
    /// and now and then); without, it turns slowly and closes.
    pub fn tick(&mut self, centre: DVec3, player: Option<DVec3>, random: &mut LegacyRandom) {
        self.o_open = self.open;
        self.o_rot = self.rot;
        match player.filter(|feet| feet.distance_squared(centre) < 9.0) {
            Some(feet) => {
                self.t_rot = (feet.z - centre.z).atan2(feet.x - centre.x) as f32;
                self.open += 0.1;
                if self.open < 0.5 || random.next_int(40) == 0 {
                    let old = self.flip_t;
                    while self.flip_t == old {
                        self.flip_t += random.next_int(4) as f32 - random.next_int(4) as f32;
                    }
                }
            }
            None => {
                self.t_rot += 0.02;
                self.open -= 0.1;
            }
        }
        self.rot = wrap(self.rot);
        self.t_rot = wrap(self.t_rot);
        self.rot += wrap(self.t_rot - self.rot) * 0.4;
        self.open = self.open.clamp(0.0, 1.0);
        self.time += 1;
        self.o_flip = self.flip;
        let diff = ((self.flip_t - self.flip) * 0.4).clamp(-0.2, 0.2);
        self.flip_a += (diff - self.flip_a) * 0.9;
        self.flip += self.flip_a;
    }

    /// `EnchantTableRenderer.extractRenderState`: the time, leafing,
    /// opening and turn between the last tick and this one.
    pub fn pose(&self, partial: f32) -> (f32, f32, f32, f32) {
        let lerp = |old: f32, now: f32| old + (now - old) * partial;
        let turn = self.o_rot + wrap(self.rot - self.o_rot) * partial;
        (self.time as f32 + partial, lerp(self.o_flip, self.flip), lerp(self.o_open, self.open), turn)
    }
}

/// `BookModel.State.forAnimation` for a book at `time`, leafed to `flip`
/// (its two pages a quarter and three quarters on) and opened to `open`:
/// the openness, breathing with the time, and each page's turn.
pub fn book_state(time: f32, flip: f32, open: f32) -> [f32; 3] {
    let page = |offset: f32| {
        let at = flip + offset;
        ((at - at.floor()) * 1.6 - 0.3).clamp(0.0, 1.0)
    };
    [((time * 0.02).sin() * 0.1 + 1.25) * open, page(0.25), page(0.75)]
}

/// The enchanting table's book's sheet (`EnchantTableRenderer.BOOK_TEXTURE`).
pub const BOOK_SHEET: &str = "minecraft:entity/enchantment/enchanting_table_book";

/// `BookModel` in a state ([`book_state`]) under `pose`, from its 64 by 32
/// sheet at `region`: the enchanting table's book, in the world and on
/// its screen.
pub fn append_book(
    mesh: &mut ChunkMesh,
    pose: Mat4,
    region: [f32; 4],
    shade: &dyn Fn(Vec3) -> f32,
    light: [f32; 2],
    [openness, flip1, flip2]: [f32; 3],
) {
    for part in &book_model(openness, flip1, flip2) {
        emit_part(mesh, part, pose, region, [64.0, 32.0], [1.0; 3], shade, light);
    }
}

/// The lids of the client's container block entities, by position. A lid
/// that is shut and still is the same as none, so only moving or open ones
/// are kept. The enchanting tables' books too, for the tables in view.
#[derive(Default)]
pub struct ContainerLids {
    lids: HashMap<BlockPos, Lid>,
    books: HashMap<BlockPos, TableBook>,
    /// `EnchantingTableBlockEntity.RANDOM`.
    random: LegacyRandom,
}

impl ContainerLids {
    /// A block event from the server (`ClientLevel.blockEvent`): event 1 is
    /// the opener count of a chest or ender chest (`ChestBlockEntity` and
    /// `EnderChestBlockEntity.triggerEvent`: the lid opens with any) or of
    /// a shulker box.
    pub fn block_event(&mut self, pos: BlockPos, block: &str, a: i32, b: i32) {
        if a != 1 {
            return;
        }
        match renderer_of(block.strip_prefix("minecraft:").unwrap_or(block)) {
            Some(Renderer::Chest(_)) => {
                let lid = self.lids.entry(pos).or_insert(Lid::Chest(ChestLidController::default()));
                if let Lid::Chest(lid) = lid {
                    lid.should_be_open(b > 0);
                }
            }
            Some(Renderer::ShulkerBox(_)) => {
                let lid = self.lids.entry(pos).or_insert(Lid::Shulker(ShulkerLid::default()));
                if let Lid::Shulker(lid) = lid {
                    lid.trigger(b);
                }
            }
            Some(Renderer::EnchantingTable) | None => {}
        }
    }

    /// The enchanting tables among `positions` have their books, which
    /// tick from then on.
    pub fn track_books<S: Scene>(&mut self, scene: &S, positions: &[BlockPos]) {
        for &pos in positions {
            if scene.block(pos).and_then(|block| renderer_of(&block.id.path)) == Some(Renderer::EnchantingTable) {
                self.books.entry(pos).or_default();
            }
        }
    }

    /// The books' ticks (`bookAnimationTick`), with the player's feet
    /// (none for a spectator, whom `getNearestPlayer` passes over).
    pub fn tick_books(&mut self, player: Option<DVec3>) {
        for (pos, book) in &mut self.books {
            let centre = DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)) + 0.5;
            book.tick(centre, player, &mut self.random);
        }
    }

    /// The client level's block entity ticks: `ChestBlockEntity` and
    /// `EnderChestBlockEntity.lidAnimateTick`, `ShulkerBoxBlockEntity.tick`.
    pub fn tick(&mut self) {
        self.lids.retain(|_, lid| match lid {
            Lid::Chest(chest) => {
                chest.tick_lid();
                !chest.at_rest()
            }
            Lid::Shulker(shulker) => {
                shulker.tick();
                *shulker != ShulkerLid::default()
            }
        });
    }

    /// Lids go with their block entities: those whose block is no longer
    /// that kind of container are forgotten.
    pub fn retain_present<S: Scene>(&mut self, scene: &S) {
        self.lids.retain(|&pos, lid| {
            let renderer = scene.block(pos).and_then(|block| renderer_of(&block.id.path));
            matches!((lid, renderer), (Lid::Chest(_), Some(Renderer::Chest(_))) | (Lid::Shulker(_), Some(Renderer::ShulkerBox(_))))
        });
        self.books.retain(|&pos, _| scene.block(pos).and_then(|block| renderer_of(&block.id.path)) == Some(Renderer::EnchantingTable));
    }

    /// `getOpenNess` of the chest at `pos`.
    pub fn chest_openness(&self, pos: BlockPos, partial: f32) -> f32 {
        match self.lids.get(&pos) {
            Some(Lid::Chest(lid)) => lid.openness(partial),
            _ => 0.0,
        }
    }

    /// `getProgress` of the shulker box at `pos`.
    pub fn shulker_progress(&self, pos: BlockPos, partial: f32) -> f32 {
        match self.lids.get(&pos) {
            Some(Lid::Shulker(lid)) => lid.progress(partial),
            _ => 0.0,
        }
    }
}

/// `ChestRenderState.ChestMaterialType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChestMaterial {
    Regular,
    Trapped,
    Christmas,
    EnderChest,
    CopperUnaffected,
    CopperExposed,
    CopperWeathered,
    CopperOxidized,
}

impl ChestMaterial {
    /// `Sheets.chooseSprite`: the material's sheet for a chest of
    /// `chest_type`, the ender chest's whatever it is.
    pub fn sheet(self, chest_type: &str) -> String {
        let base = match self {
            Self::EnderChest => return "minecraft:entity/chest/ender".to_owned(),
            Self::Regular => "normal",
            Self::Trapped => "trapped",
            Self::Christmas => "christmas",
            Self::CopperUnaffected => "copper",
            Self::CopperExposed => "copper_exposed",
            Self::CopperWeathered => "copper_weathered",
            Self::CopperOxidized => "copper_oxidized",
        };
        let half = match chest_type {
            "left" => "_left",
            "right" => "_right",
            _ => "",
        };
        format!("minecraft:entity/chest/{base}{half}")
    }
}

/// The renderer a block's entity has, and what it draws it with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Renderer {
    /// `ChestRenderer`, with the material before Christmas is applied.
    Chest(ChestMaterial),
    /// `ShulkerBoxRenderer`, with the box's dye colour (none: the plain box).
    ShulkerBox(Option<&'static str>),
    /// `EnchantTableRenderer`.
    EnchantingTable,
}

/// `DyeColor`'s names, in order.
const DYES: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown",
    "green", "red", "black",
];

fn renderer_of(path: &str) -> Option<Renderer> {
    Some(Renderer::Chest(match path {
        "chest" => ChestMaterial::Regular,
        "trapped_chest" => ChestMaterial::Trapped,
        "ender_chest" => ChestMaterial::EnderChest,
        "shulker_box" => return Some(Renderer::ShulkerBox(None)),
        "enchanting_table" => return Some(Renderer::EnchantingTable),
        _ => {
            if let Some(color) = path.strip_suffix("_shulker_box") {
                return DYES.iter().find(|dye| **dye == color).map(|dye| Renderer::ShulkerBox(Some(dye)));
            }
            // `CopperChestBlock.getState`: a waxed chest weathers no more,
            // but keeps its state's look.
            match path.strip_prefix("waxed_").unwrap_or(path) {
                "copper_chest" => ChestMaterial::CopperUnaffected,
                "exposed_copper_chest" => ChestMaterial::CopperExposed,
                "weathered_copper_chest" => ChestMaterial::CopperWeathered,
                "oxidized_copper_chest" => ChestMaterial::CopperOxidized,
                _ => return None,
            }
        }
    }))
}

/// Whether a block has one of these renderers.
pub fn renders(block: &Block) -> bool {
    renderer_of(&block.id.path).is_some()
}

/// `SpecialDates.isExtendedChristmas`: 24 to 26 December.
pub fn is_extended_christmas(month: u32, day: u32) -> bool {
    month == 12 && (24..=26).contains(&day)
}

/// The month and day of a Unix time (in UTC: the game has no time zone
/// database for `ZonedDateTime.now`).
pub fn month_day(unix_seconds: i64) -> (u32, u32) {
    // Days to the civil calendar, counting from 1 March 0000.
    let days = unix_seconds.div_euclid(86_400) + 719_468;
    let era_day = days.rem_euclid(146_097);
    let year_of_era = (era_day - era_day / 1460 + era_day / 36_524 - era_day / 146_096) / 365;
    let day_of_year = era_day - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    (month as u32, day as u32)
}

/// Every sheet these renderers draw with, for the world atlas.
pub fn sheets() -> Vec<String> {
    use ChestMaterial::*;
    let mut sheets = vec![EnderChest.sheet("single")];
    for material in [Regular, Trapped, Christmas, CopperUnaffected, CopperExposed, CopperWeathered, CopperOxidized] {
        for chest_type in ["single", "left", "right"] {
            sheets.push(material.sheet(chest_type));
        }
    }
    sheets.push("minecraft:entity/shulker/shulker".to_owned());
    sheets.extend(DYES.iter().map(|dye| format!("minecraft:entity/shulker/shulker_{dye}")));
    sheets.push(BOOK_SHEET.to_owned());
    sheets
}

/// A horizontal direction's `toYRot`, in degrees.
fn y_rot(facing: &str) -> f32 {
    match facing {
        "west" => 90.0,
        "north" => 180.0,
        "east" => 270.0,
        _ => 0.0,
    }
}

/// `Direction.getClockWise`, seen from above.
fn clockwise(facing: &str) -> &'static str {
    match facing {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        _ => "north",
    }
}

fn opposite(facing: &str) -> &'static str {
    clockwise(clockwise(facing))
}

fn step(direction: &str) -> BlockPos {
    match direction {
        "north" => (0, 0, -1),
        "south" => (0, 0, 1),
        "west" => (-1, 0, 0),
        "east" => (1, 0, 0),
        "down" => (0, -1, 0),
        _ => (0, 1, 0),
    }
}

/// `Direction.getRotation`: up turned to `direction`.
fn direction_rotation(direction: &str) -> Quat {
    use std::f32::consts::PI;
    let x = Quat::from_rotation_x(FRAC_PI_2);
    match direction {
        "down" => Quat::from_rotation_x(PI),
        "north" => x * Quat::from_rotation_z(PI),
        "south" => x,
        "west" => x * Quat::from_rotation_z(FRAC_PI_2),
        "east" => x * Quat::from_rotation_z(-FRAC_PI_2),
        _ => Quat::IDENTITY,
    }
}

/// `LevelRenderer.getLightCoords` at a cell: its sky and block light.
fn light_at(light: &SkyLight, pos: BlockPos) -> [f32; 2] {
    [light.get(pos) as f32, light.get_block(pos) as f32]
}

/// Draws the container block entity at `pos` (`extractRenderState` and
/// `submit`), if its block has one of these renderers: a chest into
/// `culled` (`entityCutoutCull`), a shulker box into `cutout`
/// (`entityCutout`), lit by the light at its cell.
#[allow(clippy::too_many_arguments)]
pub fn append_container<S: Scene>(
    culled: &mut ChunkMesh,
    cutout: &mut ChunkMesh,
    scene: &S,
    pos: BlockPos,
    lids: &ContainerLids,
    partial: f32,
    christmas: bool,
    atlas: &Atlas,
    light: &SkyLight,
) {
    let Some(block) = scene.block(pos) else {
        return;
    };
    let property = |name: &str| block.properties.get(name).map(String::as_str);
    let cell = Vec3::new(pos.0 as f32, pos.1 as f32, pos.2 as f32);
    match renderer_of(&block.id.path) {
        Some(Renderer::Chest(material)) => {
            // `getChestMaterial`: copper and ender chests keep their look
            // at Christmas.
            let material = match material {
                ChestMaterial::Regular | ChestMaterial::Trapped if christmas => ChestMaterial::Christmas,
                material => material,
            };
            let chest_type = property("type").unwrap_or("single");
            let facing = property("facing").unwrap_or("north");
            let mut open = lids.chest_openness(pos, partial);
            let mut lit = light_at(light, pos);
            // `ChestBlock.combine`: a half joined to the other half, of the
            // same block with the same facing, opens with the wider open of
            // the two (`opennessCombiner`) and takes the brighter light of
            // each kind (`BrightnessCombiner`).
            if chest_type != "single" {
                let toward = if chest_type == "left" { clockwise(facing) } else { opposite(clockwise(facing)) };
                let (dx, dy, dz) = step(toward);
                let other = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
                let joined = scene.block(other).is_some_and(|neighbour| {
                    let get = |name: &str| neighbour.properties.get(name).map(String::as_str);
                    neighbour.id == block.id
                        && get("type").is_some_and(|other_type| other_type != "single" && other_type != chest_type)
                        && get("facing") == Some(facing)
                });
                if joined {
                    open = open.max(lids.chest_openness(other, partial));
                    let other_lit = light_at(light, other);
                    lit = [lit[0].max(other_lit[0]), lit[1].max(other_lit[1])];
                }
            }
            let Ok(sheet) = ResourceId::parse(&material.sheet(chest_type)) else {
                return;
            };
            if !atlas.contains(&sheet) {
                return;
            }
            // `submit`: eased out, and turned about the block's middle
            // (`modelTransformation`).
            let open = 1.0 - (1.0 - open).powi(3);
            let pose = Mat4::from_translation(cell + Vec3::new(0.5, 0.0, 0.5))
                * Mat4::from_rotation_y((-y_rot(facing)).to_radians())
                * Mat4::from_translation(Vec3::new(-0.5, 0.0, -0.5));
            let mut parts = chest_model(chest_type);
            // `ChestModel.setupAnim`: the lid and lock swing up about the
            // hinge.
            for part in &mut parts[1..] {
                part.rotation[0] = -(open * FRAC_PI_2);
            }
            let region = atlas.entity_region(&sheet);
            for part in &parts {
                emit_part(culled, part, pose, region, [64.0; 2], [1.0; 3], &entity_shade, lit);
            }
        }
        Some(Renderer::ShulkerBox(color)) => {
            let sheet = match color {
                Some(dye) => format!("minecraft:entity/shulker/shulker_{dye}"),
                None => "minecraft:entity/shulker/shulker".to_owned(),
            };
            let Ok(sheet) = ResourceId::parse(&sheet) else {
                return;
            };
            if !atlas.contains(&sheet) {
                return;
            }
            let progress = lids.shulker_progress(pos, partial);
            // `createModelTransform`: the box turned to its facing about its
            // middle, a hair smaller, in the model's flipped space.
            let pose = Mat4::from_translation(cell + Vec3::splat(0.5))
                * Mat4::from_scale(Vec3::splat(0.9995))
                * Mat4::from_quat(direction_rotation(property("facing").unwrap_or("up")))
                * Mat4::from_scale(Vec3::new(1.0, -1.0, -1.0))
                * Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0));
            let mut parts = shulker_box_model();
            // `ShulkerBoxModel.setupAnim`: the lid rises half a block and
            // turns 270 degrees.
            parts[0].offset[1] = 24.0 - progress * 0.5 * 16.0;
            parts[0].rotation[1] = (270.0 * progress).to_radians();
            let region = atlas.entity_region(&sheet);
            for part in &parts {
                emit_part(cutout, part, pose, region, [64.0; 2], [1.0; 3], &entity_shade, light_at(light, pos));
            }
        }
        Some(Renderer::EnchantingTable) => {
            let Ok(sheet) = ResourceId::parse(BOOK_SHEET) else {
                return;
            };
            if !atlas.contains(&sheet) {
                return;
            }
            // `submit`: three quarters up and bobbing, turned to its `rot`
            // and tipped 80 degrees.
            let (time, flip, open, turn) = lids.books.get(&pos).map_or((partial, 0.0, 0.0, 0.0), |book| book.pose(partial));
            let pose = Mat4::from_translation(cell + Vec3::new(0.5, 0.75, 0.5))
                * Mat4::from_translation(Vec3::new(0.0, 0.1 + (time * 0.1).sin() * 0.01, 0.0))
                * Mat4::from_rotation_y(-turn)
                * Mat4::from_rotation_z(80f32.to_radians());
            let state = book_state(time, flip, open);
            append_book(culled, pose, atlas.entity_region(&sheet), &entity_shade, light_at(light, pos), state);
        }
        None => {}
    }
}

/// The containers in the loaded chunks near the camera that
/// `BlockEntityRenderer.shouldRender` lets through: their middles within
/// 64 blocks (`getViewDistance`).
pub fn containers_near(scene: &crate::scene::HandcraftedScene, camera: DVec3) -> Vec<BlockPos> {
    const VIEW_DISTANCE: f64 = 64.0;
    let reach = (VIEW_DISTANCE as i32 >> 4) + 1;
    let (cx, cz) = ((camera.x.floor() as i32) >> 4, (camera.z.floor() as i32) >> 4);
    let mut found = Vec::new();
    for x in cx - reach..=cx + reach {
        for z in cz - reach..=cz + reach {
            found.extend(scene.block_entity_cells((x, z)).filter(|&pos| {
                let middle = DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)) + 0.5;
                middle.distance_squared(camera) < VIEW_DISTANCE * VIEW_DISTANCE && scene.block(pos).is_some_and(renders)
            }));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::HandcraftedScene;

    #[test]
    fn a_tables_book_opens_and_turns_to_a_near_player() {
        let mut book = TableBook::default();
        let mut random = LegacyRandom::new(0);
        let centre = DVec3::new(0.5, 64.5, 0.5);
        // Two blocks south: `tRot` is atan2(dz, dx), a quarter turn.
        let near = Some(DVec3::new(0.5, 64.0, 2.5));
        book.tick(centre, near, &mut random);
        assert!(book.flip_t != 0.0, "opening leafs it");
        for _ in 0..20 {
            book.tick(centre, near, &mut random);
        }
        let (time, _, open, turn) = book.pose(1.0);
        assert_eq!((time, open), (22.0, 1.0));
        assert!((turn - FRAC_PI_2).abs() < 1e-3, "{turn}");
        // Three blocks off it closes, a tenth a tick, and turns on slowly.
        let far = Some(DVec3::new(0.5, 64.0, 3.6));
        book.tick(centre, far, &mut random);
        assert!((book.pose(1.0).2 - 0.9).abs() < 1e-6);
        for _ in 0..10 {
            book.tick(centre, far, &mut random);
        }
        assert_eq!(book.pose(1.0).2, 0.0);
        // `forAnimation`: the pages a quarter and three quarters on.
        let [openness, first, second] = book_state(0.0, 0.0, 1.0);
        assert!((openness - 1.25).abs() < 1e-6 && (first - 0.1).abs() < 1e-6 && (second - 0.9).abs() < 1e-6);
        assert_eq!(book_state(0.0, 2.0, 0.0)[0], 0.0);
    }

    #[test]
    fn chest_lid_steps_a_tenth_a_tick_and_eases() {
        let mut lid = ChestLidController::default();
        lid.should_be_open(true);
        lid.tick_lid();
        assert!((lid.openness(0.0) - 0.0).abs() < 1e-6 && (lid.openness(1.0) - 0.1).abs() < 1e-6);
        assert!((lid.openness(0.5) - 0.05).abs() < 1e-6);
        for _ in 0..12 {
            lid.tick_lid();
        }
        assert_eq!(lid.openness(0.3), 1.0);
        lid.should_be_open(false);
        lid.tick_lid();
        assert!((lid.openness(1.0) - 0.9).abs() < 1e-6);
        for _ in 0..12 {
            lid.tick_lid();
        }
        assert_eq!(lid.openness(1.0), 0.0);
        assert!(lid.at_rest());
    }

    #[test]
    fn block_events_open_and_close_the_lids() {
        let mut lids = ContainerLids::default();
        let (chest, ender, shulker) = ((0, 0, 0), (2, 0, 0), (4, 0, 0));
        lids.block_event(chest, "minecraft:chest", 1, 1);
        lids.block_event(ender, "minecraft:ender_chest", 1, 2);
        lids.block_event(shulker, "minecraft:red_shulker_box", 1, 1);
        // A note block's event is not a lid's.
        lids.block_event((6, 0, 0), "minecraft:note_block", 0, 0);
        for _ in 0..5 {
            lids.tick();
        }
        assert!((lids.chest_openness(chest, 1.0) - 0.5).abs() < 1e-5);
        assert!((lids.chest_openness(ender, 1.0) - 0.5).abs() < 1e-5);
        assert!((lids.shulker_progress(shulker, 1.0) - 0.5).abs() < 1e-5);
        for _ in 0..6 {
            lids.tick();
        }
        assert_eq!(lids.chest_openness(chest, 0.0), 1.0);
        assert_eq!(lids.shulker_progress(shulker, 0.0), 1.0);
        assert_eq!(lids.lids.len(), 3);
        lids.block_event(chest, "minecraft:chest", 1, 0);
        lids.block_event(ender, "minecraft:ender_chest", 1, 0);
        lids.block_event(shulker, "minecraft:red_shulker_box", 1, 0);
        for _ in 0..12 {
            lids.tick();
        }
        assert_eq!(lids.chest_openness(chest, 1.0), 0.0);
        assert_eq!(lids.shulker_progress(shulker, 1.0), 0.0);
        // Shut and still, they are forgotten.
        assert!(lids.lids.is_empty());
    }

    #[test]
    fn shulker_lid_runs_through_its_states() {
        let mut lid = ShulkerLid::default();
        // Two openers: only the first starts it.
        lid.trigger(2);
        lid.tick();
        assert_eq!((lid.status, lid.progress(1.0)), (ShulkerAnimation::Closed, 0.0));
        lid.trigger(1);
        let mut ticks = 0;
        while lid.status != ShulkerAnimation::Opened {
            lid.tick();
            ticks += 1;
        }
        assert_eq!((ticks, lid.progress(1.0)), (10, 1.0));
        lid.trigger(0);
        lid.tick();
        assert_eq!(lid.status, ShulkerAnimation::Closing);
        assert!((lid.progress(0.5) - 0.95).abs() < 1e-6);
    }

    #[test]
    fn christmas_and_dates() {
        assert_eq!(month_day(0), (1, 1));
        // 2026-12-24T12:00Z and 2024-02-29.
        assert_eq!(month_day(1_798_113_600), (12, 24));
        assert_eq!(month_day(1_709_164_800), (2, 29));
        assert!(is_extended_christmas(12, 24) && is_extended_christmas(12, 26));
        assert!(!is_extended_christmas(12, 23) && !is_extended_christmas(12, 27) && !is_extended_christmas(1, 25));
        assert_eq!(ChestMaterial::Regular.sheet("left"), "minecraft:entity/chest/normal_left");
        assert_eq!(ChestMaterial::EnderChest.sheet("right"), "minecraft:entity/chest/ender");
        assert_eq!(renderer_of("waxed_weathered_copper_chest"), Some(Renderer::Chest(ChestMaterial::CopperWeathered)));
        assert_eq!(renderer_of("light_blue_shulker_box"), Some(Renderer::ShulkerBox(Some("light_blue"))));
        assert_eq!(renderer_of("barrel"), None);
        assert_eq!(sheets().len(), 1 + 7 * 3 + 17 + 1);
    }

    /// An atlas holding every sheet over the whole texture, a light map of
    /// one scene, and the vertices one container draws.
    fn draw(scene: &HandcraftedScene, pos: BlockPos, lids: &ContainerLids) -> (ChunkMesh, ChunkMesh) {
        let atlas = Atlas::with_slots_for_tests(sheets().iter().map(|sheet| (ResourceId::parse(sheet).unwrap(), [0.0, 0.0, 1.0, 1.0])));
        let light = SkyLight::build(scene);
        let (mut culled, mut cutout) = (ChunkMesh::default(), ChunkMesh::default());
        append_container(&mut culled, &mut cutout, scene, pos, lids, 1.0, false, &atlas, &light);
        (culled, cutout)
    }

    fn bounds(mesh: &ChunkMesh) -> (Vec3, Vec3) {
        mesh.vertices.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| {
            let p = Vec3::from_array(v.position);
            (lo.min(p), hi.max(p))
        })
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).abs().max_element() < 1e-4
    }

    #[test]
    fn a_tables_book_floats_over_it() {
        let mut scene = HandcraftedScene::default();
        let pos = (3, 5, -2);
        scene.set(pos, Some(Block::new("minecraft:enchanting_table")));
        let mut lids = ContainerLids::default();
        lids.track_books(&scene, &[pos]);
        assert_eq!(lids.books.len(), 1);
        let (culled, cutout) = draw(&scene, pos, &lids);
        // Seven cuboids of six faces, over the table's top, in its cell.
        assert!(cutout.vertices.is_empty());
        assert_eq!(culled.faces, 42);
        let (low, high) = bounds(&culled);
        assert!(low.y > 5.75 && high.y < 6.5 && low.x > 3.0 && high.x < 4.0 && low.z > -2.0 && high.z < -1.0, "{low} {high}");
        // A book goes with its table.
        scene.set(pos, None);
        lids.retain_present(&scene);
        assert!(lids.books.is_empty());
    }

    #[test]
    fn chests_sit_in_their_cell_turned_to_their_facing() {
        let mut scene = HandcraftedScene::default();
        let pos = (3, 5, -2);
        let cell = Vec3::new(3.0, 5.0, -2.0);
        for (facing, lock_low, lock_high) in [
            ("south", Vec3::new(7.0, 7.0, 15.0), Vec3::new(9.0, 11.0, 16.0)),
            ("north", Vec3::new(7.0, 7.0, 0.0), Vec3::new(9.0, 11.0, 1.0)),
            ("east", Vec3::new(15.0, 7.0, 7.0), Vec3::new(16.0, 11.0, 9.0)),
            ("west", Vec3::new(0.0, 7.0, 7.0), Vec3::new(1.0, 11.0, 9.0)),
        ] {
            scene.set(pos, Some(Block::new("minecraft:chest").with("facing", facing).with("type", "single")));
            let (culled, cutout) = draw(&scene, pos, &ContainerLids::default());
            assert!(cutout.vertices.is_empty());
            // Bottom, lid and lock: six faces each.
            assert_eq!(culled.faces, 18);
            let (lo, hi) = bounds(&culled);
            let (body_lo, body_hi) = (cell + Vec3::new(1.0, 0.0, 1.0) / 16.0, cell + Vec3::new(15.0, 14.0, 15.0) / 16.0);
            assert!(lo.cmpge(body_lo.min(cell + lock_low / 16.0) - 1e-4).all(), "{facing} {lo}");
            assert!(hi.cmple(body_hi.max(cell + lock_high / 16.0) + 1e-4).all(), "{facing} {hi}");
            // The lock's vertices, the last 24, on the front.
            let (lock_lo, lock_hi) = bounds(&ChunkMesh { vertices: culled.vertices[48..].to_vec(), ..ChunkMesh::default() });
            assert!(near(lock_lo, cell + lock_low / 16.0) && near(lock_hi, cell + lock_high / 16.0), "{facing} {lock_lo} {lock_hi}");
        }
    }

    #[test]
    fn an_open_lid_stands_up_over_the_hinge() {
        let mut scene = HandcraftedScene::default();
        let pos = (0, 0, 0);
        scene.set(pos, Some(Block::new("minecraft:trapped_chest").with("facing", "south").with("type", "single")));
        let mut lids = ContainerLids::default();
        lids.block_event(pos, "minecraft:trapped_chest", 1, 1);
        for _ in 0..10 {
            lids.tick();
        }
        let (culled, _) = draw(&scene, pos, &lids);
        let lid = bounds(&ChunkMesh { vertices: culled.vertices[24..48].to_vec(), ..ChunkMesh::default() });
        // Turned up about the hinge at 9 up, 1 in: 14 tall, 5 thick behind it.
        assert!(near(lid.0, Vec3::new(1.0, 9.0, -4.0) / 16.0) && near(lid.1, Vec3::new(15.0, 23.0, 1.0) / 16.0), "{lid:?}");
    }

    #[test]
    fn double_chest_halves_meet_without_their_join_faces() {
        let mut scene = HandcraftedScene::default();
        // Facing north, the left half's partner is to its east.
        let (left, right) = ((0, 0, 0), (1, 0, 0));
        scene.set(left, Some(Block::new("minecraft:copper_chest").with("facing", "north").with("type", "left")));
        scene.set(right, Some(Block::new("minecraft:copper_chest").with("facing", "north").with("type", "right")));
        let mut lids = ContainerLids::default();
        // Only the right half opened: both lids go up together.
        lids.block_event(right, "minecraft:copper_chest", 1, 1);
        for _ in 0..10 {
            lids.tick();
        }
        let (a, _) = draw(&scene, left, &lids);
        let (b, _) = draw(&scene, right, &lids);
        assert_eq!((a.faces, b.faces), (15, 15));
        let ((a_lo, a_hi), (b_lo, b_hi)) = (bounds(&a), bounds(&b));
        assert!((a_hi.x - 1.0).abs() < 1e-4 && (a_lo.x - 1.0 / 16.0).abs() < 1e-4, "{a_lo} {a_hi}");
        assert!((b_lo.x - 1.0).abs() < 1e-4 && (b_hi.x - 31.0 / 16.0).abs() < 1e-4, "{b_lo} {b_hi}");
        // Open, the lock rises highest: 15 over the hinge.
        assert!((a_hi.y - 24.0 / 16.0).abs() < 1e-4 && (b_hi.y - 24.0 / 16.0).abs() < 1e-4, "{a_hi} {b_hi}");
    }

    #[test]
    fn shulker_boxes_open_along_their_facing() {
        let mut scene = HandcraftedScene::default();
        let pos = (0, 0, 0);
        scene.set(pos, Some(Block::new("minecraft:blue_shulker_box").with("facing", "up")));
        let (culled, closed) = draw(&scene, pos, &ContainerLids::default());
        assert!(culled.vertices.is_empty());
        assert_eq!(closed.faces, 12);
        let (lo, hi) = bounds(&closed);
        let shrink = |v: f32| 0.5 + (v - 0.5) * 0.9995;
        assert!(near(lo, Vec3::splat(shrink(0.0))) && near(hi, Vec3::splat(shrink(1.0))), "{lo} {hi}");
        let mut lids = ContainerLids::default();
        lids.block_event(pos, "minecraft:blue_shulker_box", 1, 1);
        for _ in 0..10 {
            lids.tick();
        }
        let (_, open) = draw(&scene, pos, &lids);
        // The lid, the first 24, half a block up and turned a quarter short
        // of a full turn: still square over the base.
        let (lid_lo, lid_hi) = bounds(&ChunkMesh { vertices: open.vertices[..24].to_vec(), ..ChunkMesh::default() });
        assert!(near(lid_lo, Vec3::new(shrink(0.0), shrink(0.75), shrink(0.0))) && near(lid_hi, Vec3::new(shrink(1.0), shrink(1.5), shrink(1.0))), "{lid_lo} {lid_hi}");
        // Facing west, the box lies on its side and opens west.
        scene.set(pos, Some(Block::new("minecraft:blue_shulker_box").with("facing", "west")));
        let (_, side) = draw(&scene, pos, &lids);
        let (lid_lo, lid_hi) = bounds(&ChunkMesh { vertices: side.vertices[..24].to_vec(), ..ChunkMesh::default() });
        assert!(near(lid_lo, Vec3::new(shrink(-0.5), shrink(0.0), shrink(0.0))) && near(lid_hi, Vec3::new(shrink(0.25), shrink(1.0), shrink(1.0))), "{lid_lo} {lid_hi}");
    }
}
