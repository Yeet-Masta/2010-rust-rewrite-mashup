//! Beacons on the server level (26.3 `BeaconBlockEntity`): the beam
//! checked ten blocks a tick up to the world's surface, coloured by the
//! stained glass it passes and stopped by a block that takes all light;
//! the pyramid's levels from `#beacon_base_blocks`, every 80 game ticks;
//! the chosen powers, saved; and the activate, deactivate, ambient and
//! power-select sounds.
//!
//! [`BeaconBeam`] runs on any block lookup, so a client's beacons check
//! their beams as vanilla's client does.
//!
//! The powers go to the players in range every 80 game ticks
//! (`applyEffects`) as [`BeaconEffect`]s, for the player's own status
//! effects. Not simulated: the `construct_beacon` advancement.

use super::Level;
use minecraftoss_core::chunk::HeightmapKind;
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::BlockPos;
use minecraftoss_generator::feature::World;

/// `BeaconBlockEntity.VALID_EFFECTS`: every tier's powers.
pub const VALID_EFFECTS: [&str; 6] = [
    "minecraft:speed",
    "minecraft:haste",
    "minecraft:resistance",
    "minecraft:jump_boost",
    "minecraft:strength",
    "minecraft:regeneration",
];

/// `SoundEvents.BEACON_*`.
const ACTIVATE: &str = "minecraft:block.beacon.activate";
const DEACTIVATE: &str = "minecraft:block.beacon.deactivate";
const AMBIENT: &str = "minecraft:block.beacon.ambient";
const POWER_SELECT: &str = "minecraft:block.beacon.power_select";

/// `DyeColor.getTextureDiffuseColor` (ARGB) of a colour's name.
fn diffuse(name: &str) -> Option<u32> {
    let rgb = match name {
        "white" => 0xF9FFFE,
        "orange" => 0xF9801D,
        "magenta" => 0xC74EBD,
        "light_blue" => 0x3AB3DA,
        "yellow" => 0xFED83D,
        "lime" => 0x80C71F,
        "pink" => 0xF38BAA,
        "gray" => 0x474F52,
        "light_gray" => 0x9D9D97,
        "cyan" => 0x169C9C,
        "purple" => 0x8932B8,
        "blue" => 0x3C44AA,
        "brown" => 0x835432,
        "green" => 0x5E7C16,
        "red" => 0xB02E26,
        "black" => 0x1D1D21,
        _ => return None,
    };
    Some(0xFF00_0000 | rgb)
}

/// `BeaconBeamBlock.getColor` of a block by its id: a beacon's white, a
/// stained glass block's or pane's dye.
pub fn beam_tint(block: &str) -> Option<u32> {
    let path = block.strip_prefix("minecraft:").unwrap_or(block);
    if path == "beacon" {
        return diffuse("white");
    }
    let colour = path.strip_suffix("_stained_glass_pane").or_else(|| path.strip_suffix("_stained_glass"))?;
    diffuse(colour)
}

/// `ARGB.average`.
fn average(a: u32, b: u32) -> u32 {
    (0..4).map(|i| i * 8).fold(0, |colour, shift| colour | ((((a >> shift) & 255) + ((b >> shift) & 255)) / 2) << shift)
}

/// What a block does to a beam passing up through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeamBlock {
    /// A `BeaconBeamBlock`: its colour.
    Tint(u32),
    /// It lets the beam through (it takes less than all light, or it is
    /// bedrock).
    Pass,
    /// It stops the beam.
    Stop,
}

impl BeamBlock {
    /// The beam's rule for a block by its id and `getLightDampening`.
    pub fn of(block: &str, light_dampening: u8) -> Self {
        match beam_tint(block) {
            Some(colour) => Self::Tint(colour),
            None if light_dampening >= 15 && block != "minecraft:bedrock" => Self::Stop,
            None => Self::Pass,
        }
    }
}

/// `BeaconBeamOwner.Section`: a colour and how many blocks it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeamSection {
    pub color: u32,
    pub height: i32,
}

/// What `BeaconBlockEntity` keeps without saving: the beam as last checked
/// (`beamSections`) and as being checked (`checkingBeamSections`,
/// `lastCheckY`), and the pyramid's `levels`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BeaconBeam {
    sections: Vec<BeamSection>,
    checking: Vec<BeamSection>,
    last_check_y: i32,
    pub levels: i32,
}

