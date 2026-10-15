//! Shulkers (26.3 `Shulker` and its goals).
//!
//! - A shulker clings to one face of its cell (`AttachFace`), always at the
//!   cell's middle and never moving; when that face no longer holds it it
//!   takes another, or teleports (`findNewAttachment`).
//! - Its lid opens to the peek its goals ask (`DATA_PEEK_ID`, eased 0.05 a
//!   tick), with the open and close sounds; shut, it wears 20 armour more
//!   (`COVERED_ARMOR_MODIFIER`) and arrows glance off it.
//! - With a target in reach it opens wide and fires a bullet every one to
//!   five and a half seconds (`ShulkerAttackGoal`); alone it peeks now and
//!   then (`ShulkerPeekGoal`). It targets players within its follow range,
//!   but only four blocks out along the axis it clings on.
//! - Hurt below half health, a quarter of the time it teleports up to eight
//!   blocks off to a face it can cling to (`teleportSomewhere`); struck open
//!   by a shulker bullet it teleports and leaves a new shulker behind,
//!   less likely the more are about (`hitByShulkerBullet`).
use super::*;
use super::mobs::MobEntity;
use crate::goals::{Control, Controls, Goal, GoalSelector};
use crate::monster_ai::{HurtByTargetGoal, LookAtPlayerGoal, MonsterGoalContext, RandomLookGoal};
use crate::species::Kind;

/// `Direction` by its 3D data value: down, up, north, south, west, east.
pub const FACES: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

/// What a hurt left for its next tick (`hurtServer`'s teleports).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Pending {
    #[default]
    None,
    Teleport,
    Bullet,
}

/// A shulker's `AttachFace`, raw peek (`DATA_PEEK_ID`) and the eased
/// `currentPeekAmount` (with last tick's).
#[derive(Clone, Copy, Debug, Default)]
pub struct ShulkerState {
    pub face: u8,
    pub peek: i32,
    pub amount: f32,
    pub amount_o: f32,
    pending: Pending,
}

impl ShulkerState {
    /// `isClosed`.
    pub fn closed(&self) -> bool {
        self.peek == 0
    }

    /// `getClientPeekAmount`.
    pub fn peek_amount(&self, partial: f32) -> f32 {
        self.amount_o + (self.amount - self.amount_o) * partial
    }

    /// `getPhysicalPeek`: how far out of its shell its box reaches.
    fn physical(amount: f32) -> f32 {
        0.5 - ((0.5 + amount) * std::f32::consts::PI).sin() * 0.5
    }
}

/// `Shulker.registerGoals`' goal selector.
pub fn goals() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, LookAtPlayerGoal { range: 8.0 });
    selector.add(4, ShulkerAttackGoal { attack_time: 0 });
    selector.add(7, ShulkerPeekGoal { peek_time: 0 });
    selector.add(8, RandomLookGoal);
    selector
}

/// `Shulker.registerGoals`' target selector (the defence goal needs a
/// team, which there are none of here).
pub fn targets() -> GoalSelector<MonsterGoalContext> {
    let mut selector = GoalSelector::default();
    selector.add(1, HurtByTargetGoal { timestamp: 0, unseen_ticks: 0, target_mob: None, alert_others: true });
    selector.add(2, ShulkerNearestAttackGoal { unseen_ticks: 0 });
    selector
}

