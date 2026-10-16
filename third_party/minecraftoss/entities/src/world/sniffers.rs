//! Sniffers (26.3 `Sniffer`, `SnifferAi`'s sniff and dig activities).
//!
//! - Idle, a grown sniffer now and then sniffs the air for two to four
//!   seconds; done, it picks a diggable block (grass, dirt, moss, mud and
//!   the like it has not dug before, among its last twenty) at most ten
//!   to eighteen blocks off and searches its way there.
//! - There it digs for eight or nine seconds, turning up a torchflower
//!   seed or a pitcher pod six seconds in, rises over two seconds, is
//!   happy for a few more, and will not dig again for eight minutes.
//! - Bred, it lays an egg instead of a sniffling.
use super::*;
use super::mobs::MobEntity;
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// `Sniffer.State`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SnifferPhase {
    #[default]
    Idling,
    FeelingHappy,
    Scenting,
    Sniffing,
    Searching,
    Digging,
    Rising,
}

/// A sniffer's memories and doings.
#[derive(Clone, Debug, Default)]
pub struct SnifferState {
    pub phase: SnifferPhase,
    pub phase_start: i32,
    /// The ticks left of what it does now (its behaviour's run).
    pub left: i32,
    /// `SNIFF_COOLDOWN`, `SNIFFER_DIGGING`, `SNIFFER_SNIFFING_TARGET`,
    /// `SNIFFER_HAPPY`, `SNIFFER_EXPLORED_POSITIONS`.
    pub cooldown: i32,
    pub digging: bool,
    pub target: Option<Cell>,
    pub happy: bool,
    pub explored: Vec<Cell>,
    /// `DATA_DROP_SEED_AT_TICK`.
    pub drop_seed_at: Option<i32>,
    /// A seed turned up this tick, at its head.
    pub seed: Option<Cell>,
    /// Ticks left before `RunOne` picks again.
    pub idle_pick: i32,
}

impl SnifferState {
    /// Whether it holds still for what it does.
    pub fn busy(&self) -> bool {
        matches!(self.phase, SnifferPhase::Scenting | SnifferPhase::Sniffing | SnifferPhase::Digging | SnifferPhase::Rising | SnifferPhase::FeelingHappy)
    }
}

impl MobEntity {
    fn sniffer_to(&mut self, phase: SnifferPhase) {
        let at = self.body.position;
        let sound = match phase {
            SnifferPhase::FeelingHappy => Some(("entity.sniffer.happy", 1.0)),
            SnifferPhase::Scenting => Some(("entity.sniffer.scenting", if self.age.baby() { 1.3 } else { 1.0 })),
            SnifferPhase::Sniffing => Some(("entity.sniffer.sniffing", 1.0)),
            SnifferPhase::Rising => Some(("entity.sniffer.digging_stop", 1.0)),
            _ => None,
        };
        if let Some((sound, pitch)) = sound {
            self.voices.push((Voice::Event(sound, 1.0, pitch), at));
        }
        if phase == SnifferPhase::Digging {
            self.sniffer.drop_seed_at = Some(self.tick_count + 120);
        }
        self.sniffer.phase = phase;
        self.sniffer.phase_start = self.tick_count;
    }

    /// `resetSniffing`.
    fn sniffer_reset(&mut self) {
        self.sniffer.digging = false;
        self.sniffer.target = None;
        self.sniffer_to(SnifferPhase::Idling);
    }

    /// `getHeadBlock`: 2.25 ahead, at its feet.
    fn sniffer_head(&self) -> Cell {
        let yaw = self.yaw.to_radians();
        let p = self.body.position + DVec3::new(f64::from(-yaw.sin()), 0.0, f64::from(yaw.cos())) * 2.25;
        (p.x.floor() as i32, (self.body.position.y + f64::from(0.2_f32)).floor() as i32, p.z.floor() as i32)
    }

    fn sniffer_diggable(&self, world: &dyn World, c: Cell) -> bool {
        world.block_in_tag(c, "minecraft:sniffer_diggable_block") && !self.sniffer.explored.contains(&c)
    }

    /// `canDig`.
    fn sniffer_can_dig(&self, world: &dyn World, panicking: bool) -> bool {
        let head = self.sniffer_head();
        !panicking && !self.age.baby() && !self.body.touching_water && self.body.on_ground && self.sniffer_diggable(world, (head.0, head.1 - 1, head.2))
    }

    /// `canSniff`.
    fn sniffer_can_sniff(&self, panicking: bool, leashed: bool) -> bool {
        !panicking && !self.body.touching_water && self.in_love == 0 && self.body.on_ground && !leashed
    }

