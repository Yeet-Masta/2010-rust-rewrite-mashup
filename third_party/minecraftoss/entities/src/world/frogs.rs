//! Frogs and tadpoles (26.3 `Frog`, `FrogAi`, `ShootTongue`, `Croak`,
//! `TryLaySpawnOnFluidNearLand` and `Tadpole`).
//!
//! - A frog eats the smallest slimes and magma cubes within ten blocks:
//!   it hops up to within 1.75 blocks, flicks its tongue (pulling the cube
//!   in), and six ticks later swallows it with a 10-damage bite. A magma
//!   cube a frog eats leaves a froglight of the frog's kind (the loot
//!   table's `frog/variant` source condition).
//! - Idle ashore it strolls, or croaks for three seconds (the throat sac
//!   shows only then).
//! - Bred, it carries spawn instead of a baby, and lays it on water beside
//!   the bank it stands on.
//! - A tadpole grows up over 20 minutes (a slime ball shortens that by a
//!   tenth of what is left) and becomes a frog of the kind its biome
//!   gives: warm, cold or temperate.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{LookAtPlayerGoal, MonsterGoalContext};
use crate::navigation::navigate_walk_to;
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// `Tadpole.ticksToBeFrog`.
pub const TICKS_TO_BE_FROG: i32 = 24000;
/// `Croak.CROAK_TICKS`.
const CROAK_TICKS: i32 = 60;

/// A frog's tongue, croak and spawn, or a tadpole's growing.
#[derive(Clone, Debug, Default)]
pub struct FrogState {
    /// A tadpole's `age`, from 0 up to [`TICKS_TO_BE_FROG`].
    pub growth: i32,
    /// `IS_PREGNANT`.
    pub pregnant: bool,
    /// The ticks its tongue and croak animations started at
    /// (`tongueAnimationState`, `croakAnimationState`).
    pub tongue_start: Option<i32>,
    pub croak_start: Option<i32>,
    /// What its goals asked this tick: a cube pulled in, a cube eaten, a
    /// spawn laid, and a tadpole that came of age.
    pub pull: Option<(u64, DVec3)>,
    pub eat: Option<u64>,
    pub laid: Option<Cell>,
    pub grown: bool,
}

/// `FrogAi`'s idle and tongue activities as goals.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(0, crate::mob_ai::PanicGoal::new(2.0));
    selector.add(0, crate::mob_ai::BreedGoal::new());
    selector.add(1, crate::mob_ai::TemptGoal::new(1.25));
    selector.add(2, ShootTongueGoal { target: None, state: TongueState::Done, path_counter: 0, timer: 0, next_scan: 0, ran: 0 });
    selector.add(3, LaySpawnGoal { target: None });
    selector.add(4, FrogIdleGoal { croak: 0 });
    selector.add(5, LookAtPlayerGoal { range: 6.0 });
    selector
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TongueState {
    MoveToTarget,
    CatchAnimation,
    EatAnimation,
    Done,
}

/// `StartAttacking` from `FrogAttackablesSensor` and `ShootTongue`.
#[derive(Clone)]
struct ShootTongueGoal {
    target: Option<u64>,
    state: TongueState,
    path_counter: i32,
    timer: i32,
    next_scan: i32,
    /// Ticks run, against `ShootTongue`'s 100-tick limit.
    ran: i32,
}

impl ShootTongueGoal {
    fn target_at(ctx: &MonsterGoalContext, id: u64) -> Option<DVec3> {
        ctx.frog_food.iter().find(|(food, _)| *food == id).map(|&(_, at)| at)
    }
}

