//! Leads (26.3 `Leashable`, `Entity.interact`'s leash half, `LeadItem`,
//! `LeashFenceKnotEntity`, `PathfinderMob.closeRangeLeashBehaviour`).
//!
//! - A lead used on a mob ties it to the player; using the mob again
//!   unties it (the lead drops, unless in creative). Sneaking on another
//!   grown, leashable mob hands it the player's leashed mobs; shears cut
//!   every lead on a mob.
//! - Using a fence ties the player's leashed mobs to a knot there; using
//!   the knot takes them back, or ties more to it.
//! - Each tick a leashed mob within two blocks of slack walks to its
//!   holder; beyond six blocks less the two widths the lead pulls it like
//!   a damped spring and twists it; past twelve it snaps. A lead whose mob
//!   or holder is gone drops as an item.
use super::*;
use crate::navigation::navigate_walk_to;
use crate::species::{Category, Kind};

type Cell = (i32, i32, i32);

/// What holds a lead's other end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeashHolder {
    Player(u64),
    Mob(u64),
    /// A knot on the fence at this block.
    Knot(Cell),
}

/// A leashed mob (`Leashable.LeashData`).
#[derive(Clone, Debug)]
pub struct Leash {
    pub mob: u64,
    pub holder: LeashHolder,
    pub angular_momentum: f64,
}

/// A lead read from a save (`delayedLeashInfo`): its holder's UUID or
/// knot, found within five seconds or dropped.
#[derive(Clone, Debug)]
pub(super) struct SavedLeash {
    mob: u64,
    holder: Result<u128, Cell>,
    waited: i32,
}

/// What a use on a mob did to leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeashUse {
    /// Tied (the held lead spent when `spends`).
    Tied { spends: bool },
    /// Untied from the player.
    Untied,
    /// Shears cut its leads (the shears wear).
    Sheared,
}

const SNAP_DISTANCE: f64 = 12.0;
const ELASTIC_DISTANCE: f64 = 6.0;

/// `Leashable.canBeLeashed` by type: no `Enemy` but the hoglin and zoglin,
/// no villager, fish, turtle or panda.
fn kind_leashable(kind: &str) -> bool {
    match kind {
        "minecraft:hoglin" | "minecraft:zoglin" | "minecraft:dolphin" | "minecraft:squid" | "minecraft:glow_squid" | "minecraft:axolotl" => true,
        "minecraft:villager" | "minecraft:wandering_trader" | "minecraft:turtle" | "minecraft:panda" | "minecraft:tadpole" => false,
        "minecraft:cow" | "minecraft:mooshroom" | "minecraft:sheep" | "minecraft:pig" | "minecraft:chicken" | "minecraft:wolf" | "minecraft:iron_golem" | "minecraft:bat" => true,
        id => match crate::species::ALL.iter().find(|k| k.species().id == id) {
            Some(kind) => {
                let s = kind.species();
                s.category != Category::Monster && !s.fish && !matches!(s.category, Category::WaterCreature | Category::UndergroundWaterCreature | Category::WaterAmbient) && !s.boss
            }
            None => false,
        },
    }
}

impl EntityWorld {
    /// Whether `player` is sneaking, as last told.
    pub fn player_sneaking(&self, player: u64) -> bool {
        self.player_vitals.iter().any(|(id, v)| *id == player && v.sneaking)
    }

    /// A mob's box: width and height.
    pub fn mob_dimensions(&self, id: u64) -> Option<(f32, f32)> {
        self.mob_body(id).map(|(b, _)| (b.width, b.height))
    }

    /// The leads in the world.
    pub fn leashes(&self) -> &[Leash] {
        &self.leashes
    }

    /// The fence knots, by block.
    pub fn knots(&self) -> &[Cell] {
        &self.knots
    }

    /// What holds `mob`'s lead, if any.
    pub fn leash_holder(&self, mob: u64) -> Option<LeashHolder> {
        self.leashes.iter().find(|l| l.mob == mob).map(|l| l.holder)
    }

