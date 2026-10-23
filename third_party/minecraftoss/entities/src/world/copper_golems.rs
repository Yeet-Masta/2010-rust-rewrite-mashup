//! Copper golems (26.3 `CopperGolem`, `CopperGolemAi`,
//! `TransportItemsBetweenContainers`).
//!
//! - Empty-handed, a copper golem walks to the nearest copper chest within
//!   32 blocks across and 8 up or down. It opens it for three seconds and
//!   takes up to sixteen of its first stack. Laden, it walks to chests and
//!   trapped chests that are empty or hold the same item, and puts its load
//!   in. It waits its turn at a chest someone has open, visits at most ten
//!   chests a round, and rests seven seconds after a round finds none.
//! - Every seven hours or so it weathers a stage. Fully oxidized, it may
//!   turn into a statue where it stands. Honeycomb waxes it against
//!   weathering, and an axe scrapes off the wax or a stage.
//! - An empty hand takes the item it carries.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Controls, Goal, GoalSelector};
use crate::monster_ai::MonsterGoalContext;
use crate::species::Kind;
use minecraftoss_player::ContainerTarget;

type Cell = (i32, i32, i32);

/// `WEATHERING_TICK_FROM` and `WEATHERING_TICK_TO`.
const WEATHERING: (i64, i64) = (504000, 552000);
/// `TARGET_INTERACTION_TIME`, `IDLE_COOLDOWN`, `MAX_VISITED_POSITIONS`,
/// `MAX_UNREACHABLE_POSITIONS`.
const INTERACTION_TIME: i32 = 60;
const IDLE_COOLDOWN: i32 = 140;
const MAX_VISITED: usize = 10;
const MAX_UNREACHABLE: usize = 50;
/// The four oxidation stages' names (`WeatheringCopper.WeatherState`).
pub const WEATHER_STATES: [&str; 4] = ["unaffected", "exposed", "weathered", "oxidized"];
/// `CopperGolemStatueBlock.Pose`.
const POSES: [&str; 4] = ["standing", "running", "sitting", "star"];

/// `CopperGolemState`: what its model shows it doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GolemAction {
    #[default]
    Idle,
    GettingItem,
    GettingNoItem,
    DroppingItem,
    DroppingNoItem,
}

/// `TransportItemState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Travelling,
    Queuing,
    Interacting,
}

/// What it asks of the level this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Open(Cell),
    Close(Cell),
    Take(Cell),
    Put(Cell),
}

/// A copper golem's load, weathering and errand.
#[derive(Clone, Debug, Default)]
pub struct CopperGolemState {
    /// What it carries in hand, with its count.
    pub carried: Option<minecraftoss_player::inventory::ItemStack>,
    /// Its oxidation stage (0 to 3), and `nextWeatheringTick` (-1 unset,
    /// -2 waxed).
    pub weather: u8,
    pub next_weathering: i64,
    pub action: GolemAction,
    pub action_start: i32,
    /// `TRANSPORT_ITEMS_COOLDOWN_TICKS` (none at zero), first set as it
    /// comes (60 to 99).
    pub cooldown: i32,
    started: bool,
    /// `VISITED_BLOCK_POSITIONS` and `UNREACHABLE_TRANSPORT_BLOCK_POSITIONS`,
    /// with when they expire.
    visited: Vec<Cell>,
    unreachable: Vec<Cell>,
    memory_until: i64,
    target: Option<Cell>,
    phase: Phase,
    interaction: Option<GolemAction>,
    ticks_at_target: i32,
    opened: Option<Cell>,
    ops: Vec<Op>,
}

/// What the golem's goals share.
#[derive(Clone, Debug, Default)]
pub struct CopperCtx {
    /// It rests between rounds: it may stroll.
    pub resting: bool,
}

pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, crate::mob_ai::PanicGoal::new(1.5));
    selector.add(2, RestingGate(crate::monster_ai::StrollGoal::new(1.0, 0.0)));
    selector.add(3, crate::monster_ai::LookAtPlayerGoal { range: 6.0 });
    selector
}

