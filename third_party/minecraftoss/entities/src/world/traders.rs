//! Wandering traders (26.3 `WanderingTrader`, `UseItemGoal`,
//! `TradeWithPlayerGoal`, `LookAtTradingPlayerGoal`,
//! `WanderToPositionGoal`, `MoveTowardsRestrictionGoal`).
//!
//! - At dusk a trader drinks a potion of invisibility (it says
//!   `disappeared` when done), and at dawn milk to show itself again.
//! - It shies from zombies, illagers, vexes and zoglins, panics when hurt,
//!   heads for the spot the spawner chose ten blocks at a time, keeps
//!   within sixteen of its home, strolls and looks about.
//! - A player's use opens its trades: one buying, two uncommon and five
//!   common offers, made the first time they are asked for. Trades give no
//!   experience to the trader, only the orb, and the screen shows no level.
//! - Its despawn delay counts down while nobody trades with it, and at
//!   nought it is gone.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::merchant::{MerchantEvent, Offers};
use crate::monster_ai::{MonsterGoalContext, Target};
use crate::navigation::navigate_walk_to;
use crate::species::Kind;
use crate::trading::MerchantOffer;

type Cell = (i32, i32, i32);

/// What a trader drinks: the potion by night, milk by day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraderDrink {
    Invisibility,
    Milk,
}

impl TraderDrink {
    fn item(self) -> &'static str {
        match self {
            Self::Invisibility => "minecraft:potion",
            Self::Milk => "minecraft:milk_bucket",
        }
    }
}

/// A wandering trader's trades, timer and errands.
#[derive(Clone, Debug, Default)]
pub struct TraderState {
    pub offers: Option<Vec<MerchantOffer>>,
    pub trading_player: Option<u64>,
    /// `despawnDelay`: none at nought.
    pub despawn_delay: i32,
    /// `wanderTarget`: where the spawner sent it.
    pub wander_target: Option<Cell>,
    /// `setHomeTo`: the spot it keeps near and how near.
    pub home: Option<(Cell, i32)>,
    /// What it drinks and the ticks of drinking left (`useItemRemaining`).
    pub drinking: Option<(TraderDrink, i32)>,
}

/// What its goals share.
#[derive(Clone, Debug, Default)]
pub struct TraderCtx {
    pub invisible: bool,
    pub drinking: bool,
    pub trading: Option<u64>,
    pub wander_target: Option<Cell>,
    pub home: Option<(Cell, i32)>,
    /// Out: a drink started, a drink's goal ended (its finishing sound),
    /// trading stopped, the errand done.
    pub start_drink: Option<TraderDrink>,
    pub drink_done: Option<TraderDrink>,
    pub stop_trading: bool,
    pub clear_wander: bool,
}

/// `WanderingTrader.registerGoals`.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    use crate::monster_ai::AvoidEntityGoal;
    let mut selector = GoalSelector::default();
    selector.add(0, crate::monster_ai::FloatGoal);
    selector.add(0, DrinkGoal { drink: TraderDrink::Invisibility });
    selector.add(0, DrinkGoal { drink: TraderDrink::Milk });
    selector.add(1, TradeWithPlayerGoal);
    for (kind, distance) in [("minecraft:zombie", 8.0), ("minecraft:evoker", 12.0), ("minecraft:vindicator", 8.0), ("minecraft:vex", 8.0), ("minecraft:pillager", 15.0), ("minecraft:illusioner", 12.0), ("minecraft:zoglin", 10.0)] {
        selector.add(1, AvoidEntityGoal::new(kind, distance, 0.5, 0.5));
    }
    selector.add(1, crate::mob_ai::PanicGoal::new(0.5));
    selector.add(1, LookAtTradingPlayerGoal);
    selector.add(2, WanderToPositionGoal { stop_distance: 2.0, speed: 0.35 });
    selector.add(4, MoveTowardsRestrictionGoal { speed: 0.35, wanted: None });
    selector.add(8, crate::monster_ai::StrollGoal::new(0.35, 0.001));
    selector.add(9, InteractGoal { range: 3.0 });
    selector.add(10, LookAtMobGoal { range: 8.0, target: None, look_time: 0 });
    selector
}

/// `UseItemGoal`: while its wish holds (dark outside and seen for the
/// potion, light outside and unseen for the milk) it takes the drink in
/// hand and drinks it; when done the hand is emptied with its sound.
#[derive(Clone)]
struct DrinkGoal {
    drink: TraderDrink,
}