/// A power a beacon gives a player (`applyEffects`' `MobEffectInstance`,
/// ambient and visible): the player's index in `Level.players`, the
/// effect, its duration in ticks and its amplifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeaconEffect {
    pub player: usize,
    pub effect: &'static str,
    pub duration: i32,
    pub amplifier: i32,
}

/// `applyEffects`: with a primary power, it goes to every player whose box
/// is within `levels * 10 + 10` blocks of the beacon's (up to the world's
/// height above), for `(9 + levels * 2) * 20` ticks, level II when it is
/// also the secondary at four levels; another secondary, at four levels,
/// goes to them too. `players` are the players' feet.
pub fn beacon_effects(pos: BlockPos, levels: i32, beacon: &Beacon, world_height: i32, players: &[[f64; 3]]) -> Vec<BeaconEffect> {
    let static_of = |effect: &Option<String>| effect.as_deref().and_then(|effect| VALID_EFFECTS.iter().copied().find(|valid| *valid == effect));
    let (Some(primary), secondary) = (static_of(&beacon.primary), static_of(&beacon.secondary)) else {
        return Vec::new();
    };
    let range = f64::from(levels * 10 + 10);
    let amplifier = i32::from(levels >= 4 && secondary == Some(primary));
    let duration = (9 + levels * 2) * 20;
    // `new AABB(pos).inflate(range).expandTowards(0, height, 0)` against the
    // players' boxes (0.6 wide, 1.8 high), by `AABB.intersects`.
    let [x, y, z] = [pos.x, pos.y, pos.z].map(f64::from);
    let (min, max) = ([x - range, y - range, z - range], [x + 1.0 + range, y + 1.0 + range + f64::from(world_height), z + 1.0 + range]);
    let inside = |[fx, fy, fz]: [f64; 3]| {
        let (low, high) = ([fx - 0.3, fy, fz - 0.3], [fx + 0.3, fy + 1.8, fz + 0.3]);
        (0..3).all(|a| min[a] < high[a] && max[a] > low[a])
    };
    let near: Vec<usize> = (0..players.len()).filter(|&i| inside(players[i])).collect();
    let mut effects: Vec<BeaconEffect> = near.iter().map(|&player| BeaconEffect { player, effect: primary, duration, amplifier }).collect();
    if let Some(secondary) = secondary.filter(|&secondary| levels >= 4 && secondary != primary) {
        effects.extend(near.iter().map(|&player| BeaconEffect { player, effect: secondary, duration, amplifier: 0 }));
    }
    effects
}

/// What a tick did that is heard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BeaconTicked {
    /// Every 80 game ticks while it shines (`BEACON_AMBIENT`), when the
    /// powers are given.
    pub ambient: bool,
    /// A check ended with it newly lit, or newly out.
    pub activated: bool,
    pub deactivated: bool,
}

impl BeaconBeam {
    /// `setLevel`: no check begun.
    pub fn new(min_y: i32) -> Self {
        Self { last_check_y: min_y - 1, ..Self::default() }
    }

    /// `getBeamSections`: none without a pyramid.
    pub fn sections(&self) -> &[BeamSection] {
        if self.levels == 0 {
            &[]
        } else {
            &self.sections
        }
    }

    /// Whether the last check found a beam at all (`beamSections`).
    pub fn has_beam(&self) -> bool {
        !self.sections.is_empty()
    }

    /// `BeaconBlockEntity.tick` for a beacon at `pos`: up to ten more
    /// blocks of the beam checked, to the column's `surface`
    /// (`WORLD_SURFACE`); every 80 game ticks, with a beam, the pyramid's
    /// levels (`updateBase`, `base` saying which blocks are
    /// `#beacon_base_blocks`); and at the end of a check, the beam found.
    pub fn tick(
        &mut self,
        pos: BlockPos,
        game_time: i64,
        surface: i32,
        min_y: i32,
        block: impl Fn(i32) -> BeamBlock,
        base: impl Fn(BlockPos) -> bool,
    ) -> BeaconTicked {
        let first = if self.last_check_y < pos.y {
            self.checking.clear();
            self.last_check_y = pos.y - 1;
            pos.y
        } else {
            self.last_check_y + 1
        };
        let mut last = self.checking.len().checked_sub(1);
        for check in first..first + 10 {
            if check > surface {
                break;
            }
            match block(check) {
                BeamBlock::Tint(color) => {
                    if self.checking.len() <= 1 {
                        self.checking.push(BeamSection { color, height: 1 });
                        last = Some(self.checking.len() - 1);
                    } else if let Some(index) = last {
                        let section = self.checking[index];
                        if section.color == color {
                            self.checking[index].height += 1;
                        } else {
                            self.checking.push(BeamSection { color: average(section.color, color), height: 1 });
                            last = Some(self.checking.len() - 1);
                        }
                    }
                }
                other => {
                    let Some(index) = last.filter(|_| other == BeamBlock::Pass) else {
                        self.checking.clear();
                        self.last_check_y = surface;
                        break;
                    };
                    self.checking[index].height += 1;
                }
            }
            self.last_check_y += 1;
        }
        let mut ticked = BeaconTicked::default();
        let previous_levels = self.levels;
        if game_time % 80 == 0 {
            if !self.sections.is_empty() {
                self.levels = pyramid_levels(pos, min_y, base);
            }
            ticked.ambient = self.levels > 0 && !self.sections.is_empty();
        }
        if self.last_check_y >= surface {
            self.last_check_y = min_y - 1;
            self.sections = std::mem::take(&mut self.checking);
            let (was, is) = (previous_levels > 0, self.levels > 0);
            ticked.activated = !was && is;
            ticked.deactivated = was && !is;
        }
        ticked
    }
}

