//! Allays (26.3 `Allay`, `AllayAi`).
//!
//! - Given an item, an allay holds one of it and likes the player who gave
//!   it. Clicked with an empty hand, it hands its item back, drops what it
//!   gathered and likes nobody.
//! - Holding an item, it flies to items of the same kind within 32 blocks
//!   and gathers them into its one slot (a stack).
//! - With something gathered it takes it to a note block it heard play in
//!   the last thirty seconds, or else to the player it likes (within 64
//!   blocks), and throws one at a time from within three blocks, waiting
//!   three seconds between throws before it gathers again.
//! - It keeps within sixteen blocks of whoever it delivers to, and heals one
//!   every half second.
//! - A jukebox playing within ten blocks sets it dancing until the song
//!   stops or it strays; given an amethyst shard as it dances, it makes a
//!   second allay, and neither can again for five minutes.
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::MonsterGoalContext;
use crate::species::Kind;

type Cell = (i32, i32, i32);

/// `TIME_TO_FORGET_NOTEBLOCK`.
const FORGET_NOTE_BLOCK: i32 = 600;
/// `ITEM_PICKUP_COOLDOWN_DURATION`.
const PICKUP_COOLDOWN: i32 = 60;
/// `THROW_SOUND_PITCHES`.
const THROW_PITCHES: [f32; 7] = [0.5625, 0.625, 0.75, 0.9375, 1.0, 1.0, 1.125];

/// What an allay keeps and remembers.
#[derive(Clone, Debug, Default)]
pub struct AllayState {
    /// Its one inventory slot (`SimpleContainer(1)`).
    pub stored: Option<(String, i32)>,
    /// `LIKED_PLAYER`, `LIKED_NOTEBLOCK_POSITION` and its
    /// `LIKED_NOTEBLOCK_COOLDOWN_TICKS`, `ITEM_PICKUP_COOLDOWN_TICKS`.
    pub liked_player: Option<u128>,
    pub liked_note_block: Option<Cell>,
    pub note_block_cooldown: i32,
    pub pickup_cooldown: i32,
    /// Out: where it threw an item this tick.
    pub throw_at: Option<DVec3>,
    /// `DATA_DANCING`, the jukebox it dances to, and `duplicationCooldown`.
    pub dancing: bool,
    pub jukebox: Option<Cell>,
    pub duplication_cooldown: i64,
}

/// What the allay's goals share.
#[derive(Clone, Debug, Default)]
pub struct AllayCtx {
    pub allay: bool,
    /// Something gathered to deliver.
    pub has_items: bool,
    /// `getItemDepositPosition`: the note block's top or the liked
    /// player's eyes.
    pub deposit: Option<DVec3>,
    /// `NEAREST_VISIBLE_WANTED_ITEM`.
    pub wanted_item: Option<DVec3>,
    /// Out: a throw toward the deposit.
    pub throw: Option<DVec3>,
}

pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, crate::mob_ai::PanicGoal::new(2.5));
    selector.add(2, GoToWantedItemGoal);
    selector.add(3, GiveItemsGoal { ticks: 0 });
    selector.add(4, StayCloseGoal);
    selector.add(5, crate::mob_ai::WanderGoal { speed: 1.0, interval: 20, horizontal: 8, vertical: 4, water: false });
    selector.add(6, crate::monster_ai::LookAtPlayerGoal { range: 6.0 });
    selector
}

/// `GoToWantedItem(1.75, within 32)`.
#[derive(Clone)]
struct GoToWantedItemGoal;

impl Goal<MonsterGoalContext> for GoToWantedItemGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.allay.wanted_item.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.can_start(ctx, world)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fly_target = None;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        if let Some(item) = ctx.allay.wanted_item {
            ctx.fly_target = Some((item, 1.75));
        }
    }
}

/// `GoAndGiveItemsToTarget(2.25, 20)`: to the deposit, throwing from
/// within three blocks of its eyes.
#[derive(Clone)]
struct GiveItemsGoal {
    ticks: i32,
}

