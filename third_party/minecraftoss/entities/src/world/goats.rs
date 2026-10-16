//! Goats (26.3 `Goat`, `GoatAi`'s ram activity: `PrepareRamNearestTarget`
//! and `RamTarget`).
//!
//! - Every half minute to five (a screaming goat's five to fifteen
//!   seconds) a goat picks the nearest living thing it can see that is not
//!   a goat, walks to a spot four to seven blocks straight out from it,
//!   lowers its head for a second and charges at three times its pace.
//! - Whatever it meets takes 2 and a shove of 2.5 (a kid's 1) in the
//!   charge's line, more the faster it ran; a charge into stone, logs, ore
//!   or packed ice snaps off a horn instead. A target that moves before the
//!   charge makes it start over; one it cannot line up with waits for the
//!   shorter cooldown.
//! - A bucket on a grown goat fills with milk.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{MonsterGoalContext, Target};
use crate::navigation::navigate_walk_to;
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// A goat's horns and its ram.
#[derive(Clone, Debug)]
pub struct GoatState {
    pub screaming: bool,
    pub left_horn: bool,
    pub right_horn: bool,
    /// `RAM_COOLDOWN_TICKS`.
    pub ram_cooldown: i32,
    /// Event 58 / 59: its head going down for the charge.
    pub lowering_head: bool,
    pub lower_head_tick: i32,
    pub lower_head_tick_o: i32,
    /// What its ram did this tick: whom it struck and the line of the
    /// charge, and whether it snapped a horn.
    pub hit: Option<(Target, DVec3)>,
    pub snapped: bool,
    /// The prepare sound this tick.
    pub prepared: bool,
    pub ready: bool,
}

impl Default for GoatState {
    fn default() -> Self {
        Self { screaming: false, left_horn: true, right_horn: true, ram_cooldown: 0, lowering_head: false, lower_head_tick: 0, lower_head_tick_o: 0, hit: None, snapped: false, prepared: false, ready: false }
    }
}

impl GoatState {
    /// `getRammingXHeadRot` between ticks.
    pub fn ramming_head(&self, baby: bool, partial: f32) -> f32 {
        let tick = self.lower_head_tick_o as f32 + (self.lower_head_tick - self.lower_head_tick_o) as f32 * partial;
        tick / 20.0 * if baby { 52.5 } else { 30.0 } * std::f32::consts::PI / 180.0
    }

    /// `TIME_BETWEEN_RAMS` (a screamer's `TIME_BETWEEN_RAMS_SCREAMER`).
    fn time_between_rams(&self, random: &mut LegacyRandom) -> i32 {
        if self.screaming {
            100 + random.next_int(201) as i32
        } else {
            600 + random.next_int(5401) as i32
        }
    }
}

/// What the ram goal shares.
#[derive(Clone, Debug, Default)]
pub struct GoatCtx {
    pub cooldown: i32,
    pub screaming: bool,
    pub lowering: Option<bool>,
    pub hit: Option<(Target, DVec3)>,
    pub snapped: bool,
    pub prepared: bool,
    pub failed: bool,
    pub finished: bool,
}

/// `GoatAi`'s activities as goals: panic, love, temptation, the ram, and
/// idling.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, crate::monster_ai::FloatGoal);
    selector.add(1, crate::mob_ai::PanicGoal::new(2.0));
    selector.add(2, crate::mob_ai::BreedGoal::new());
    selector.add(3, crate::mob_ai::TemptGoal::new(1.25));
    selector.add(4, GoatRamGoal { phase: RamPhase::Idle, start: (0, 0, 0), target_cell: (0, 0, 0), target: None, reached_at: None, ram_to: DVec3::ZERO, direction: DVec3::ZERO, ticks: 0 });
    selector.add(5, crate::monster_ai::StrollGoal::new(1.0, 0.001));
    selector.add(6, crate::monster_ai::LookAtPlayerGoal { range: 6.0 });
    selector.add(7, crate::monster_ai::RandomLookGoal);
    selector
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RamPhase {
    Idle,
    Prepare,
    Charge,
}

/// `PrepareRamNearestTarget(…, 4, 7, 1.25, …, 20, …)` then
/// `RamTarget(…, 3.0, …)`.
#[derive(Clone)]
struct GoatRamGoal {
    phase: RamPhase,
    start: Cell,
    target_cell: Cell,
    target: Option<Target>,
    reached_at: Option<i32>,
    ram_to: DVec3,
    direction: DVec3,
    ticks: i32,
}