/// `RunOne` only with `TRANSPORT_ITEMS_COOLDOWN_TICKS` present.
#[derive(Clone)]
struct RestingGate<G>(G);

impl<G: Goal<MonsterGoalContext> + Clone + 'static> Goal<MonsterGoalContext> for RestingGate<G> {
    fn controls(&self) -> Controls {
        self.0.controls()
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        ctx.copper.resting && self.0.can_start(ctx, world)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        ctx.copper.resting && self.0.can_continue(ctx, world)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.start(ctx);
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.start_world(ctx, world);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.stop(ctx);
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        self.0.tick(ctx);
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        self.0.tick_world(ctx, world);
    }
}

/// `isWithinTargetDistance`: the chest's shape grown `reach` across and
/// half a block up and down meets the golem's box about `centre`.
fn within(target: Cell, reach: f64, centre: DVec3, width: f64, height: f64) -> bool {
    let (x, y, z) = (f64::from(target.0), f64::from(target.1), f64::from(target.2));
    let (min, max) = (DVec3::new(x + 0.0625 - reach, y - 0.5, z + 0.0625 - reach), DVec3::new(x + 0.9375 + reach, y + 0.875 + 0.5, z + 0.9375 + reach));
    let (half_w, half_h) = (width / 2.0, height / 2.0);
    centre.x - half_w < max.x && centre.x + half_w > min.x && centre.y - half_h < max.y && centre.y + half_h > min.y && centre.z - half_w < max.z && centre.z + half_w > min.z
}

impl CopperGolemState {
    fn picking_up(&self) -> bool {
        self.carried.is_none()
    }

    /// `isWantedBlock`.
    fn wants(&self, t: &ContainerTarget) -> bool {
        if self.picking_up() {
            t.copper
        } else {
            t.chest
        }
    }

    /// `stopTargetingCurrentTarget`.
    fn drop_target(&mut self) {
        self.ticks_at_target = 0;
        self.target = None;
    }

    /// `clearMemoriesAfterMatchingTargetFound`.
    fn found_match(&mut self) {
        self.drop_target();
        self.visited.clear();
        self.unreachable.clear();
    }

    /// `enterCooldownAfterNoMatchingTargetFound`.
    fn rest(&mut self) {
        self.drop_target();
        self.cooldown = IDLE_COOLDOWN;
        self.visited.clear();
        self.unreachable.clear();
    }

    /// `onStartTravelling`: the chest it had open closes, and it stands
    /// idle.
    fn start_travelling(&mut self, tick: i32) {
        if let Some(c) = self.opened.take() {
            self.ops.push(Op::Close(c));
        }
        self.set_action(GolemAction::Idle, tick);
        self.phase = Phase::Travelling;
        self.interaction = None;
        self.ticks_at_target = 0;
    }

    fn set_action(&mut self, action: GolemAction, tick: i32) {
        if self.action != action {
            self.action = action;
            self.action_start = tick;
        }
    }

    /// What the chest makes of its visit (`doReachedTargetInteraction`).
    fn interaction_for(&self, t: &ContainerTarget) -> GolemAction {
        match &self.carried {
            None if !t.empty => GolemAction::GettingItem,
            None => GolemAction::GettingNoItem,
            Some(held) if t.empty || t.items.iter().any(|i| *i == held.id) => GolemAction::DroppingItem,
            Some(_) => GolemAction::DroppingNoItem,
        }
    }
}