impl Goal<MonsterGoalContext> for DrinkGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        // `isDarkOutside` and `isBrightOutside` are each other's opposite
        // in the overworld.
        match self.drink {
            TraderDrink::Invisibility => !ctx.bright_outside && !ctx.trader.invisible,
            TraderDrink::Milk => ctx.bright_outside && ctx.trader.invisible,
        }
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.trader.drinking
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.trader.start_drink = Some(self.drink);
        ctx.trader.drinking = true;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.trader.drink_done = Some(self.drink);
    }
}

/// `TradeWithPlayerGoal`: standing still for its trader while it is on
/// the ground, dry, and within four blocks; when that fails trading ends.
#[derive(Clone)]
struct TradeWithPlayerGoal;

impl TradeWithPlayerGoal {
    fn holds(ctx: &MonsterGoalContext) -> bool {
        if ctx.fluid.in_water() || !ctx.body.on_ground || ctx.health <= 0.0 {
            return false;
        }
        let Some(player) = ctx.trader.trading.and_then(|id| ctx.players.iter().find(|p| p.id == id)) else { return false };
        player.position.distance_squared(ctx.body.position) <= 16.0
    }
}

impl Goal<MonsterGoalContext> for TradeWithPlayerGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Jump, Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        Self::holds(ctx)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        Self::holds(ctx)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.navigation.stop();
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.trader.stop_trading = true;
    }
}

/// `LookAtTradingPlayerGoal`: `LookAtPlayerGoal` on its trader, while it
/// trades.
#[derive(Clone)]
struct LookAtTradingPlayerGoal;

impl Goal<MonsterGoalContext> for LookAtTradingPlayerGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.look_at_player.target_id = ctx.trader.trading;
        ctx.trader.trading.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.look_at_player.can_continue_within(ctx.body.position, &ctx.players, 8.0)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.look_at_player.start(&mut ctx.random);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.look_at_player.stop();
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        if let Some(target) = ctx.look_at_player.tick(&ctx.players) {
            let x_max = ctx.max_head_x_rot();
            ctx.look_control.set_look_at_with_limits(target, 10.0, x_max);
        }
    }
}

/// `InteractGoal(trader, Player.class, 3, 1)`: `LookAtPlayerGoal` that
/// never declines, on a player it sees within three blocks.
#[derive(Clone)]
struct InteractGoal {
    range: f64,
}

impl Goal<MonsterGoalContext> for InteractGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Look, Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        // Its 1.0 chance still draws a float.
        let _ = ctx.random.next_float();
        let (position, eye) = (ctx.body.position, ctx.eye());
        let mut best: Option<(f64, u64)> = None;
        for player in ctx.players.clone() {
            if !player.alive || player.spectator || position.distance_squared(player.position) > self.range * self.range {
                continue;
            }
            let Some(info) = ctx.info(Target::Player(player.id)) else { continue };
            if !ctx.sees(world, info) {
                continue;
            }
            let distance = player.position.distance_squared(eye);
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, player.id));
            }
        }
        ctx.look_at_player.target_id = best.map(|(_, id)| id);
        best.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.look_at_player.can_continue_within(ctx.body.position, &ctx.players, self.range)
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.look_at_player.start(&mut ctx.random);
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.look_at_player.stop();
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        if let Some(target) = ctx.look_at_player.tick(&ctx.players) {
            let x_max = ctx.max_head_x_rot();
            ctx.look_control.set_look_at_with_limits(target, 10.0, x_max);
        }
    }
}

/// `LookAtPlayerGoal(trader, Mob.class, 8)`: one time in fifty, the
/// nearest other mob within eight blocks (not a player), watched for two
/// to four seconds.
#[derive(Clone)]
struct LookAtMobGoal {
    range: f64,
    target: Option<u64>,
    look_time: i32,
}