    /// The holder `mob` was saved with, to find once loaded
    /// (`readLeashData`): a UUID or a knot's block.
    pub fn restore_leash(&mut self, mob: u64, holder: Result<u128, Cell>) {
        self.saved_leashes.retain(|s| s.mob != mob);
        self.saved_leashes.push(SavedLeash { mob, holder, waited: 0 });
    }

    /// What `mob`'s lead is saved as (`LeashData.CODEC`): its holder's UUID
    /// or its knot's block.
    pub fn leash_save(&self, mob: u64) -> Option<Result<u128, Cell>> {
        if let Some(saved) = self.saved_leashes.iter().find(|s| s.mob == mob) {
            return Some(saved.holder);
        }
        Some(match self.leash_holder(mob)? {
            LeashHolder::Player(p) => Ok(self.uuid_of(PLAYER_TARGET + p)),
            LeashHolder::Mob(m) => Ok(self.uuid_of(m)),
            LeashHolder::Knot(cell) => Err(cell),
        })
    }

    /// The type of the entity `id`, alive or not.
    fn type_of(&self, id: u64) -> Option<&'static str> {
        self.mob_candidates().into_iter().find(|c| c.id == id).map(|c| c.kind)
    }

    /// `canBeLeashed` for the mob `id` (a wolf not while angry).
    fn can_be_leashed(&self, id: u64) -> bool {
        if let Some(w) = self.wolves.iter().find(|w| w.id == id) {
            return !w.angry(self.game_time);
        }
        self.type_of(id).is_some_and(kind_leashable)
    }

    /// The centre of a holder's box and its width, its motion, and where
    /// its rope is held.
    fn holder_state(&self, holder: LeashHolder) -> Option<(DVec3, f32, DVec3, f32)> {
        match holder {
            LeashHolder::Player(p) => {
                let player = self.pushing_players.iter().find(|c| c.id == p)?;
                let velocity = self.player_vitals.iter().find(|(id, _)| *id == p).map_or(DVec3::ZERO, |(_, v)| v.velocity);
                let alive = self.player_vitals.iter().find(|(id, _)| *id == p).is_none_or(|(_, v)| v.health > 0.0);
                // `canInteractWithLevel`: alive and no spectator.
                (alive && player.pushable).then_some((player.position + DVec3::new(0.0, f64::from(player.height) / 2.0, 0.0), player.width, velocity, player.height))
            }
            LeashHolder::Mob(m) => {
                let (body, health) = self.mob_body(m)?;
                (health > 0.0).then_some((body.position + DVec3::new(0.0, f64::from(body.height) / 2.0, 0.0), body.width, if self.mobs.iter().any(|e| e.id == m && e.no_ai) { DVec3::ZERO } else { body.velocity }, body.height))
            }
            LeashHolder::Knot(cell) => {
                self.knots.contains(&cell).then(|| (DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.375 + 0.25, f64::from(cell.2) + 0.5), 0.375, DVec3::ZERO, 0.5))
            }
        }
    }

    /// `Leashable.canHaveALeashAttachedTo`: not itself, within snapping
    /// distance, and leashable.
    fn can_attach(&self, mob: u64, holder: LeashHolder) -> bool {
        if holder == LeashHolder::Mob(mob) {
            return false;
        }
        let (Some((body, _)), Some((centre, ..))) = (self.mob_body(mob), self.holder_state(holder)) else { return false };
        let own = body.position + DVec3::new(0.0, f64::from(body.height) / 2.0, 0.0);
        own.distance(centre) <= SNAP_DISTANCE && self.can_be_leashed(mob)
    }

    /// `setLeashedTo`.
    fn set_leashed_to(&mut self, mob: u64, holder: LeashHolder) {
        let old = self.leashes.iter().position(|l| l.mob == mob);
        match old {
            Some(i) => {
                let was = self.leashes[i].holder;
                self.leashes[i].holder = holder;
                if was != holder {
                    self.leashee_removed(was);
                }
            }
            None => self.leashes.push(Leash { mob, holder, angular_momentum: 0.0 }),
        }
    }

    /// `notifyLeasheeRemoved`: a knot with nothing left on it goes.
    fn leashee_removed(&mut self, holder: LeashHolder) {
        if let LeashHolder::Knot(cell) = holder {
            if !self.leashes.iter().any(|l| l.holder == holder) {
                self.knots.retain(|&k| k != cell);
            }
        }
    }

    /// `dropLeash` (the lead dropped at the mob when `drop`) or
    /// `removeLeash`.
    fn drop_leash(&mut self, mob: u64, drop: bool) {
        let Some(i) = self.leashes.iter().position(|l| l.mob == mob) else { return };
        let leash = self.leashes.remove(i);
        if drop {
            if let Some((body, _)) = self.mob_body(mob) {
                // `spawnAtLocation`: at its feet.
                self.spawned.items.push((body.position, ItemStack::new("minecraft:lead", 1)));
            }
        }
        self.leashee_removed(leash.holder);
    }

    /// The mobs leashed to `holder` within the 32-block box about `centre`
    /// (`leashableInArea`).
    fn leashed_to_near(&self, holder: LeashHolder, centre: DVec3) -> Vec<u64> {
        self.leashes
            .iter()
            .filter(|l| l.holder == holder)
            .filter(|l| self.mob_body(l.mob).is_some_and(|(b, _)| {
                let c = b.position + DVec3::new(0.0, f64::from(b.height) / 2.0, 0.0) - centre;
                c.x.abs() <= 16.0 + f64::from(b.width) / 2.0 && c.y.abs() <= 16.0 + f64::from(b.height) / 2.0 && c.z.abs() <= 16.0 + f64::from(b.width) / 2.0
            }))
            .map(|l| l.mob)
            .collect()
    }

    fn lead_sound(&mut self, event: &'static str, at: DVec3) {
        self.sounds.push(MobSound { event: event.to_owned(), position: at, volume: 1.0, pitch: 1.0, category: "neutral" });
    }

    /// `Entity.interact`'s leash half for `player`'s use on mob `id` with
    /// `held` in hand (sneaking or not, in creative or not), or `None` when
    /// leads have nothing to do with it.
    pub fn leash_interact(&mut self, id: u64, player: u64, held: Option<&str>, sneaking: bool, creative: bool) -> Option<LeashUse> {
        let (body, health) = self.mob_body(id)?;
        let centre = body.position + DVec3::new(0.0, f64::from(body.height) / 2.0, 0.0);
        let baby = self.type_of(id).is_some() && self.mobs.iter().find(|e| e.id == id).map_or_else(|| self.is_baby(id), MobEntity::baby);
        // Sneaking: the player's leashed mobs are handed to this one.
        if sneaking && self.can_be_leashed(id) && health > 0.0 && !baby {
            let mine = self.leashed_to_near(LeashHolder::Player(player), centre);
            let mut any = false;
            for mob in mine {
                if self.can_attach(mob, LeashHolder::Mob(id)) {
                    self.set_leashed_to(mob, LeashHolder::Mob(id));
                    any = true;
                }
            }
            if any {
                self.lead_sound("item.lead.tied", body.position);
                return Some(LeashUse::Tied { spends: false });
            }
        }
        // Shears cut every lead it has (`shearOffAllLeashConnections`).
        if held == Some("minecraft:shears") {
            let mut cut: Vec<u64> = self.leashes.iter().filter(|l| l.holder == LeashHolder::Mob(id)).map(|l| l.mob).collect();
            if self.leash_holder(id).is_some() {
                cut.push(id);
            }
            if !cut.is_empty() {
                for mob in cut {
                    self.drop_leash(mob, true);
                }
                self.sounds.push(MobSound { event: "item.shears.snip".to_owned(), position: body.position, volume: 1.0, pitch: 1.0, category: "player" });
                return Some(LeashUse::Sheared);
            }
        }
        if health <= 0.0 {
            return None;
        }
        // Its own lead in the player's hand: untied.
        if self.leash_holder(id) == Some(LeashHolder::Player(player)) {
            self.drop_leash(id, !creative);
            self.lead_sound("item.lead.untied", body.position);
            return Some(LeashUse::Untied);
        }
        if held == Some("minecraft:lead") && !matches!(self.leash_holder(id), Some(LeashHolder::Player(_))) && self.can_attach(id, LeashHolder::Player(player)) {
            if self.leash_holder(id).is_some() {
                self.drop_leash(id, true);
            }
            self.set_leashed_to(id, LeashHolder::Player(player));
            self.lead_sound("item.lead.tied", body.position);
            return Some(LeashUse::Tied { spends: true });
        }
        None
    }

    /// Whether a mob other than the generic ones is a baby.
    fn is_baby(&self, id: u64) -> bool {
        self.cows.iter().find(|e| e.id == id).is_some_and(|e| e.cow.age.baby())
            || self.pigs.iter().find(|e| e.id == id).is_some_and(|e| e.pig.age.baby())
            || self.chickens.iter().find(|e| e.id == id).is_some_and(|e| e.chicken.age.baby())
            || self.sheep.iter().find(|e| e.id == id).is_some_and(|e| e.sheep.age.baby())
            || self.wolves.iter().find(|e| e.id == id).is_some_and(|e| e.wolf.age.baby())
    }

    /// Whether `player` has a leashed mob within the 32-block box about the
    /// fence at `cell` (a use there binds them).
    pub fn has_leashed_near(&self, player: u64, cell: Cell) -> bool {
        let centre = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.5, f64::from(cell.2) + 0.5);
        !self.leashed_to_near(LeashHolder::Player(player), centre).is_empty()
    }

    /// `LeadItem.bindPlayerMobs` (a use on a fence): `player`'s leashed
    /// mobs near the fence at `cell` are tied to its knot, made if need be.
    /// Whether any were.
    pub fn bind_to_fence(&mut self, player: u64, cell: Cell) -> bool {
        let centre = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.5, f64::from(cell.2) + 0.5);
        let mine = self.leashed_to_near(LeashHolder::Player(player), centre);
        if mine.is_empty() {
            return false;
        }
        let existed = self.knots.contains(&cell);
        if !existed {
            self.knots.push(cell);
        }
        let knot = LeashHolder::Knot(cell);
        let mut any = false;
        for mob in mine {
            if self.can_attach(mob, knot) {
                self.set_leashed_to(mob, knot);
                any = true;
            }
        }
        if any {
            self.lead_sound("item.lead.tied", centre);
        } else if !existed {
            self.knots.retain(|&k| k != cell);
        }
        any
    }

    /// `LeashFenceKnotEntity.interact`: `player`'s leashed mobs go to the
    /// knot, or (not sneaking, none to give) the knot's mobs go to the
    /// player. Whether anything was tied.
    pub fn knot_interact(&mut self, player: u64, cell: Cell, sneaking: bool) -> bool {
        if !self.knots.contains(&cell) {
            return false;
        }
        let knot = LeashHolder::Knot(cell);
        let centre = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.5, f64::from(cell.2) + 0.5);
        let mut attached = false;
        for mob in self.leashed_to_near(LeashHolder::Player(player), centre) {
            if self.can_attach(mob, knot) {
                self.set_leashed_to(mob, knot);
                attached = true;
            }
        }
        let mut taken = false;
        if !attached && !sneaking {
            for mob in self.leashed_to_near(knot, centre) {
                if self.can_attach(mob, LeashHolder::Player(player)) {
                    self.set_leashed_to(mob, LeashHolder::Player(player));
                    taken = true;
                }
            }
        }
        if attached || taken {
            self.lead_sound("item.lead.tied", centre);
        }
        attached || taken
    }

    /// A knot broken off (`LeashFenceKnotEntity.dropItem`): its mobs' leads
    /// drop.
    pub fn break_knot(&mut self, cell: Cell) {
        if !self.knots.contains(&cell) {
            return;
        }
        let centre = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1) + 0.375, f64::from(cell.2) + 0.5);
        for mob in self.leashes.iter().filter(|l| l.holder == LeashHolder::Knot(cell)).map(|l| l.mob).collect::<Vec<_>>() {
            self.drop_leash(mob, true);
        }
        self.knots.retain(|&k| k != cell);
        self.lead_sound("item.lead.untied", centre);
    }

    /// Walks a leashed mob toward `to` (`closeRangeLeashBehaviour`'s
    /// `moveTo`), for those with a ground navigation.
    fn leash_walk(&mut self, id: u64, to: DVec3, speed: f64, world: &impl World) {
        if let Some(e) = self.mobs.iter_mut().find(|e| e.id == id) {
            if crate::mob_ai::panicking(&e.ai.state) {
                return;
            }
            let profile = e.ai.state.profile();
            let fluid = e.ai.state.fluid;
            let _ = navigate_walk_to(&e.body, &mut e.ai.state.navigation, world, &profile, fluid, to, speed, 1);
            return;
        }
        let walk = |body: &Body, navigation: &mut crate::navigation::GroundNavigation| {
            let profile = crate::walk_path::WalkProfile::animal(body.width, body.height);
            let _ = navigate_walk_to(body, navigation, world, &profile, crate::fluid::FluidFrame::default(), to, speed, 1);
        };
        if let Some(e) = self.cows.iter_mut().find(|e| e.id == id) {
            walk(&e.cow.body.clone(), &mut e.cow.navigation);
        } else if let Some(e) = self.pigs.iter_mut().find(|e| e.id == id) {
            walk(&e.pig.body.clone(), &mut e.pig.navigation);
        } else if let Some(e) = self.chickens.iter_mut().find(|e| e.id == id) {
            walk(&e.chicken.body.clone(), &mut e.chicken.navigation);
        } else if let Some(e) = self.sheep.iter_mut().find(|e| e.id == id) {
            walk(&e.body.clone(), &mut e.navigation);
        }
    }

    /// `Leashable.tickLeash` for every lead, after the mobs' ticks.
    pub(super) fn tick_leashes(&mut self, world: &mut impl World) {
        // Saved leads find their holders, or drop after five seconds.
        let saved = std::mem::take(&mut self.saved_leashes);
        for mut s in saved {
            let holder = match s.holder {
                Err(cell) => {
                    if !self.knots.contains(&cell) {
                        self.knots.push(cell);
                    }
                    Some(LeashHolder::Knot(cell))
                }
                Ok(uuid) => {
                    if self.uuid_of(PLAYER_TARGET) == uuid && self.pushing_players.iter().any(|p| p.id == 0) {
                        Some(LeashHolder::Player(0))
                    } else {
                        self.positions().map(|(id, _)| id).find(|&id| id != s.mob && self.uuid_of(id) == uuid).map(LeashHolder::Mob)
                    }
                }
            };
            match holder {
                Some(h) if self.mob_body(s.mob).is_some() => self.set_leashed_to(s.mob, h),
                _ => {
                    s.waited += 1;
                    if s.waited > 100 {
                        if let Some((body, _)) = self.mob_body(s.mob) {
                            self.spawned.items.push((body.position, ItemStack::new("minecraft:lead", 1)));
                        }
                    } else {
                        self.saved_leashes.push(s);
                    }
                }
            }
        }
        // Knots off their fences go (`survives`).
        for cell in self.knots.clone() {
            if !world.block_in_tag(cell, "minecraft:fences") {
                self.break_knot(cell);
            }
        }
        let leashes = self.leashes.clone();
        for leash in leashes {
            let mob = leash.mob;
            let Some((body, health)) = self.mob_body(mob) else {
                // Gone from the world (unloaded or discarded) with its lead.
                self.leashes.retain(|l| l.mob != mob);
                self.leashee_removed(leash.holder);
                continue;
            };
            let holder = self.holder_state(leash.holder);
            if health <= 0.0 || holder.is_none() {
                self.drop_leash(mob, true);
                continue;
            }
            let (centre, holder_width, holder_motion, holder_height) = holder.unwrap();
            let own_centre = body.position + DVec3::new(0.0, f64::from(body.height) / 2.0, 0.0);
            let distance = own_centre.distance(centre);
            let mut momentum = leash.angular_momentum;
            if distance > SNAP_DISTANCE {
                self.lead_sound("item.lead.break", centre);
                self.drop_leash(mob, true);
                continue;
            }
            let elastic = distance > ELASTIC_DISTANCE - f64::from(holder_width) - f64::from(body.width) && {
                // `checkElasticInteractions`: one attachment each, the mob's
                // at half its height and half its width ahead.
                let yaw = self.mob_yaw(mob).to_radians();
                let lever = rot_y(DVec3::new(0.0, 0.5 * f64::from(body.height), 0.5 * f64::from(body.width)), -yaw);
                let object = body.position + lever;
                let pivot = centre - DVec3::new(0.0, f64::from(holder_height) / 2.0, 0.0) + DVec3::new(0.0, 0.5 * f64::from(holder_height), 0.0);
                let gap = object.distance(pivot);
                if gap < ELASTIC_DISTANCE {
                    false
                } else {
                    let mut displacement = (pivot - object).normalize_or_zero() * (gap - ELASTIC_DISTANCE);
                    let torque = lever.z * displacement.x - lever.x * displacement.z;
                    if body.velocity.dot(displacement) >= 0.0 {
                        displacement *= f64::from(0.3_f32);
                    }
                    momentum += 10.0 * torque;
                    let relative = holder_motion - body.velocity;
                    let push = displacement * DVec3::new(0.8, 0.2, 0.8) + relative * 0.11;
                    if let Some(b) = self.body_mut(mob) {
                        b.velocity += push;
                    }
                    true
                }
            };
            if !elastic {
                // `PathfinderMob.closeRangeLeashBehaviour`: to two blocks off.
                let towards = (centre - own_centre).normalize_or_zero() * (body.position.distance(centre - DVec3::new(0.0, f64::from(holder_height) / 2.0, 0.0)) - 2.0).max(0.0);
                let speed = if matches!(self.mobs.iter().find(|e| e.id == mob).map(|e| e.kind), Some(Kind::Llama | Kind::TraderLlama)) { 2.0 } else { 1.0 };
                self.leash_walk(mob, body.position + towards, speed, world);
            }
            // The twist, worn down by the ground's grip.
            let friction = if body.on_ground { 0.6_f32 * 0.91 } else { 0.91 };
            if let Some(e) = self.mobs.iter_mut().find(|e| e.id == mob) {
                e.yaw -= momentum as f32;
            }
            if let Some(l) = self.leashes.iter_mut().find(|l| l.mob == mob) {
                l.angular_momentum = momentum * f64::from(friction);
            }
        }
    }

    /// A mob's body yaw, where kept.
    fn mob_yaw(&self, id: u64) -> f32 {
        self.mobs.iter().find(|e| e.id == id).map_or(0.0, |e| e.yaw)
    }
}

/// `Vec3.yRot`.
fn rot_y(v: DVec3, angle: f32) -> DVec3 {
    let (sin, cos) = (f64::from(angle.sin()), f64::from(angle.cos()));
    DVec3::new(v.x * cos + v.z * sin, v.y, v.z * cos - v.x * sin)
}