fn cell_of(p: DVec3) -> Cell {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

/// `isWalkableBlock`: a stable spot (solid below, room above) with no
/// malus.
fn walkable(world: &dyn World, c: Cell) -> bool {
    world.solid((c.0, c.1 - 1, c.2)) && !world.solid(c) && !world.solid((c.0, c.1 + 1, c.2)) && world.block(c).is_none_or(|b| !b.id.contains("water") && !b.id.contains("lava"))
}

impl GoatRamGoal {
    /// The nearest seen living thing that is no goat.
    fn nearest(ctx: &mut MonsterGoalContext, world: &dyn World) -> Option<Target> {
        let here = ctx.body.position;
        let mut options: Vec<(f64, Target)> = Vec::new();
        for p in ctx.players.clone() {
            if p.alive && p.attackable && p.position.distance_squared(here) <= 16.0 * 16.0 {
                options.push((p.position.distance_squared(here), Target::Player(p.id)));
            }
        }
        for m in ctx.mobs.clone() {
            if m.alive && m.kind != "minecraft:goat" && m.position.distance_squared(here) <= 16.0 * 16.0 {
                options.push((m.position.distance_squared(here), Target::Mob(m.id)));
            }
        }
        options.sort_by(|a, b| a.0.total_cmp(&b.0));
        options.into_iter().map(|(_, t)| t).find(|&t| ctx.info(t).is_some_and(|info| ctx.sees(world, info)))
    }

    /// `calculateRammingStartPosition`: the farthest walkable spot out from
    /// the target along each side, four or more away, nearest the goat.
    fn start_for(ctx: &MonsterGoalContext, world: &dyn World, target: Cell) -> Option<Cell> {
        if !walkable(world, target) {
            return None;
        }
        let mut options = Vec::new();
        for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            let mut c = target;
            for _ in 0..7 {
                let next = (c.0 + dx, c.1, c.2 + dz);
                if !walkable(world, next) {
                    break;
                }
                c = next;
            }
            if (c.0 - target.0).abs() + (c.2 - target.2).abs() >= 4 {
                options.push(c);
            }
        }
        let here = cell_of(ctx.body.position);
        options.into_iter().min_by_key(|c| (c.0 - here.0).pow(2) + (c.1 - here.1).pow(2) + (c.2 - here.2).pow(2))
    }

    fn choose(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World, target: Target) -> bool {
        self.reached_at = None;
        let Some(info) = ctx.info(target) else { return false };
        let target_cell = cell_of(info.position);
        match Self::start_for(ctx, world, target_cell) {
            Some(start) => {
                self.start = start;
                self.target_cell = target_cell;
                self.target = Some(target);
                true
            }
            None => false,
        }
    }

    fn walk(ctx: &mut MonsterGoalContext, world: &dyn World, to: DVec3, speed: f64) {
        let profile = ctx.profile();
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, speed, 0);
    }
}