impl Goal<MonsterGoalContext> for LookAtMobGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.random.next_float() >= 0.02 {
            return false;
        }
        let (p, eye) = (ctx.body.position, ctx.eye());
        let r = self.range;
        let mut best: Option<(f64, u64)> = None;
        for m in ctx.mobs.clone() {
            let d = m.position - p;
            if !m.alive || d.x.abs() > r || d.z.abs() > r || d.y.abs() > 3.0 + f64::from(m.height) || p.distance_squared(m.position) > r * r {
                continue;
            }
            let Some(info) = ctx.info(Target::Mob(m.id)) else { continue };
            if !ctx.sees(world, info) {
                continue;
            }
            let distance = m.position.distance_squared(eye);
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, m.id));
            }
        }
        self.target = best.map(|(_, id)| id);
        best.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        let Some(m) = self.target.and_then(|id| ctx.mobs.iter().find(|m| m.id == id)) else { return false };
        m.alive && m.position.distance_squared(ctx.body.position) <= self.range * self.range && self.look_time > 0
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        // `adjustedTickDelay(40 + nextInt(40))` on the every-other tick.
        self.look_time = (40 + ctx.random.next_int(40) as i32 + 1) / 2;
    }
    fn stop(&mut self, _ctx: &mut MonsterGoalContext) {
        self.target = None;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        let Some(m) = self.target.and_then(|id| ctx.mobs.iter().find(|m| m.id == id)) else { return };
        let at = m.position + DVec3::new(0.0, f64::from(m.eye_height), 0.0);
        let x_max = ctx.max_head_x_rot();
        ctx.look_control.set_look_at_with_limits(at, 10.0, x_max);
        self.look_time -= 1;
    }
}

/// `WanderToPositionGoal(trader, 2, 0.35)`: farther than two blocks from
/// its errand's spot it walks there, ten blocks at a time while more than
/// ten off; the errand is forgotten when the goal ends.
#[derive(Clone)]
struct WanderToPositionGoal {
    stop_distance: f64,
    speed: f64,
}

fn too_far(cell: Cell, position: DVec3, distance: f64) -> bool {
    let centre = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.5, f64::from(cell.2) + 0.5);
    centre.distance_squared(position) >= distance * distance
}

impl Goal<MonsterGoalContext> for WanderToPositionGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.trader.wander_target.is_some_and(|cell| too_far(cell, ctx.body.position, self.stop_distance))
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.can_start(ctx, world)
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.trader.wander_target = None;
        ctx.trader.clear_wander = true;
        ctx.navigation.stop();
    }
    fn tick_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        let Some(cell) = ctx.trader.wander_target else { return };
        if !ctx.navigation.is_done() {
            return;
        }
        let p = ctx.body.position;
        let to = if too_far(cell, p, 10.0) {
            let towards = DVec3::new(f64::from(cell.0), f64::from(cell.1), f64::from(cell.2)) - p;
            p + towards.normalize_or_zero() * 10.0
        } else {
            DVec3::new(f64::from(cell.0), f64::from(cell.1), f64::from(cell.2))
        };
        let profile = ctx.profile();
        let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, self.speed, 1);
    }
}

/// `MoveTowardsRestrictionGoal(trader, 0.35)`: strayed beyond its home's
/// reach, it walks to a spot up to 16 blocks toward home.
#[derive(Clone)]
struct MoveTowardsRestrictionGoal {
    speed: f64,
    wanted: Option<DVec3>,
}

impl Goal<MonsterGoalContext> for MoveTowardsRestrictionGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let Some((home, radius)) = ctx.trader.home else { return false };
        let p = ctx.body.position;
        let cell = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let (dx, dy, dz) = (i64::from(cell.0 - home.0), i64::from(cell.1 - home.1), i64::from(cell.2 - home.2));
        if dx * dx + dy * dy + dz * dz < i64::from(radius) * i64::from(radius) {
            return false;
        }
        let towards = DVec3::new(f64::from(home.0) + 0.5, f64::from(home.1), f64::from(home.2) + 0.5);
        let profile = ctx.profile();
        self.wanted = crate::stroll::default_random_position_towards(world, &profile, p, 16, 7, towards, std::f32::consts::FRAC_PI_2, &mut ctx.random);
        self.wanted.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        !ctx.navigation.is_done()
    }
    fn start_world(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) {
        if let Some(to) = self.wanted {
            let profile = ctx.profile();
            let _ = navigate_walk_to(&ctx.body, &mut ctx.navigation, world, &profile, ctx.fluid, to, self.speed, 1);
        }
    }
}

/// How long its drinks take (`Consumable.consumeSeconds` 1.6).
const DRINK_TICKS: i32 = 32;