    /// The brain before the goals (`SnifferAi`'s DIG, SNIFF and IDLE).
    pub(super) fn sniffer_prepare(&mut self, world: &dyn World, leashed: bool) {
        if self.kind != Kind::Sniffer {
            return;
        }
        let panicking = crate::mob_ai::panicking(&self.ai.state);
        if panicking && (self.sniffer.digging || self.sniffer.target.is_some() || self.sniffer.phase != SnifferPhase::Idling) {
            self.sniffer_reset();
        }
        let s = &mut self.sniffer;
        s.cooldown = (s.cooldown - 1).max(0);
        s.left -= 1;
        if panicking {
            return;
        }
        let can_sniff = self.sniffer_can_sniff(panicking, leashed);
        // DIG: `Digging(160, 180)` then `FinishedDigging(40)`.
        if self.sniffer.digging && self.sniffer.target.is_none() {
            match self.sniffer.phase {
                SnifferPhase::Digging => {
                    let can_dig = self.sniffer_can_dig(world, panicking);
                    if self.sniffer.left <= 0 {
                        self.sniffer.cooldown = 9600;
                        self.sniffer_to(SnifferPhase::Idling);
                    } else if !can_dig || self.in_love > 0 {
                        self.sniffer_reset();
                    }
                }
                SnifferPhase::Rising => {
                    if self.sniffer.left <= 0 {
                        // `onDiggingComplete(true)`: the block it stands on.
                        let p = self.body.position;
                        let on = (p.x.floor() as i32, (p.y - 0.2).floor() as i32, p.z.floor() as i32);
                        self.sniffer.explored.insert(0, on);
                        self.sniffer.explored.truncate(21);
                        self.sniffer_to(SnifferPhase::Idling);
                        self.sniffer.digging = false;
                        self.sniffer.happy = true;
                    }
                }
                _ if self.sniffer.cooldown > 0 => {
                    self.sniffer.left = 40;
                    self.sniffer_to(SnifferPhase::Rising);
                }
                _ if can_sniff => {
                    self.sniffer.left = 160 + self.random.next_int(21) as i32;
                    self.sniffer_to(SnifferPhase::Digging);
                }
                _ => {}
            }
            self.ai.state.navigation.stop();
            return;
        }
        // SNIFF: `Searching`, the walk to its target.
        if let Some(target) = self.sniffer.target {
            if self.sniffer.phase != SnifferPhase::Searching {
                if can_sniff {
                    self.sniffer.left = 600;
                    self.sniffer_to(SnifferPhase::Searching);
                    let to = DVec3::new(f64::from(target.0) + 0.5, f64::from(target.1 + 1), f64::from(target.2) + 0.5);
                    let profile = self.ai.state.profile();
                    let fluid = self.ai.state.fluid;
                    let _ = crate::navigation::navigate_walk_to(&self.body, &mut self.ai.state.navigation, world, &profile, fluid, to, 1.25, 0);
                }
                return;
            }
            if !can_sniff {
                self.sniffer_to(SnifferPhase::Idling);
                self.sniffer.target = None;
                return;
            }
            if self.ai.state.navigation.is_done() || self.sniffer.left <= 0 {
                if self.sniffer_can_dig(world, panicking) {
                    self.sniffer.digging = true;
                }
                self.sniffer.target = None;
                self.ai.state.navigation.stop();
                self.sniffer_to(SnifferPhase::Idling);
            }
            return;
        }
        // IDLE: `FeelingHappy`, then `RunOne`'s scent or sniff.
        match self.sniffer.phase {
            SnifferPhase::FeelingHappy | SnifferPhase::Scenting => {
                if self.sniffer.left <= 0 {
                    self.sniffer.happy = false;
                    self.sniffer_to(SnifferPhase::Idling);
                }
            }
            SnifferPhase::Sniffing => {
                if !can_sniff {
                    self.sniffer_to(SnifferPhase::Idling);
                } else if self.sniffer.left <= 0 {
                    self.sniffer_to(SnifferPhase::Idling);
                    // `calculateDigPosition`: five tries further out.
                    let profile = self.ai.state.profile();
                    for i in 0..5 {
                        let Some(at) = crate::stroll::land_random_position(world, &profile, self.body.position, 10 + 2 * i, 3, &mut self.random) else { continue };
                        let c = (at.x.floor() as i32, at.y.floor() as i32 - 1, at.z.floor() as i32);
                        if self.sniffer_diggable(world, c) {
                            self.sniffer.target = Some(c);
                            break;
                        }
                    }
                }
            }
            _ if self.sniffer.happy => {
                self.sniffer.left = 40 + self.random.next_int(61) as i32;
                self.sniffer_to(SnifferPhase::FeelingHappy);
            }
            _ => {
                self.sniffer.idle_pick -= 1;
                if self.sniffer.idle_pick > 0 || !self.ai.state.navigation.is_done() {
                    return;
                }
                self.sniffer.idle_pick = 20 + self.random.next_int(41) as i32;
                // `RunOne`: two of nine walk, one scents, one sniffs, the
                // rest look about, stroll or rest.
                match self.random.next_int(9) {
                    2 => {
                        self.sniffer.left = 40 + self.random.next_int(41) as i32;
                        self.sniffer_to(SnifferPhase::Scenting);
                    }
                    3 if !self.age.baby() && can_sniff && self.sniffer.cooldown == 0 => {
                        self.sniffer.left = 40 + self.random.next_int(41) as i32;
                        self.sniffer_to(SnifferPhase::Sniffing);
                    }
                    _ => {}
                }
            }
        }
    }

    /// `Sniffer.tick`: the searching snuffle and the seed it digs up.
    pub(super) fn sniffer_settle(&mut self) {
        if self.kind != Kind::Sniffer {
            return;
        }
        let at = self.body.position;
        if self.sniffer.phase == SnifferPhase::Searching && self.tick_count % 20 == 0 {
            self.voices.push((Voice::Event("entity.sniffer.searching", 1.0, 1.0), at));
        }
        if self.sniffer.phase == SnifferPhase::Digging && self.sniffer.drop_seed_at == Some(self.tick_count) {
            self.sniffer.seed = Some(self.sniffer_head());
            self.voices.push((Voice::Event("entity.sniffer.drop_seed", 1.0, 1.0), at));
        }
    }
}

impl EntityWorld {
    /// The seed dug up (`SNIFFER_DIGGING`): one of the two ancient plants,
    /// at the corner of its head's block.
    pub(super) fn sniffer_step(&mut self, id: u64) {
        let Some(e) = self.mob_mut(id) else { return };
        let Some(c) = e.sniffer.seed.take() else { return };
        let item = if e.random.next_boolean() { "minecraft:torchflower_seeds" } else { "minecraft:pitcher_pod" };
        self.spawned.items.push((DVec3::new(f64::from(c.0), f64::from(c.1), f64::from(c.2)), ItemStack::new(item, 1)));
    }
}