/// `ShulkerAttackGoal`: open wide and shoot while the target lives and is
/// within 20 blocks (out of reach, the target is dropped).
#[derive(Clone)]
pub(crate) struct ShulkerAttackGoal {
    attack_time: i32,
}
impl Goal<MonsterGoalContext> for ShulkerAttackGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Move, Control::Look])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target().is_some_and(|t| t.alive) && ctx.difficulty != 0
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        self.attack_time = 20;
        ctx.peek = 100;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.peek = 0;
    }
    fn tick(&mut self, ctx: &mut MonsterGoalContext) {
        if ctx.difficulty == 0 {
            return;
        }
        self.attack_time -= 1;
        let Some(target) = ctx.target() else { return };
        ctx.look_control.set_look_at_with_limits(target.position + DVec3::new(0.0, f64::from(target.eye_height), 0.0), 180.0, 180.0);
        if ctx.body.position.distance_squared(target.position) < 400.0 {
            if self.attack_time <= 0 {
                self.attack_time = 20 + ctx.random.next_int(10) as i32 * 20 / 2;
                ctx.shoot = Some((target.target, 0.0));
            }
        } else {
            ctx.clear_target();
        }
    }
}

/// `ShulkerPeekGoal`: alone and able to stay, one goal tick in twenty it
/// opens a little for one to three seconds.
#[derive(Clone)]
pub(crate) struct ShulkerPeekGoal {
    peek_time: i32,
}
impl Goal<MonsterGoalContext> for ShulkerPeekGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target.is_none() && ctx.random.next_int(20) == 0 && ctx.can_stay
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, _world: &dyn World) -> bool {
        ctx.target.is_none() && self.peek_time > 0
    }
    fn start(&mut self, ctx: &mut MonsterGoalContext) {
        // `adjustedTickDelay`: in goal ticks.
        self.peek_time = (20 * (1 + ctx.random.next_int(3) as i32) + 1) / 2;
        ctx.peek = 30;
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        if ctx.target.is_none() {
            ctx.peek = 0;
        }
    }
    fn tick(&mut self, _ctx: &mut MonsterGoalContext) {
        self.peek_time -= 1;
    }
}

/// `ShulkerNearestAttackGoal`: the nearest player it can see in its box
/// grown by its follow range, but by 4 along the axis it clings on;
/// never in peaceful.
#[derive(Clone)]
pub(crate) struct ShulkerNearestAttackGoal {
    unseen_ticks: i32,
}
impl Goal<MonsterGoalContext> for ShulkerNearestAttackGoal {
    fn controls(&self) -> Controls {
        Controls::new(&[Control::Target])
    }
    fn can_start(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        if ctx.difficulty == 0 || ctx.random.next_int(5) != 0 {
            return false;
        }
        let range = ctx.follow_range;
        let axis = FACES[usize::from(ctx.attach_face.min(5))];
        let grow = DVec3::new(if axis.0 != 0 { 4.0 } else { range }, if axis.1 != 0 { 4.0 } else { range }, if axis.2 != 0 { 4.0 } else { range });
        let p = ctx.body.position;
        let (min, max) = (p - DVec3::new(0.5, 0.0, 0.5) - grow, p + DVec3::new(0.5, 1.0, 0.5) + grow);
        let mut best: Option<(f64, crate::monster_ai::Target)> = None;
        for player in ctx.players.clone() {
            let q = player.position;
            let inside = q.x + 0.3 > min.x && q.x - 0.3 < max.x && q.y + 1.8 > min.y && q.y < max.y && q.z + 0.3 > min.z && q.z - 0.3 < max.z;
            let Some(info) = ctx.info(crate::monster_ai::Target::Player(player.id)) else { continue };
            if !inside || !ctx.can_attack(info) || !ctx.sees(world, info) {
                continue;
            }
            let distance = p.distance_squared(q);
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, info.target));
            }
        }
        let Some((_, target)) = best else { return false };
        ctx.set_target(Some(target));
        true
    }
    fn can_continue(&mut self, ctx: &mut MonsterGoalContext, world: &dyn World) -> bool {
        let remembered = ctx.target;
        crate::monster_ai::continue_target(ctx, world, remembered, &mut self.unseen_ticks, Some(60))
    }
    fn stop(&mut self, ctx: &mut MonsterGoalContext) {
        ctx.clear_target();
    }
}