impl MobEntity {
    /// Before its goals: `LivingEntity.updatingUsingItem` (the drinking
    /// sounds, and the drink's effect when done) and what the goals see.
    pub(super) fn trader_prepare(&mut self) {
        if self.kind != Kind::WanderingTrader {
            return;
        }
        let position = self.body.position;
        if let Some((drink, left)) = self.trader.drinking {
            let left = left - 1;
            // `Consumable.emitParticlesAndSounds` every fourth tick once
            // seven have passed, and once more as it finishes.
            if left == 0 || (left <= DRINK_TICKS - 7 && left % 4 == 0) {
                let _ = self.random.next_boolean();
                let _ = (self.random.next_float(), self.random.next_float());
                let pitch = 0.9 + self.random.next_float() * 0.1;
                let sound = if drink == TraderDrink::Milk { "entity.wandering_trader.drink_milk" } else { "entity.wandering_trader.drink_potion" };
                self.voices.push((Voice::Event(sound, 0.5, pitch), position));
            }
            if left <= 0 {
                self.trader.drinking = None;
                match drink {
                    // `Potions.INVISIBILITY`: three minutes.
                    TraderDrink::Invisibility => {
                        let _ = self.effects.add(crate::effects::EffectInstance::new(crate::effects::MobEffect::Invisibility, 3600, 0));
                    }
                    // `ClearAllStatusEffectsConsumeEffect`.
                    TraderDrink::Milk => self.effects.clear(),
                }
                self.hand = Some(if drink == TraderDrink::Milk { "minecraft:bucket" } else { "minecraft:glass_bottle" }.to_owned());
            } else {
                self.trader.drinking = Some((drink, left));
            }
        }
        let invisible = self.effects.has(crate::effects::MobEffect::Invisibility);
        let t = &self.trader;
        let ctx = &mut self.ai.state.trader;
        ctx.invisible = invisible;
        ctx.drinking = t.drinking.is_some();
        ctx.trading = t.trading_player;
        ctx.wander_target = t.wander_target;
        ctx.home = t.home;
    }

    /// After its goals: a drink taken up or put down, trading ended, its
    /// errand dropped.
    pub(super) fn trader_settle(&mut self) {
        if self.kind != Kind::WanderingTrader {
            return;
        }
        let position = self.body.position;
        let ctx = std::mem::take(&mut self.ai.state.trader);
        if let Some(drink) = ctx.start_drink {
            self.hand = Some(drink.item().to_owned());
            self.trader.drinking = Some((drink, DRINK_TICKS));
        }
        if let Some(drink) = ctx.drink_done {
            self.hand = None;
            let pitch = self.random.next_float() * 0.2 + 0.9;
            let sound = if drink == TraderDrink::Milk { "entity.wandering_trader.reappeared" } else { "entity.wandering_trader.disappeared" };
            self.voices.push((Voice::Event(sound, 1.0, pitch), position));
        }
        if ctx.stop_trading {
            self.trader.trading_player = None;
        }
        if ctx.clear_wander {
            self.trader.wander_target = None;
        }
    }
}

impl EntityWorld {
    /// `WanderingTrader.maybeDespawn` after its `aiStep`: its delay counts
    /// down while nobody trades with it, and at nought it goes.
    pub(super) fn trader_step(&mut self, id: u64) {
        let Some(e) = self.mob_mut(id) else { return };
        let t = &mut e.trader;
        if t.despawn_delay > 0 && t.trading_player.is_none() {
            t.despawn_delay -= 1;
            if t.despawn_delay == 0 {
                let _ = self.discard_mob(id);
            }
        }
    }

    /// `WanderingTrader.updateTrades`: one offer from each of its buying,
    /// uncommon and common sets, as many as each set's amount.
    fn trader_offers(&mut self, id: u64) -> Option<&[MerchantOffer]> {
        let index = self.mobs.iter().position(|e| e.id == id && e.kind == Kind::WanderingTrader)?;
        if self.mobs[index].trader.offers.is_none() {
            let mut offers = Vec::new();
            if let Some(book) = self.trades.clone() {
                for set in ["minecraft:wandering_trader/buying", "minecraft:wandering_trader/uncommon", "minecraft:wandering_trader/common"] {
                    offers.extend(book.offers_from(set, "minecraft:plains", &mut self.trade_sequences));
                }
            }
            self.mobs[index].trader.offers = Some(offers);
        }
        self.mobs[index].trader.offers.as_deref()
    }