impl Goal<MonsterGoalContext> for GoatRamGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        // Not while tempted or breeding (`TEMPTING_PLAYER`, `BREED_TARGET`).
        if ctx.goat.cooldown > 0 || ctx.in_love || !ctx.tempters.is_empty() {
            return false;
        }
        let Some(target) = Self::nearest(ctx, world) else {
            ctx.goat.failed = true;
            return false;
        };
        if self.choose(ctx, world, target) {
            self.phase = RamPhase::Prepare;
            self.ticks = 0;
            true
        } else {
            ctx.goat.failed = true;
            false
        }
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        match self.phase {
            RamPhase::Prepare => self.ticks < 160 && self.target.is_some_and(|t| ctx.info(t).is_some_and(|i| i.alive)),
            RamPhase::Charge => self.ticks < 200,
            RamPhase::Idle => false,
        }
    }
    fn interruptible(&self) -> bool {
        false
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        match self.phase {
            RamPhase::Prepare => ctx.goat.failed = true,
            RamPhase::Charge => ctx.goat.finished = true,
            RamPhase::Idle => {}
        }
        ctx.goat.lowering = Some(false);
        self.phase = RamPhase::Idle;
        ctx.navigation.stop();
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.ticks += 1;
        match self.phase {
            RamPhase::Prepare => {
                let Some(target) = self.target else { return };
                let Some(info) = ctx.info(target) else { return };
                let start = DVec3::new(f64::from(self.start.0) + 0.5, f64::from(self.start.1), f64::from(self.start.2) + 0.5);
                ctx.look_control.set_look_at(info.position + DVec3::new(0.0, f64::from(info.eye_height), 0.0));
                if cell_of(info.position) != self.target_cell {
                    // It moved: head up, and line up anew.
                    ctx.goat.lowering = Some(false);
                    ctx.navigation.stop();
                    if !self.choose(ctx, world, target) {
                        self.target = None;
                    }
                    return;
                }
                if cell_of(ctx.body.position) == self.start {
                    ctx.goat.lowering = Some(true);
                    let at = *self.reached_at.get_or_insert(ctx.tick_count);
                    if ctx.tick_count - at >= 20 {
                        // `getEdgeOfBlock`: the near edge of its block.
                        let (sx, sz) = ((self.target_cell.0 - self.start.0).signum(), (self.target_cell.2 - self.start.2).signum());
                        self.ram_to = DVec3::new(f64::from(self.target_cell.0) + 0.5 + 0.5 * f64::from(sx), f64::from(self.target_cell.1), f64::from(self.target_cell.2) + 0.5 + 0.5 * f64::from(sz));
                        ctx.goat.prepared = true;
                        // `RamTarget.start`.
                        let here = cell_of(ctx.body.position);
                        self.direction = DVec3::new(f64::from(here.0) - self.ram_to.x, 0.0, f64::from(here.2) - self.ram_to.z).normalize_or_zero();
                        self.phase = RamPhase::Charge;
                        self.ticks = 0;
                        Self::walk(ctx, world, self.ram_to, 3.0);
                    }
                } else if ctx.navigation.is_done() {
                    Self::walk(ctx, world, start, 1.25);
                }
            }
            RamPhase::Charge => {
                // Whoever its box meets is struck.
                let b = &ctx.body;
                let half = f64::from(b.width) / 2.0;
                let p = b.position;
                let touching = |q: DVec3, w: f32, h: f32| {
                    let qh = f64::from(w) / 2.0;
                    (q.x - p.x).abs() < half + qh && (q.z - p.z).abs() < half + qh && q.y < p.y + f64::from(b.height) && q.y + f64::from(h) > p.y
                };
                let struck = ctx.players.iter().find(|pl| pl.alive && pl.attackable && touching(pl.position, 0.6, 1.8)).map(|pl| Target::Player(pl.id)).or_else(|| {
                    ctx.mobs.iter().find(|m| m.alive && m.kind != "minecraft:goat" && touching(m.position, m.width, m.height)).map(|m| Target::Mob(m.id))
                });
                if let Some(t) = struck {
                    ctx.goat.hit = Some((t, -self.direction));
                    self.phase = RamPhase::Idle;
                    ctx.goat.finished = true;
                    ctx.goat.lowering = Some(false);
                    return;
                }
                // `hasRammedHornBreakingBlock`.
                let v = ctx.body.velocity;
                let flat = DVec3::new(v.x, 0.0, v.z).normalize_or_zero();
                let ahead = cell_of(p + flat);
                if world.block_in_tag(ahead, "minecraft:snaps_goat_horn") || world.block_in_tag((ahead.0, ahead.1 + 1, ahead.2), "minecraft:snaps_goat_horn") {
                    ctx.goat.snapped = true;
                    self.phase = RamPhase::Idle;
                    ctx.goat.finished = true;
                    ctx.goat.lowering = Some(false);
                    return;
                }
                if ctx.navigation.is_done() || p.distance_squared(self.ram_to) < 0.25 * 0.25 {
                    self.phase = RamPhase::Idle;
                    ctx.goat.finished = true;
                    ctx.goat.lowering = Some(false);
                }
            }
            RamPhase::Idle => {}
        }
    }
}

impl MobEntity {
    /// Before its goals: the ram's cooldown (`CountDownCooldownTicks`).
    pub(super) fn goat_prepare(&mut self) {
        if self.kind != Kind::Goat {
            return;
        }
        let g = &mut self.goat;
        if !g.ready {
            // `GoatAi.initMemories`.
            g.ready = true;
            g.ram_cooldown = g.time_between_rams(&mut self.random);
        }
        g.ram_cooldown = (g.ram_cooldown - 1).max(0);
        // `Goat.aiStep`: the head's lowering.
        g.lower_head_tick_o = g.lower_head_tick;
        g.lower_head_tick = if g.lowering_head { g.lower_head_tick + 1 } else { g.lower_head_tick - 2 }.clamp(0, 20);
        let ctx = &mut self.ai.state.goat;
        ctx.cooldown = g.ram_cooldown;
        ctx.screaming = g.screaming;
    }