impl MobEntity {
    /// Whether the shulker could cling at `cell` by `face` (`canStayAt`):
    /// the cell is empty, the block beyond that face is sturdy, and the
    /// shell fully open would meet nothing.
    fn shulker_can_stay(&self, world: &impl World, cell: (i32, i32, i32), face: u8) -> bool {
        let air = |pos: (i32, i32, i32)| world.block(pos).is_none_or(|b| b.id == "minecraft:air" || b.id == "minecraft:cave_air" || b.id == "minecraft:void_air");
        if !air(cell) {
            return false;
        }
        let (dx, dy, dz) = FACES[usize::from(face)];
        if !world.solid((cell.0 + dx, cell.1 + dy, cell.2 + dz)) {
            return false;
        }
        // Open, it reaches a block out the other way.
        air((cell.0 - dx, cell.1 - dy, cell.2 - dz))
    }

    /// `findAttachableSurface`: the first face, in `Direction` order, it
    /// could cling by.
    fn shulker_surface(&self, world: &impl World, cell: (i32, i32, i32)) -> Option<u8> {
        (0..6u8).find(|&face| self.shulker_can_stay(world, cell, face))
    }

    /// `teleportSomewhere`: five tries at a random empty cell up to eight
    /// off in each axis with a face to cling to; there it clings, shut,
    /// targetless, with the teleport sound. Whether it went.
    fn shulker_teleport(&mut self, world: &impl World) -> bool {
        if self.no_ai || self.health <= 0.0 {
            return false;
        }
        let p = self.body.position;
        let old = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        for _ in 0..5 {
            let (dx, dy, dz) = (self.random.next_int(17) as i32 - 8, self.random.next_int(17) as i32 - 8, self.random.next_int(17) as i32 - 8);
            let cell = (old.0 + dx, old.1 + dy, old.2 + dz);
            if cell.1 <= world.min_y() || !world.collision_boxes(cell).is_empty() {
                continue;
            }
            let Some(face) = self.shulker_surface(world, cell) else { continue };
            // `#shulker_does_not_teleport_to`: not onto other shulkers' boxes.
            let (fx, fy, fz) = FACES[usize::from(face)];
            if world.block((cell.0 + fx, cell.1 + fy, cell.2 + fz)).is_some_and(|b| b.id.ends_with("shulker_box")) {
                continue;
            }
            self.shulker.face = face;
            self.body.position = DVec3::new(f64::from(cell.0) + 0.5, f64::from(cell.1), f64::from(cell.2) + 0.5);
            self.shulker.peek = 0;
            self.ai.state.peek = 0;
            self.ai.state.clear_target();
            let at = self.body.position;
            self.voices.push((Voice::Event("entity.shulker.teleport", 1.0, 1.0), at));
            return true;
        }
        false
    }

    /// `Shulker.hurtServer`'s roll after a hit lands: below half health a
    /// quarter of the time it will teleport.
    pub(super) fn shulker_hurt(&mut self) {
        if self.kind == Kind::Shulker && self.health > 0.0 && self.health < self.max_health() * 0.5 && self.random.next_int(4) == 0 {
            self.shulker.pending = Pending::Teleport;
        }
    }

    /// The covered armour (`COVERED_ARMOR_MODIFIER`) while shut.
    pub fn shulker_armor(&self) -> f32 {
        if self.kind == Kind::Shulker && self.shulker.closed() {
            20.0
        } else {
            0.0
        }
    }

    /// Whether an arrow glances off it: shut (`hurtServer`).
    pub fn deflects_arrows(&self) -> bool {
        self.kind == Kind::Shulker && self.shulker.closed()
    }
}

impl EntityWorld {
    /// A shulker bullet struck `id` (`hitByShulkerBullet`, unless the hit
    /// already sent it off).
    pub(super) fn shulker_bullet_hit(&mut self, id: u64) {
        if let Some(e) = self.mob_mut(id).filter(|e| e.kind == Kind::Shulker && e.shulker.pending == Pending::None) {
            e.shulker.pending = Pending::Bullet;
        }
    }