impl MobEntity {
    /// `TransportItemsBetweenContainers.tick` as its brain runs it, before
    /// its goals.
    pub(super) fn copper_prepare(&mut self, world: &dyn World, game_time: i64, leashed: bool) {
        if self.kind != Kind::CopperGolem || self.health <= 0.0 {
            return;
        }
        let tick = self.tick_count;
        let c = &mut self.copper;
        if !c.started {
            c.started = true;
            if c.cooldown == 0 {
                c.cooldown = 60 + self.random.next_int(40) as i32;
            }
        }
        let c = &mut self.copper;
        c.cooldown = (c.cooldown - 1).max(0);
        if c.memory_until != 0 && game_time >= c.memory_until {
            c.visited.clear();
            c.unreachable.clear();
            c.memory_until = 0;
        }
        let panicking = crate::mob_ai::panicking(&self.ai.state);
        self.ai.state.copper = CopperCtx { resting: self.copper.cooldown > 0 };
        if self.copper.cooldown > 0 || panicking || leashed {
            if self.copper.target.is_some() || self.copper.opened.is_some() {
                self.copper.drop_target();
                self.copper.start_travelling(tick);
            }
            return;
        }
        let p = self.body.position;
        let (width, height) = (f64::from(self.body.width), f64::from(self.body.height));
        let centre = p + DVec3::new(0.0, height / 2.0, 0.0);
        // `hasFinishedPath`: a path followed to its end.
        let nav = &self.ai.state.navigation;
        let finished = !nav.nodes.is_empty() && nav.next >= nav.nodes.len();
        let reach = if finished { 1.0 } else { 0.5 };
        // `updateInvalidTarget`.
        let current = self.copper.target.and_then(|t| world.transport_target(t));
        let valid = match &current {
            Some(t) if self.copper.wants(t) && !t.blocked => {
                // A path that ended short of the chest: unreachable.
                let stuck = self.copper.phase == Phase::Travelling && finished && !within(t.pos, reach, centre, width, height);
                if stuck {
                    let pos = t.pos;
                    self.copper.visited.retain(|v| *v != pos);
                    self.copper.unreachable.push(pos);
                    if self.copper.unreachable.len() > MAX_UNREACHABLE {
                        self.copper.rest();
                    } else {
                        self.copper.memory_until = game_time + 6000;
                    }
                }
                !stuck
            }
            _ => false,
        };
        if !valid {
            self.copper.drop_target();
            let at = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let seen = |c: Cell, s: &CopperGolemState| s.visited.contains(&c) || s.unreachable.contains(&c);
            let best = world
                .transport_targets(at, 32, 8)
                .into_iter()
                .filter(|t| self.copper.wants(t) && !t.locked && !seen(t.pos, &self.copper) && t.connected.is_none_or(|o| !seen(o, &self.copper)))
                .min_by(|a, b| {
                    let d = |c: Cell| DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + 0.5, f64::from(c.2) + 0.5).distance_squared(p);
                    d(a.pos).total_cmp(&d(b.pos))
                });
            match best {
                Some(t) => {
                    self.copper.target = Some(t.pos);
                    self.copper.start_travelling(tick);
                    // `setVisitedBlockPos`.
                    self.copper.visited.push(t.pos);
                    if self.copper.visited.len() > MAX_VISITED {
                        self.copper.rest();
                    } else {
                        self.copper.memory_until = game_time + 6000;
                    }
                }
                None => self.copper.rest(),
            }
            return;
        }
        let Some(t) = current else { return };
        let walk = |e: &mut MobEntity| {
            let to = DVec3::new(f64::from(t.pos.0) + 0.5, f64::from(t.pos.1), f64::from(t.pos.2) + 0.5);
            if e.ai.state.navigation.nodes.is_empty() {
                let profile = e.ai.state.profile();
                let fluid = e.ai.state.fluid;
                let _ = crate::navigation::navigate_walk_to(&e.body, &mut e.ai.state.navigation, world, &profile, fluid, to, 1.0, 0);
            }
        };
        let stop_in_place = |e: &mut MobEntity| {
            e.ai.state.navigation.stop();
            e.body.velocity.x = 0.0;
            e.body.velocity.z = 0.0;
        };
        match self.copper.phase {
            Phase::Queuing => {
                if !t.in_use {
                    self.copper.phase = Phase::Travelling;
                    walk(self);
                }
            }
            Phase::Travelling => {
                if within(t.pos, 3.0, centre, width, height) && t.in_use {
                    stop_in_place(self);
                    self.copper.phase = Phase::Queuing;
                } else if within(t.pos, reach, centre, width, height) {
                    self.copper.interaction = Some(self.copper.interaction_for(&t));
                    self.copper.phase = Phase::Interacting;
                } else {
                    walk(self);
                }
            }
            Phase::Interacting => {
                if !within(t.pos, 2.0, centre, width, height) {
                    self.copper.start_travelling(tick);
                    return;
                }
                self.copper.ticks_at_target += 1;
                let ticks = self.copper.ticks_at_target;
                let look = DVec3::new(f64::from(t.pos.0) + 0.5, f64::from(t.pos.1) + 0.5, f64::from(t.pos.2) + 0.5);
                self.ai.state.look_control.set_look_at_with_limits(look, 45.0, 90.0);
                stop_in_place(self);
                // `onReachedTargetInteraction`.
                if let Some(action) = self.copper.interaction {
                    if ticks == 1 {
                        self.copper.ops.push(Op::Open(t.pos));
                        self.copper.opened = Some(t.pos);
                        self.copper.set_action(action, tick);
                    }
                    if ticks == 9 {
                        let sound = match action {
                            GolemAction::GettingItem => "entity.copper_golem.no_item_get",
                            GolemAction::GettingNoItem => "entity.copper_golem.no_item_no_get",
                            GolemAction::DroppingItem => "entity.copper_golem.item_drop",
                            _ => "entity.copper_golem.item_no_drop",
                        };
                        let at = self.position();
                        self.voices.push((Voice::Event(sound, 1.0, voice_pitch(&mut self.random, false)), at));
                    }
                    if ticks == INTERACTION_TIME {
                        if let Some(c) = self.copper.opened.take() {
                            self.copper.ops.push(Op::Close(c));
                        }
                    }
                }
                if ticks >= INTERACTION_TIME {
                    match self.copper.interaction_for(&t) {
                        GolemAction::GettingItem => self.copper.ops.push(Op::Take(t.pos)),
                        GolemAction::DroppingItem => self.copper.ops.push(Op::Put(t.pos)),
                        _ => self.copper.drop_target(),
                    }
                    self.copper.start_travelling(tick);
                }
            }
        }
    }

    /// `CopperGolem.tick` on the server: `updateWeathering`. Whether it
    /// turns into a statue now.
    pub(super) fn copper_weathering(&mut self, world: &dyn World, game_time: i64) -> bool {
        if self.kind != Kind::CopperGolem || self.health <= 0.0 {
            return false;
        }
        let c = &mut self.copper;
        if c.next_weathering == -2 {
            return false;
        }
        if c.next_weathering == -1 {
            c.next_weathering = game_time + WEATHERING.0 + i64::from(self.random.next_int((WEATHERING.1 - WEATHERING.0 + 1) as u32));
            return false;
        }
        let oxidized = c.weather >= 3;
        if game_time >= c.next_weathering && !oxidized {
            c.weather += 1;
            c.next_weathering = if c.weather >= 3 { 0 } else { c.next_weathering + WEATHERING.0 + i64::from(self.random.next_int((WEATHERING.1 - WEATHERING.0 + 1) as u32)) };
            self.variant = WEATHER_STATES[usize::from(c.weather)].to_owned();
        }
        // `canTurnToStatue`.
        let p = self.body.position;
        let feet = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        oxidized && world.block(feet).is_none_or(|b| b.id == "minecraft:air") && self.random.next_float() <= 0.0058
    }
}