impl Goal<MonsterGoalContext> for ShootTongueGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        // Not while panicking or breeding (`canAttack`, `IS_PANICKING`).
        if ctx.in_love || ctx.hurt_by.is_some_and(|(_, when)| ctx.tick_count - when < 40) {
            return false;
        }
        if self.next_scan > 0 {
            self.next_scan -= 1;
            return false;
        }
        self.next_scan = 20;
        let here = ctx.body.position;
        self.target = ctx
            .frog_food
            .iter()
            .filter(|(_, at)| at.distance_squared(here) < 100.0)
            .min_by(|a, b| a.1.distance_squared(here).total_cmp(&b.1.distance_squared(here)))
            .map(|&(id, _)| id);
        self.target.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        self.ran < 100 && self.state != TongueState::Done && self.target.is_some_and(|id| Self::target_at(ctx, id).is_some())
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.ran = 0;
        self.state = TongueState::MoveToTarget;
        self.path_counter = 10;
        let Some(at) = self.target.and_then(|id| Self::target_at(ctx, id)) else { return };
        ctx.look_control.set_look_at(at);
        let profile = ctx.profile();
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, at, 2.0, 0);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.target = None;
        self.state = TongueState::Done;
        ctx.frog_tongue = false;
        ctx.navigation.stop();
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.ran += 1;
        let (Some(id), Some(at)) = (self.target, self.target.and_then(|id| Self::target_at(ctx, id))) else { return };
        match self.state {
            TongueState::MoveToTarget => {
                if at.distance(ctx.body.position) < 1.75 {
                    // The tongue flicks out and draws the cube in.
                    ctx.frog_tongue = true;
                    ctx.navigation.stop();
                    let pull = (ctx.body.position - at).normalize_or_zero() * 0.75;
                    ctx.frog_pull = Some((id, pull));
                    self.timer = 0;
                    self.state = TongueState::CatchAnimation;
                } else if self.path_counter <= 0 {
                    let profile = ctx.profile();
                    let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, at, 2.0, 0);
                    self.path_counter = 10;
                } else {
                    self.path_counter -= 1;
                }
            }
            TongueState::CatchAnimation => {
                let t = self.timer;
                self.timer += 1;
                if t >= 6 {
                    self.state = TongueState::EatAnimation;
                    ctx.frog_eat = Some(id);
                }
            }
            TongueState::EatAnimation => {
                if self.timer >= 10 {
                    self.state = TongueState::Done;
                } else {
                    self.timer += 1;
                }
            }
            TongueState::Done => {}
        }
    }
}

/// The `LAY_SPAWN` activity with `TryLaySpawnOnFluidNearLand`: pregnant,
/// to water, then onto the bank beside it to lay.
#[derive(Clone)]
struct LaySpawnGoal {
    target: Option<Cell>,
}

/// Water still enough for spawn, with air above (`#supports_frogspawn`).
fn spawn_water(world: &dyn World, c: Cell) -> bool {
    let water = world.block(c).is_some_and(|b| b.id == "minecraft:water" && b.property("level").is_none_or(|l| l == "0"));
    let air = world.block((c.0, c.1 + 1, c.2)).is_none_or(|b| matches!(b.id.as_str(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"));
    water && air
}

impl Goal<MonsterGoalContext> for LaySpawnGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if !ctx.frog_pregnant || ctx.random.next_int(20) != 0 {
            return false;
        }
        let p = ctx.body.position;
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let mut best: Option<(i32, Cell)> = None;
        for dx in -8..=8 {
            for dz in -8..=8 {
                for dy in -2..=1 {
                    let c = (x + dx, y + dy - 1, z + dz);
                    if spawn_water(world, c) {
                        let d = dx * dx + dz * dz + dy * dy;
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, c));
                        }
                    }
                }
            }
        }
        self.target = best.map(|(_, c)| c);
        self.target.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.frog_pregnant && self.target.is_some()
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(c) = self.target else { return };
        let profile = ctx.profile();
        let to = DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1 + 1), f64::from(c.2) + 0.5);
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, 1.0, 1);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        // `TryLaySpawnOnFluidNearLand`: ashore, still water beside the block
        // below, with air over it, takes the spawn.
        if ctx.fluid.in_water() || !ctx.body.on_ground {
            if ctx.navigation.is_done() {
                self.target = None;
            }
            return;
        }
        let p = ctx.body.position;
        let below = (p.x.floor() as i32, p.y.floor() as i32 - 1, p.z.floor() as i32);
        for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            let c = (below.0 + dx, below.1, below.2 + dz);
            if spawn_water(world, c) {
                ctx.frog_laid = Some((c.0, c.1 + 1, c.2));
                ctx.frog_pregnant = false;
                self.target = None;
                return;
            }
        }
        if ctx.navigation.is_done() {
            self.target = None;
        }
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.target = None;
        ctx.navigation.stop();
    }
}