    /// A shulker's own tick (`Shulker.tick`): it holds to its cell's middle,
    /// works what a hurt left, finds new footing when its face gives way,
    /// takes the peek its goals asked (with the sound) and eases its lid,
    /// its box rising as a floor shulker opens.
    pub(super) fn shulker_step(&mut self, id: u64, world: &impl World) {
        let Some(index) = self.mobs.iter().position(|e| e.id == id) else { return };
        let e = &mut self.mobs[index];
        if e.kind != Kind::Shulker {
            return;
        }
        let p = e.body.position;
        e.body.position = DVec3::new(p.x.floor() + 0.5, (p.y + 0.5).floor(), p.z.floor() + 0.5);
        e.body.velocity = DVec3::ZERO;
        let old = e.body.position;
        let mut baby = None;
        match std::mem::take(&mut e.shulker.pending) {
            Pending::Teleport => {
                e.shulker_teleport(world);
            }
            Pending::Bullet => {
                if !e.shulker.closed() && e.shulker_teleport(world) {
                    baby = Some(old);
                }
            }
            Pending::None => {}
        }
        let e = &mut self.mobs[index];
        let pos = e.body.position;
        let cell = (pos.x.floor() as i32, pos.y.floor() as i32, pos.z.floor() as i32);
        if e.health > 0.0 && !e.shulker_can_stay(world, cell, e.shulker.face) {
            match e.shulker_surface(world, cell) {
                Some(face) => e.shulker.face = face,
                None => {
                    e.shulker_teleport(world);
                }
            }
        }
        // `setRawPeekAmount`: the goals' ask, with its sound.
        let asked = e.ai.state.peek.clamp(0, 100);
        if asked != e.shulker.peek {
            let at = e.body.position;
            e.voices.push((Voice::Event(if asked == 0 { "entity.shulker.close" } else { "entity.shulker.open" }, 1.0, 1.0), at));
            e.shulker.peek = asked;
        }
        // `updatePeekAmount`.
        let state = &mut e.shulker;
        state.amount_o = state.amount;
        let wanted = state.peek as f32 * 0.01;
        if state.amount > wanted {
            state.amount = (state.amount - 0.05).clamp(wanted, 1.0);
        } else if state.amount < wanted {
            state.amount = (state.amount + 0.05).clamp(0.0, wanted);
        }
        e.body.height = if state.face == 0 { 1.0 + ShulkerState::physical(state.amount) } else { 1.0 };
        e.ai.state.attach_face = state.face;
        // A new shulker where the old one stood, less likely the more
        // shulkers are within its box grown by 8.
        if let Some(at) = baby {
            let count = self.mobs.iter().filter(|m| m.kind == Kind::Shulker && m.health > 0.0 && (m.body.position - at).abs().max_element() <= 8.5).count();
            let failure = (count as f32 - 1.0) / 5.0;
            let roll = self.level_random.next_float();
            if roll >= failure {
                let variant = self.mobs[index].variant.clone();
                let variant_key = self.mobs[index].variant_key;
                let child = self.spawn_mob(Kind::Shulker, at, 0.0, 0, false);
                if let Some(c) = self.mob_mut(child) {
                    c.variant = variant;
                    c.variant_key = variant_key;
                }
            }
        }
    }

    /// Before its goals: what they read of the shulker (its peek and face,
    /// and whether it may stay where it is).
    pub(super) fn shulker_prepare(&mut self, id: u64, world: &impl World) {
        let Some(e) = self.mob_mut(id).filter(|e| e.kind == Kind::Shulker) else { return };
        let pos = e.body.position;
        let cell = (pos.x.floor() as i32, pos.y.floor() as i32, pos.z.floor() as i32);
        e.ai.state.can_stay = e.shulker_can_stay(world, cell, e.shulker.face);
        e.ai.state.peek = e.shulker.peek;
        e.ai.state.attach_face = e.shulker.face;
    }
}