impl EntityWorld {
    /// A player's feet as the entity world last had them.
    pub fn player_position(&self, id: u64) -> Option<DVec3> {
        self.pushing_players.iter().find(|b| b.id == id).map(|b| b.position)
    }

    /// After its tick: the chests it opens, closes, takes from and fills,
    /// and its turn into a statue.
    pub(super) fn copper_step(&mut self, id: u64, world: &mut impl World, statue: bool) {
        let Some(e) = self.mob_mut(id) else { return };
        for op in std::mem::take(&mut e.copper.ops) {
            let Some(e) = self.mob_mut(id) else { return };
            match op {
                Op::Open(c) => world.open_container(c, true),
                Op::Close(c) => world.open_container(c, false),
                // `pickUpItems`.
                Op::Take(c) => {
                    if let Some(stack) = world.transport_take(c) {
                        e.hand = Some(stack.id.clone());
                        e.copper.carried = Some(stack);
                        e.copper.found_match();
                    }
                }
                // `putDownItem`.
                Op::Put(c) => {
                    if let Some(stack) = e.copper.carried.take() {
                        let left = world.transport_put(c, stack);
                        if left.count == 0 || left.id == "minecraft:air" {
                            e.hand = None;
                            e.copper.found_match();
                        } else {
                            e.hand = Some(left.id.clone());
                            e.copper.carried = Some(left);
                            e.copper.drop_target();
                        }
                    }
                }
            }
        }
        if !statue {
            return;
        }
        // `turnToStatue`: an oxidized statue in a random pose, facing its
        // way; its load drops and it is gone.
        let Some(e) = self.mobs.iter_mut().find(|e| e.id == id) else { return };
        let p = e.body.position;
        let feet = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let pose = POSES[e.random.next_int(POSES.len() as u32) as usize];
        let facing = match (((e.yaw / 90.0) + 0.5).floor() as i32).rem_euclid(4) {
            0 => "south",
            1 => "west",
            2 => "north",
            _ => "east",
        };
        let carried = e.copper.carried.take();
        world.set_block(feet, Some(minecraftoss_player::Block::new("minecraft:oxidized_copper_golem_statue").with("facing", facing).with("copper_golem_pose", pose).with("waterlogged", "false")));
        if let Some(stack) = carried {
            self.spawned.items.push((p, stack));
        }
        self.discard_mob(id);
        self.sounds.push(MobSound { event: "entity.copper_golem_become_statue".to_owned(), position: p, volume: 1.0, pitch: 1.0, category: "neutral" });
    }