impl Goal<MonsterGoalContext> for GiveItemsGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.allay.has_items && ctx.allay.deposit.is_some()
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        self.ticks < 20 && self.can_start(ctx, world)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.ticks = 0;
        if let Some(to) = ctx.allay.deposit {
            ctx.fly_target = Some((to, 2.25));
        }
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fly_target = None;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        self.ticks += 1;
        let Some(to) = ctx.allay.deposit else { return };
        ctx.look_control.set_look_at_with_limits(to, 45.0, 90.0);
        if to.distance(ctx.eye()) < 3.0 {
            ctx.allay.throw = Some(to);
        }
    }
}

/// `StayCloseToTarget(deposit, no wanted item, 4, 16, 2.25)`.
#[derive(Clone)]
struct StayCloseGoal;

impl Goal<MonsterGoalContext> for StayCloseGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.allay.wanted_item.is_none() && ctx.allay.deposit.is_some_and(|d| d.distance(ctx.body.position) >= 16.0)
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.allay.wanted_item.is_none() && ctx.allay.deposit.is_some_and(|d| d.distance(ctx.body.position) > 4.0)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.fly_target = None;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        if let Some(to) = ctx.allay.deposit {
            ctx.fly_target = Some((to, 2.25));
        }
    }
}

/// `allayConsidersItemEqual` by the item alone.
fn can_store(stored: &Option<(String, i32)>, item: &str) -> bool {
    stored.as_ref().is_none_or(|(id, count)| id == item && *count < 64)
}

impl MobEntity {
    /// Before its goals: its memories' clocks, where it delivers, and the
    /// nearest item it wants.
    pub(super) fn allay_prepare(&mut self, deposit: Option<DVec3>, items: &[(DVec3, String)]) {
        if self.kind != Kind::Allay {
            return;
        }
        let a = &mut self.allay;
        a.note_block_cooldown = (a.note_block_cooldown - 1).max(0);
        a.pickup_cooldown = (a.pickup_cooldown - 1).max(0);
        a.duplication_cooldown = (a.duplication_cooldown - 1).max(0);
        // `setDancing(false)` while it panics.
        if crate::mob_ai::panicking(&self.ai.state) {
            self.allay.dancing = false;
        }
        let a = &mut self.allay;
        let p = self.body.position;
        // `canPickUpLoot` and `wantsToPickUp` for the item sensor.
        let wanted = match (&self.hand, a.pickup_cooldown) {
            (Some(held), 0) => items
                .iter()
                .filter(|(at, id)| id == held && can_store(&a.stored, id) && (at.x - p.x).abs() <= 32.0 && (at.y - p.y).abs() <= 16.0 && (at.z - p.z).abs() <= 32.0)
                .map(|(at, _)| *at)
                .min_by(|x, y| x.distance_squared(p).total_cmp(&y.distance_squared(p))),
            _ => None,
        };
        self.ai.state.allay = AllayCtx { allay: true, has_items: a.stored.is_some(), deposit, wanted_item: wanted, throw: None };
    }

    /// After its goals: heals one every ten ticks, and the throw.
    pub(super) fn allay_settle(&mut self) {
        if self.kind != Kind::Allay {
            return;
        }
        if self.health > 0.0 && self.tick_count % 10 == 0 {
            let max = self.max_health();
            heal(&mut self.health, max, 1.0);
        }
        let ctx = std::mem::take(&mut self.ai.state.allay);
        self.allay.throw_at = ctx.throw;
    }
}