/// `updateBase`: how many layers of `#beacon_base_blocks` under the beacon
/// are whole, each a block wider on every side, up to four.
pub fn pyramid_levels(pos: BlockPos, min_y: i32, base: impl Fn(BlockPos) -> bool) -> i32 {
    let mut levels = 0;
    for step in 1..=4 {
        let y = pos.y - step;
        if y < min_y {
            break;
        }
        let whole = (pos.x - step..=pos.x + step).all(|x| (pos.z - step..=pos.z + step).all(|z| base(BlockPos::new(x, y, z))));
        if !whole {
            break;
        }
        levels = step;
    }
    levels
}

/// A beacon block entity's saved powers (`primary_effect`,
/// `secondary_effect`), each one of [`VALID_EFFECTS`] or none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Beacon {
    pub primary: Option<String>,
    pub secondary: Option<String>,
}

/// `filterEffect`.
fn valid(effect: Option<&str>) -> Option<String> {
    effect.filter(|effect| VALID_EFFECTS.contains(effect)).map(str::to_owned)
}

impl Beacon {
    /// `loadAdditional`'s powers.
    pub fn from_tag(tag: &Tag) -> Self {
        let effect = |key: &str| valid(tag.get(key).and_then(Tag::as_str));
        Self { primary: effect("primary_effect"), secondary: effect("secondary_effect") }
    }
}