    /// `CopperGolem.mobInteract`: an empty hand takes its load; honeycomb
    /// waxes it, an axe takes off the wax or a stage of weathering. What
    /// the use did, or `None` when it is not the golem's.
    pub fn copper_golem_interact(&mut self, id: u64, held: Option<&str>, player_at: DVec3) -> Option<CopperUse> {
        let e = self.mobs.iter_mut().find(|e| e.id == id && e.kind == Kind::CopperGolem && e.health > 0.0)?;
        let at = e.body.position;
        let cell = (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
        match held {
            None => {
                let stack = e.copper.carried.take()?;
                e.hand = None;
                // `BehaviorUtils.throwItem` toward the player.
                let toward = (player_at - at).normalize_or_zero() * 0.5;
                let from = at + toward + DVec3::new(0.0, f64::from(e.eye_height()) - 0.3, 0.0);
                self.spawned.items.push((from, stack));
                Some(CopperUse::Took)
            }
            Some("minecraft:honeycomb") if e.copper.next_weathering != -2 => {
                e.copper.next_weathering = -2;
                self.spawned.global_events.push((3003, cell, 0));
                self.sounds.push(MobSound { event: "item.honeycomb.wax_on".to_owned(), position: at, volume: 1.0, pitch: 1.0, category: "blocks" });
                Some(CopperUse::Spent)
            }
            Some(axe) if axe.ends_with("_axe") => {
                if e.copper.next_weathering == -2 {
                    e.copper.next_weathering = -1;
                    self.spawned.global_events.push((3004, cell, 0));
                } else if e.copper.weather > 0 {
                    e.copper.weather -= 1;
                    e.copper.next_weathering = -1;
                    e.variant = WEATHER_STATES[usize::from(e.copper.weather)].to_owned();
                    self.spawned.global_events.push((3005, cell, 0));
                } else {
                    return None;
                }
                self.sounds.push(MobSound { event: "item.axe.scrape".to_owned(), position: at, volume: 1.0, pitch: 1.0, category: "neutral" });
                Some(CopperUse::Worn)
            }
            _ => None,
        }
    }
}

/// What a click on a copper golem did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopperUse {
    /// The player took its load (thrown to them).
    Took,
    /// The held item was used up (honeycomb).
    Spent,
    /// The held tool was worn (an axe).
    Worn,
}