impl EntityWorld {
    /// Where an allay delivers: the note block it likes while it still
    /// plays to it, else the player it likes within 64 blocks.
    pub(super) fn allay_deposit(&mut self, id: u64, world: &dyn World, players: &[PlayerCandidate]) -> Option<DVec3> {
        let e = self.mobs.iter().find(|e| e.id == id && e.kind == Kind::Allay)?;
        let p = e.body.position;
        // `shouldStopDancing`, checked each second: its jukebox gone or
        // more than ten blocks off.
        if e.allay.dancing && e.tick_count % 20 == 0 {
            let gone = e.allay.jukebox.is_none_or(|c| {
                let centre = DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + 0.5, f64::from(c.2) + 0.5);
                centre.distance(p) >= 10.0 || world.block(c).is_none_or(|b| b.id != "minecraft:jukebox")
            });
            if gone {
                let e = self.mob_mut(id)?;
                e.allay.dancing = false;
                e.allay.jukebox = None;
            }
        }
        let e = self.mobs.iter().find(|e| e.id == id)?;
        if let Some(c) = e.allay.liked_note_block {
            let near = DVec3::new(f64::from(c.0), f64::from(c.1), f64::from(c.2)).distance(p) < 1024.0;
            if near && e.allay.note_block_cooldown > 0 && world.block(c).is_some_and(|b| b.id == "minecraft:note_block") {
                return Some(DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + 1.5, f64::from(c.2) + 0.5));
            }
            self.mob_mut(id)?.allay.liked_note_block = None;
        }
        let liked = self.mobs.iter().find(|e| e.id == id)?.allay.liked_player?;
        let player = players.iter().find(|q| self.uuid_of(PLAYER_TARGET + q.id) == liked && q.alive && !q.spectator)?;
        (player.position.distance(p) < 64.0).then(|| player.position + DVec3::new(0.0, f64::from(player.eye_height), 0.0))
    }

    /// `AllayAi.hearNoteblock` for the allays within sixteen blocks of a
    /// note block that played (`NOTE_BLOCK_PLAY`, their vibration range).
    pub fn note_block_played(&mut self, c: Cell) {
        let at = DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + 0.5, f64::from(c.2) + 0.5);
        for e in self.mobs.iter_mut().filter(|e| e.kind == Kind::Allay && e.health > 0.0 && e.body.position.distance(at) <= 16.0) {
            match e.allay.liked_note_block {
                None => {
                    e.allay.liked_note_block = Some(c);
                    e.allay.note_block_cooldown = FORGET_NOTE_BLOCK;
                }
                Some(liked) if liked == c => e.allay.note_block_cooldown = FORGET_NOTE_BLOCK,
                Some(_) => {}
            }
        }
    }

    /// `Allay.setJukeboxPlaying` for the allays within ten blocks of a
    /// jukebox that plays (`JUKEBOX_PLAY`) or stopped.
    pub fn jukebox_heard(&mut self, c: Cell, playing: bool) {
        let at = DVec3::new(f64::from(c.0) + 0.5, f64::from(c.1) + 0.5, f64::from(c.2) + 0.5);
        for e in self.mobs.iter_mut().filter(|e| e.kind == Kind::Allay && e.health > 0.0 && e.body.position.distance(at) <= 10.0) {
            let panicking = crate::mob_ai::panicking(&e.ai.state);
            if playing {
                if !e.allay.dancing && !panicking {
                    e.allay.jukebox = Some(c);
                    e.allay.dancing = true;
                }
            } else if e.allay.jukebox.is_none_or(|j| j == c) {
                e.allay.jukebox = None;
                e.allay.dancing = false;
            }
        }
    }

    /// `Allay.mobInteract`: an amethyst shard to a dancing allay that may
    /// duplicate, an item given (one of it), or its item taken back with
    /// what it gathered dropped. `None` when the click is not the allay's.
    pub fn allay_interact(&mut self, id: u64, player: u64, held: Option<&str>) -> Option<AllayUse> {
        let liked = self.uuid_of(PLAYER_TARGET + player);
        let e = self.mobs.iter_mut().find(|e| e.id == id && e.kind == Kind::Allay && e.health > 0.0)?;
        let at = e.body.position;
        // `#duplicates_allays`, and `canDuplicate`.
        if e.allay.dancing && held == Some("minecraft:amethyst_shard") && e.allay.duplication_cooldown == 0 {
            // `duplicateAllay`: a new allay where it is, both resting from it.
            e.allay.duplication_cooldown = 6000;
            let yaw = e.yaw;
            e.voices.push((Voice::Event("block.amethyst_block.chime", 2.0, 1.0), at));
            self.spawned.mobs.push(("minecraft:allay".to_owned(), at, yaw, "{PersistenceRequired:1b,DuplicationCooldown:6000L}".to_owned()));
            self.entity_events.push((id, 18));
            return Some(AllayUse::Duplicated);
        }
        match (&e.hand, held) {
            (None, Some(item)) => {
                e.hand = Some(item.to_owned());
                e.allay.liked_player = Some(liked);
                e.voices.push((Voice::Event("entity.allay.item_given", 2.0, 1.0), at));
                Some(AllayUse::Given)
            }
            (Some(_), None) => {
                let item = e.hand.take()?;
                e.allay.liked_player = None;
                e.voices.push((Voice::Event("entity.allay.item_taken", 2.0, 1.0), at));
                if let Some((id, count)) = e.allay.stored.take() {
                    self.spawned.items.push((at, ItemStack::new(&id, count.clamp(1, 64) as u8)));
                }
                Some(AllayUse::Taken(item))
            }
            _ => None,
        }
    }

    /// After its tick: the items it gathers (`InventoryCarrier.pickUpItem`,
    /// a block about it every way) and the one it threw
    /// (`AllayAi.throwItem`).
    pub(super) fn allay_step(&mut self, id: u64, world: &mut impl World) {
        let Some(e) = self.mobs.iter_mut().find(|e| e.id == id) else { return };
        if let Some(to) = e.allay.throw_at.take() {
            if let Some((item, count)) = e.allay.stored.take() {
                if count > 1 {
                    e.allay.stored = Some((item.clone(), count - 1));
                }
                e.allay.pickup_cooldown = PICKUP_COOLDOWN;
                // `BehaviorUtils.throwItem` toward a block over the mark.
                let from = e.body.position + DVec3::new(0.0, f64::from(e.eye_height()) - 0.3, 0.0);
                let scale = DVec3::new(0.2, 0.3, 0.2);
                let velocity = ((to + DVec3::Y - e.body.position) * scale).normalize_or_zero() * scale;
                world.spawn_item(from, &ItemStack::new(&item, 1), velocity, 40);
                if self.game_time % 7 == 0 && e.random.next_double() < 0.9 {
                    let pitch = THROW_PITCHES[e.random.next_int(THROW_PITCHES.len() as u32) as usize];
                    let at = e.position();
                    e.voices.push((Voice::Event("entity.allay.throw", 1.0, pitch), at));
                }
            }
        }
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return };
        let Some(held) = e.hand.clone() else { return };
        if !self.mob_griefing || e.allay.pickup_cooldown > 0 || e.health <= 0.0 {
            return;
        }
        let half = f64::from(e.body.width) / 2.0;
        let (min, max) = (e.body.position - DVec3::new(half + 1.0, 1.0, half + 1.0), e.body.position + DVec3::new(half + 1.0, f64::from(e.body.height) + 1.0, half + 1.0));
        for item in world.items_in(min, max) {
            if item.count <= 0 || item.pickup_delay > 0 || item.item != held {
                continue;
            }
            let Some(e) = self.mobs.iter_mut().find(|e| e.id == id) else { return };
            let have = e.allay.stored.as_ref().map_or(0, |(_, c)| *c);
            if !can_store(&e.allay.stored, &item.item) {
                return;
            }
            let taken = item.count.min(64 - have);
            e.allay.stored = Some((item.item.clone(), have + taken));
            let at = e.position();
            e.voices.push((Voice::Event("entity.item.pickup", 0.2, 1.0), at));
            world.take_item(item.id, taken);
        }
    }
}

/// What a click on an allay did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllayUse {
    /// It took one of the held item.
    Given,
    /// It handed back its item.
    Taken(String),
    /// It made a second allay of an amethyst shard.
    Duplicated,
}