    /// `WanderingTrader.mobInteract` by `player`: its offers open, if it has
    /// any and is free (a villager spawn egg is used as such instead).
    pub fn trader_interact(&mut self, id: u64, player: u64, held: Option<&str>) -> VillagerUse {
        let Some(e) = self.mobs.iter().find(|e| e.id == id && e.kind == Kind::WanderingTrader) else { return VillagerUse::Pass };
        if held == Some("minecraft:villager_spawn_egg") || e.health <= 0.0 || e.trader.trading_player.is_some() {
            return VillagerUse::Pass;
        }
        if self.trader_offers(id).is_none_or(<[MerchantOffer]>::is_empty) {
            return VillagerUse::Consume;
        }
        if let Some(e) = self.mob_mut(id) {
            e.trader.trading_player = Some(player);
        }
        VillagerUse::Success
    }

    /// The trader trading with `player`, if any.
    pub(super) fn trading_trader(&self, player: u64) -> Option<u64> {
        self.mobs.iter().find(|e| e.kind == Kind::WanderingTrader && e.trader.trading_player == Some(player)).map(|e| e.id)
    }

    /// What a trader shows: its offers, with no level or progress and no
    /// restocking.
    pub(super) fn trader_merchant_offers(&mut self, id: u64) -> Option<Offers> {
        let offers = self.trader_offers(id)?.to_vec();
        let mut max_stacks: Vec<(String, i32)> = Vec::new();
        for offer in &offers {
            for item in std::iter::once(&offer.buy.id).chain(offer.buy_b.as_ref().map(|b| &b.id)).chain(std::iter::once(&offer.sell.id)) {
                let max = self.item_max_stack(item);
                if max != 64 && !max_stacks.iter().any(|(id, _)| id == item) {
                    max_stacks.push((item.clone(), max));
                }
            }
        }
        Some(Offers { offers, max_stacks, level: 1, xp: 0, show_progress: false, can_restock: false })
    }

    /// `AbstractVillager.notifyTradeUpdated` and `notifyTrade` with the
    /// trader's own sounds and `WanderingTrader.rewardTradeXp`.
    pub(super) fn trader_notify(&mut self, id: u64, events: &[MerchantEvent]) {
        for event in events {
            let Some(e) = self.mob_mut(id) else { return };
            let position = e.body.position;
            match *event {
                MerchantEvent::Updated(valid) => {
                    if e.ambient_sound_time > -80 + 20 {
                        e.ambient_sound_time = -80;
                        let pitch = (e.random.next_float() - e.random.next_float()) * 0.2 + 1.0;
                        let sound = if valid { "entity.wandering_trader.yes" } else { "entity.wandering_trader.no" };
                        e.voices.push((Voice::Event(sound, 1.0, pitch), position));
                    }
                }
                MerchantEvent::Trade(index) => {
                    let Some(offer) = e.trader.offers.as_mut().and_then(|o| o.get_mut(index)) else { continue };
                    offer.uses += 1;
                    let reward = offer.reward_exp;
                    e.ambient_sound_time = -80;
                    let pitch = (e.random.next_float() - e.random.next_float()) * 0.2 + 1.0;
                    e.voices.push((Voice::Event("entity.wandering_trader.yes", 1.0, pitch), position));
                    if reward {
                        let pop = 3 + e.random.next_int(4) as i32;
                        self.trade_experience.push((DVec3::new(position.x, position.y + 0.5, position.z), pop));
                    }
                }
            }
        }
    }

    /// `AbstractVillager.stillValid` for a trader's screen.
    pub(super) fn trader_still_valid(&self, id: u64, player: u64, eyes: DVec3, creative: bool) -> bool {
        let Some(e) = self.mobs.iter().find(|e| e.id == id && e.kind == Kind::WanderingTrader) else { return false };
        if e.trader.trading_player != Some(player) || e.health <= 0.0 {
            return false;
        }
        let body = &e.body;
        let half = f64::from(body.width) / 2.0;
        let (min, max) = (body.position - DVec3::new(half, 0.0, half), body.position + DVec3::new(half, f64::from(body.height), half));
        let outside = |v: f64, lo: f64, hi: f64| (lo - v).max(v - hi).max(0.0);
        let (dx, dy, dz) = (outside(eyes.x, min.x, max.x), outside(eyes.y, min.y, max.y), outside(eyes.z, min.z, max.z));
        let range = if creative { 5.0 } else { 3.0 } + 4.0;
        dx * dx + dy * dy + dz * dz < range * range
    }

    /// The screen closed: the trader is free again.
    pub(super) fn trader_stop(&mut self, id: u64) {
        if let Some(e) = self.mobs.iter_mut().find(|e| e.id == id && e.kind == Kind::WanderingTrader) {
            e.trader.trading_player = None;
        }
    }
}