/// The idle `RunOne` ashore: a stroll (1 in 7, or 2 with the walk towards
/// what it looks at), a croak (3), or nothing (2).
#[derive(Clone)]
struct FrogIdleGoal {
    croak: i32,
}

impl Goal<MonsterGoalContext> for FrogIdleGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if !ctx.navigation.is_done() || ctx.frog_tongue {
            return false;
        }
        match ctx.random.next_int(7) {
            0 | 1 => {
                let profile = ctx.profile();
                let Some(spot) = crate::stroll::default_random_position(world, &profile, ctx.body.position, 10, 7, &mut ctx.random) else { return false };
                let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, spot, 1.0, 0);
                self.croak = 0;
                true
            }
            // `Croak.start`: not in a liquid.
            2..=4 if !ctx.fluid.in_water() && !ctx.fluid.in_lava() => {
                self.croak = CROAK_TICKS;
                ctx.frog_croaking = true;
                true
            }
            _ => false,
        }
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        if self.croak > 0 {
            true
        } else {
            !ctx.navigation.is_done()
        }
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, _ctx: &mut MonsterGoalContext) {
        if self.croak > 0 {
            self.croak -= 1;
        }
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.croak = 0;
        ctx.frog_croaking = false;
    }
}

/// The frog kind a biome breeds (`FrogVariants`' spawn conditions over
/// `#spawns_warm_variant_frogs` and `#spawns_cold_variant_frogs`).
pub fn variant_for_biome(biome: &str) -> &'static str {
    const WARM: [&str; 17] = [
        "minecraft:desert", "minecraft:warm_ocean", "minecraft:jungle", "minecraft:sparse_jungle", "minecraft:bamboo_jungle", "minecraft:savanna", "minecraft:savanna_plateau", "minecraft:windswept_savanna",
        "minecraft:nether_wastes", "minecraft:soul_sand_valley", "minecraft:crimson_forest", "minecraft:warped_forest", "minecraft:basalt_deltas", "minecraft:badlands", "minecraft:eroded_badlands", "minecraft:wooded_badlands", "minecraft:mangrove_swamp",
    ];
    const COLD: [&str; 17] = [
        "minecraft:snowy_plains", "minecraft:ice_spikes", "minecraft:frozen_peaks", "minecraft:jagged_peaks", "minecraft:snowy_slopes", "minecraft:frozen_ocean", "minecraft:deep_frozen_ocean", "minecraft:grove", "minecraft:deep_dark",
        "minecraft:frozen_river", "minecraft:snowy_taiga", "minecraft:snowy_beach", "minecraft:the_end", "minecraft:end_highlands", "minecraft:end_midlands", "minecraft:small_end_islands", "minecraft:end_barrens",
    ];
    if WARM.contains(&biome) {
        "minecraft:warm"
    } else if COLD.contains(&biome) {
        "minecraft:cold"
    } else {
        "minecraft:temperate"
    }
}

/// The frog variant as a loot condition names it.
pub fn variant_key(variant: &str) -> &'static str {
    match variant {
        "minecraft:warm" | "warm" => "minecraft:warm",
        "minecraft:cold" | "cold" => "minecraft:cold",
        _ => "minecraft:temperate",
    }
}

impl MobEntity {
    /// Before its goals: what a frog sees of itself.
    pub(super) fn frog_prepare(&mut self, food: Vec<(u64, DVec3)>) {
        let state = &mut self.ai.state;
        state.frog_food = food;
        state.frog_pregnant = self.frog.pregnant;
    }

    /// After its goals: its animations and what they asked of the world.
    pub(super) fn frog_settle(&mut self) {
        let state = &mut self.ai.state;
        self.frog.pregnant = state.frog_pregnant;
        self.frog.pull = state.frog_pull.take();
        self.frog.eat = state.frog_eat.take();
        self.frog.laid = state.frog_laid.take();
        let position = self.body.position;
        // `Pose.USING_TONGUE` and `Pose.CROAKING` start their animations.
        if state.frog_tongue {
            if self.frog.tongue_start.is_none() {
                self.frog.tongue_start = Some(self.tick_count);
                self.voices.push((Voice::Event("entity.frog.tongue", 2.0, 1.0), position));
            }
        } else {
            self.frog.tongue_start = None;
        }
        if state.frog_croaking {
            self.frog.croak_start.get_or_insert(self.tick_count);
        } else {
            self.frog.croak_start = None;
        }
    }