impl Level<'_> {
    /// The beacon block entity at a position: its saved powers.
    pub fn beacon(&self, pos: BlockPos) -> Option<Beacon> {
        if !self.is_a(self.block(pos), "BeaconBlock") {
            return None;
        }
        self.block_entity(pos).map(Beacon::from_tag)
    }

    /// The beacon's menu data: its levels (none until its first tick of the
    /// 80 since it loaded) and its powers.
    pub fn beacon_data(&self, pos: BlockPos) -> Option<(i32, Beacon)> {
        let beacon = self.beacon(pos)?;
        Some((self.beacons.get(&pos).map_or(0, |beam| beam.levels), beacon))
    }

    /// The beacon's `dataAccess.set` for its powers, from its menu: the
    /// power-select sound while it has a beam, and the powers saved
    /// (`filterEffect`).
    pub fn beacon_set_effects(&mut self, pos: BlockPos, primary: Option<&str>, secondary: Option<&str>) {
        if self.beacon(pos).is_none() {
            return;
        }
        if self.beacons.get(&pos).is_some_and(BeaconBeam::has_beam) {
            self.play_sound(POWER_SELECT, centre(pos), 1.0, 1.0);
        }
        if let Some(Tag::Compound(map)) = self.block_entity_mut(pos) {
            for (key, effect) in [("primary_effect", valid(primary)), ("secondary_effect", valid(secondary))] {
                match effect {
                    Some(effect) => map.insert(key.to_owned(), Tag::String(effect)),
                    None => map.remove(key),
                };
            }
        }
    }

    /// `BeamBlock::of` the block at a position.
    fn beam_block(&self, pos: BlockPos) -> BeamBlock {
        let state = self.block(pos);
        let blocks = &self.registries().blocks;
        BeamBlock::of(blocks.block(blocks.block_of(state)).name.as_str(), blocks.state(state).light_dampening)
    }

    /// `BeaconBlockEntity.tick` on the server: the beam and levels, the
    /// saved `Levels` when they change, and the sounds.
    pub(super) fn beacon_tick(&mut self, pos: BlockPos) {
        if self.beacon(pos).is_none() {
            return;
        }
        let min_y = self.min_y();
        let mut beam = self.beacons.remove(&pos).unwrap_or_else(|| BeaconBeam::new(min_y));
        let surface = self.height_at(HeightmapKind::WorldSurface, pos.x, pos.z);
        let registries = self.registries();
        let tag = registries.block_tags.require("minecraft:beacon_base_blocks").ok();
        let base = |p: BlockPos| tag.is_some_and(|tag| registries.block_in_tag(self.block(p), tag));
        let before = beam.levels;
        let ticked = beam.tick(pos, self.game_time, surface, min_y, |y| self.beam_block(BlockPos::new(pos.x, y, pos.z)), base);
        let levels = beam.levels;
        self.beacons.insert(pos, beam);
        if levels != before
            && let Some(Tag::Compound(map)) = self.block_entity_mut(pos)
        {
            map.insert("Levels".to_owned(), Tag::Int(levels));
        }
        if ticked.ambient
            && let Some(beacon) = self.beacon(pos)
        {
            let height = self.max_y() - min_y + 1;
            let effects = beacon_effects(pos, levels, &beacon, height, &self.players);
            self.beacon_effects.extend(effects);
        }
        for (heard, event) in [(ticked.ambient, AMBIENT), (ticked.activated, ACTIVATE), (ticked.deactivated, DEACTIVATE)] {
            if heard {
                self.play_sound(event, centre(pos), 1.0, 1.0);
            }
        }
    }

    /// The powers the beacons gave since the last call.
    pub fn take_beacon_effects(&mut self) -> Vec<BeaconEffect> {
        std::mem::take(&mut self.beacon_effects)
    }

    /// `BeaconBlockEntity.setRemoved`: a beacon that ticked goes out.
    pub(super) fn beacon_removed(&mut self, pos: BlockPos) {
        if self.beacons.remove(&pos).is_some() {
            self.play_sound(DEACTIVATE, centre(pos), 1.0, 1.0);
        }
    }
}

/// `Vec3.atCenterOf`, where `playSound` at a block plays.
fn centre(pos: BlockPos) -> [f64; 3] {
    [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_beam_takes_the_glass_colours_and_shines_over_a_pyramid() {
        let pos = BlockPos::new(0, 10, 0);
        let red = beam_tint("minecraft:red_stained_glass").unwrap();
        let blue = beam_tint("minecraft:blue_stained_glass_pane").unwrap();
        // The beacon, two red panes of glass, blue, air to the surface at 15:
        // a whole check a tick.
        let column = |y: i32| match y {
            10 => BeamBlock::Tint(beam_tint("minecraft:beacon").unwrap()),
            11 | 12 => BeamBlock::Tint(red),
            13 => BeamBlock::Tint(blue),
            _ => BeamBlock::Pass,
        };
        // A two-layer pyramid of base blocks.
        let base = |p: BlockPos| p.y >= 8 && p.y < 10 && p.x.abs() <= 10 - p.y && p.z.abs() <= 10 - p.y;
        let mut beam = BeaconBeam::new(0);
        let heard: Vec<_> = (1..=80).map(|time| beam.tick(pos, time, 15, 0, column, base)).collect();
        // The first glass takes its own colour; the next is mixed in.
        let sections = [(0xFFF9FFFE, 1), (red, 2), (average(red, blue), 3)];
        assert_eq!(beam.sections(), sections.map(|(color, height)| BeamSection { color, height }));
        assert_eq!(beam.levels, 2);
        assert_eq!(heard[79], BeaconTicked { ambient: true, activated: true, deactivated: false });
        assert_eq!(heard[78], BeaconTicked::default(), "unlit before");
        // A block that takes all light stops it.
        let stopped = |y: i32| if y == 12 { BeamBlock::Stop } else { column(y) };
        beam.tick(pos, 81, 15, 0, stopped, base);
        assert_eq!(beam.sections(), []);
        // Four levels give speed II to a player 50 blocks off, not 51.
        let speed = Beacon { primary: Some("minecraft:speed".to_owned()), secondary: Some("minecraft:speed".to_owned()) };
        let players = [[50.5, 10.0, 0.5], [51.5, 10.0, 0.5]];
        let given = BeaconEffect { player: 0, effect: "minecraft:speed", duration: 340, amplifier: 1 };
        assert_eq!(beacon_effects(pos, 4, &speed, 384, &players), [given]);
    }
}