    /// After its goals: the ram's outcome and its cooldowns.
    pub(super) fn goat_settle(&mut self) {
        if self.kind != Kind::Goat {
            return;
        }
        let position = self.body.position;
        let ctx = std::mem::take(&mut self.ai.state.goat);
        let g = &mut self.goat;
        if let Some(lowering) = ctx.lowering {
            g.lowering_head = lowering;
        }
        if ctx.prepared {
            let pitch = (self.random.next_float() - self.random.next_float()) * 0.2 + 1.0;
            let sound = if g.screaming { "entity.goat.screaming.prepare_ram" } else { "entity.goat.prepare_ram" };
            self.voices.push((Voice::Event(sound, 1.0, pitch), position));
        }
        if ctx.failed {
            // `PrepareRamNearestTarget.stop`: the shorter wait.
            g.ram_cooldown = if g.screaming { 100 } else { 600 };
        }
        if ctx.finished {
            g.ram_cooldown = g.time_between_rams(&mut self.random);
        }
        g.hit = ctx.hit;
        g.snapped = ctx.snapped;
    }

    /// `Goat.dropHorn`: one of its horns, as a goat horn.
    fn drop_horn(&mut self) -> Option<ItemStack> {
        if self.baby() || !(self.goat.left_horn || self.goat.right_horn) {
            return None;
        }
        let left = if !self.goat.left_horn { false } else if !self.goat.right_horn { true } else { self.random.next_boolean() };
        if left {
            self.goat.left_horn = false;
        } else {
            self.goat.right_horn = false;
        }
        // `createHorn`: an instrument from its kind's horns.
        const REGULAR: [&str; 4] = ["ponder", "sing", "seek", "feel"];
        const SCREAMING: [&str; 4] = ["admire", "call", "yearn", "dream"];
        let set = if self.goat.screaming { SCREAMING } else { REGULAR };
        let pick = set[self.random.next_int(4) as usize];
        let mut horn = ItemStack::new("minecraft:goat_horn", 1);
        horn.components = Some(serde_json::json!({ "minecraft:instrument": format!("minecraft:{pick}_goat_horn") }));
        Some(horn)
    }
}

impl EntityWorld {
    /// The ram's blow (`noAggroMobAttack` for 2, and the shove) or its
    /// snapped horn.
    pub(super) fn goat_step(&mut self, id: u64, world: &mut impl World) {
        let Some(e) = self.mob_mut(id) else { return };
        let (hit, snapped) = (e.goat.hit.take(), std::mem::take(&mut e.goat.snapped));
        let position = e.body.position;
        let impact = if e.goat.screaming { "entity.goat.screaming.ram_impact" } else { "entity.goat.ram_impact" };
        if snapped {
            e.voices.push((Voice::Event(impact, 1.0, 1.0), position));
            if let Some(horn) = e.drop_horn() {
                e.voices.push((Voice::Event("item.goat_horn.break", 1.0, 1.0), position));
                let (a, b, c) = (e.random.next_float(), e.random.next_float(), e.random.next_float());
                let velocity = DVec3::new(f64::from(a * 0.4 - 0.2), f64::from(0.3 + b * 0.4), f64::from(c * 0.4 - 0.2));
                world.spawn_item(position, &horn, velocity, 0);
            }
        }
        let Some((target, line)) = hit else { return };
        let force = if e.baby() { 1.0 } else { 2.5 };
        // `speedFactor`: its pace now, a little more for Speed.
        let speed = e.body.velocity.x.hypot(e.body.velocity.z) as f32;
        let factor = f64::from((speed * 1.65 * 3.0 / 0.3).clamp(0.2, 3.0));
        let power = force * factor;
        e.voices.push((Voice::Event(impact, 1.0, 1.0), position));
        let damage = Kind::Goat.species().attack;
        match target {
            Target::Player(player_id) => {
                let yaw = (-line.x).atan2(line.z).to_degrees() as f32;
                let weapon = MeleeWeapon { knockback: power as f32, yaw, ..MeleeWeapon::default() };
                self.player_hits.push(PlayerHit { player_id, damage, kind: PlayerHitKind::Melee { attacker: position, hunger_ticks: 0, lift: 0.0, weapon, effect: None }, source: Some(id) });
            }
            Target::Mob(victim) | Target::Villager(victim) => {
                let _ = self.mob_hits_mob(id, victim, damage, position, 0.0);
                self.knock_back(victim, power, -line.x, -line.z);
            }
        }
    }

    /// `Goat.mobInteract` with a bucket: milk from a grown goat. Whether it
    /// gave any; the caller swaps the bucket.
    pub fn milk_goat(&mut self, id: u64) -> bool {
        let Some(e) = self.mob_mut(id) else { return false };
        if e.kind != Kind::Goat || e.baby() || e.health <= 0.0 {
            return false;
        }
        let sound = if e.goat.screaming { "entity.goat.screaming.milk" } else { "entity.goat.milk" };
        let position = e.body.position;
        e.voices.push((Voice::Event(sound, 1.0, 1.0), position));
        true
    }
}