    /// `Tadpole.aiStep`: a tick older, a frog at last.
    pub(super) fn tadpole_grow(&mut self) {
        if self.kind != Kind::Tadpole || self.age.locked || self.health <= 0.0 {
            return;
        }
        self.frog.growth += 1;
        if self.frog.growth >= TICKS_TO_BE_FROG {
            self.frog.grown = true;
        }
    }

    /// `Tadpole.feed` with a slime ball: a tenth of the time left goes.
    pub fn feed_tadpole(&mut self) {
        let left = (TICKS_TO_BE_FROG - self.frog.growth).max(0);
        let seconds = (left as f32 / 20.0 * 0.1) as i32;
        self.frog.growth += seconds * 20;
        self.persistence_required = true;
        if self.frog.growth >= TICKS_TO_BE_FROG {
            self.frog.grown = true;
        }
    }
}

impl EntityWorld {
    /// The cubes a frog may eat (`Frog.canEat`: `#frog_food`, at size 1).
    pub(super) fn frog_food(&self) -> Vec<(u64, DVec3)> {
        self.slimes.iter().filter(|e| e.slime.health > 0.0 && e.slime.size == 1).map(|e| (e.id, e.slime.body.position)).collect()
    }

    /// A frog's pull, bite and spawn, and a tadpole grown into a frog.
    pub(super) fn frog_step(&mut self, id: u64, world: &mut impl World) {
        let Some(e) = self.mob_mut(id) else { return };
        let (pull, eat, laid, grown) = (e.frog.pull.take(), e.frog.eat.take(), e.frog.laid.take(), std::mem::take(&mut e.frog.grown));
        let (position, variant) = (e.body.position, variant_key(&e.variant));
        if let Some((cube, velocity)) = pull {
            if let Some(c) = self.slimes.iter_mut().find(|c| c.id == cube) {
                c.slime.body.velocity = velocity;
                c.slime.body.needs_sync = true;
            }
        }
        if let Some(cube) = eat {
            if let Some(e) = self.mob_mut(id) {
                e.voices.push((Voice::Event("entity.frog.eat", 2.0, 1.0), position));
            }
            if self.slimes.iter().any(|c| c.id == cube && c.slime.health > 0.0) {
                let attack = Kind::Frog.species().attack;
                let _ = self.mob_hits_mob(id, cube, attack, position, 0.0);
                // The loot sees which frog it was (`frog/variant`).
                if let Some(c) = self.slimes.iter_mut().find(|c| c.id == cube) {
                    if let Some(death) = c.slime.damage.death.as_mut() {
                        death.attacker_variant = Some(variant);
                    }
                }
            }
        }
        if let Some(cell) = laid {
            world.set_block(cell, Some(minecraftoss_player::Block::new("minecraft:frogspawn")));
            if let Some(e) = self.mob_mut(id) {
                e.voices.push((Voice::Event("entity.frog.lay_spawn", 1.0, 1.0), position));
            }
        }
        if grown {
            self.tadpole_to_frog(id, world);
        }
    }

    /// `Tadpole.ageUp`: it becomes a frog of its biome's kind, kept, with
    /// the grow-up sound.
    fn tadpole_to_frog(&mut self, id: u64, world: &impl World) {
        let Some(old) = self.discard_mob(id) else { return };
        let p = old.body.position;
        let biome = world.biome((p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)).unwrap_or_default();
        let new = self.spawn_mob(Kind::Frog, p, old.yaw, 0, old.no_ai);
        if let Some(e) = self.mob_mut(new) {
            e.variant = variant_for_biome(&biome).to_owned();
            e.persistence_required = true;
            e.body.velocity = old.body.velocity;
            e.voices.push((Voice::Event("entity.tadpole.grow_up", 0.15, 1.0), p));
        }
    }
}
