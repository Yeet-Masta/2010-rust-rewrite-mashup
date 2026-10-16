//! Armadillos (26.3 `Armadillo`, `ArmadilloAi`'s ball-up and roll-out,
//! the `ARMADILLO_SCARE_DETECTED` sensor).
//!
//! - Four times a second it looks about: an undead thing, what last hurt
//!   it, or a sprinting or riding player within seven blocks across and two
//!   up and down keeps it wary for four seconds. Wary on the ground, it
//!   rolls up (half a second), lies scared in its shell (peeking out now and
//!   then while the danger is about), and when the danger has passed
//!   unrolls (a second and a half) and goes on.
//! - Rolled up it takes half of what hurts it less one, makes no ambient
//!   sound and does not move; fire or the like makes it unroll and run.
//! - Every five to ten minutes it sheds a scute; a brush takes one off a
//!   grown armadillo at once.
use super::*;
use super::mobs::MobEntity;
use crate::species::Kind;

/// `ArmadilloState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArmadilloPhase {
    #[default]
    Idle,
    Rolling,
    Scared,
    Unrolling,
}

impl ArmadilloPhase {
    /// `animationDuration`.
    pub fn duration(self) -> i64 {
        match self {
            Self::Idle => 0,
            Self::Rolling => 10,
            Self::Scared => 50,
            Self::Unrolling => 30,
        }
    }

    /// `isThreatened`.
    pub fn threatened(self) -> bool {
        self != Self::Idle
    }

    /// `shouldHideInShell`.
    pub fn hides(self, ticks: i64) -> bool {
        match self {
            Self::Idle => false,
            Self::Rolling => ticks > 5,
            Self::Scared => true,
            Self::Unrolling => ticks < 26,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Rolling => "rolling",
            Self::Scared => "scared",
            Self::Unrolling => "unrolling",
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name {
            "rolling" => Self::Rolling,
            "scared" => Self::Scared,
            "unrolling" => Self::Unrolling,
            _ => Self::Idle,
        }
    }
}

/// An armadillo's shell and memories.
#[derive(Clone, Debug, Default)]
pub struct ArmadilloState {
    pub phase: ArmadilloPhase,
    pub in_state_ticks: i64,
    /// The tick its phase began, and its last peek (entity event 64).
    pub phase_start: i32,
    pub peek_start: Option<i32>,
    /// `scuteTime`; none until first set.
    pub scute_time: Option<i32>,
    /// `DANGER_DETECTED_RECENTLY`'s ticks left (none at nought).
    pub danger: i32,
    /// `ArmadilloBallUp` running, its ticks, peek timer and last danger.
    pub balled: Option<i64>,
    pub next_peek: i32,
    pub danger_was_around: bool,
    pub hurt_seen: Option<i32>,
    /// A scute shed this tick.
    pub shed: bool,
}

impl MobEntity {
    /// `isScared`.
    pub fn armadillo_scared(&self) -> bool {
        self.kind == Kind::Armadillo && self.armadillo.phase != ArmadilloPhase::Idle
    }

    fn armadillo_switch(&mut self, phase: ArmadilloPhase) {
        self.armadillo.phase = phase;
        self.armadillo.in_state_ticks = 0;
        self.armadillo.phase_start = self.tick_count;
    }

    fn armadillo_sound(&mut self, event: &'static str) {
        let baby = self.baby();
        let pitch = voice_pitch(&mut self.random, baby);
        self.voices.push((Voice::Event(event, 1.0, pitch), self.body.position));
    }

    /// `rollUp`: it stops where it is, forgets love, and curls up.
    fn armadillo_roll_up(&mut self) {
        if self.armadillo_scared() {
            return;
        }
        self.ai.state.navigation.stop();
        self.ai.forward = 0.0;
        self.ai.sideways = 0.0;
        self.ai.speed = 0.0;
        self.in_love = 0;
        self.armadillo_sound("entity.armadillo.roll");
        self.armadillo_switch(ArmadilloPhase::Rolling);
    }

    /// `rollOut`.
    fn armadillo_roll_out(&mut self) {
        if !self.armadillo_scared() {
            return;
        }
        self.armadillo_sound("entity.armadillo.unroll_finish");
        self.armadillo_switch(ArmadilloPhase::Idle);
    }

    /// `rangeOfSeconds(300, 600)`'s draw for the next scute.
    fn armadillo_next_scute(&mut self) -> i32 {
        self.random.next_int(6000) as i32 + 6000
    }

    /// Before its goals: its brain (`ArmadilloAi`) and `customServerAiStep`'s
    /// scute. `scary` is the sensor's look about, when it looks; `leashed`
    /// whether a lead holds it.
    pub(super) fn armadillo_prepare(&mut self, scary: Option<bool>, leashed: bool) {
        if self.kind != Kind::Armadillo {
            return;
        }
        if self.armadillo.scute_time.is_none() {
            let next = self.armadillo_next_scute();
            self.armadillo.scute_time = Some(next);
        }
        self.armadillo.in_state_ticks += 1;
        let in_liquid = self.body.touching_water;
        // `isPanicking` for an armadillo: fire and the like.
        let panicking = self.body.fire_ticks > 0;
        let can_stay = !panicking && !in_liquid && !leashed;
        let a = &mut self.armadillo;
        if a.danger > 0 {
            a.danger -= 1;
        }
        if let Some(scary) = scary {
            if !can_stay {
                a.danger = 0;
            } else if scary {
                a.danger = 80;
            }
        }
        // `actuallyHurt` by a living thing.
        let hurt = self.ai.state.hurt_by.map(|(_, when)| when);
        let fresh = hurt.is_some() && hurt != a.hurt_seen;
        a.hurt_seen = hurt;
        if fresh {
            a.danger = 80;
            if can_stay {
                self.armadillo_roll_up();
            }
        }
        // `ArmadilloPanic.start` and `ARMADILLO_ROLLING_OUT`.
        if panicking || self.armadillo.danger == 0 {
            self.armadillo_roll_out();
        }
        // The PANIC activity: `ArmadilloBallUp` while danger is about and it
        // does not panic.
        let active = self.armadillo.danger > 0 && !panicking;
        match self.armadillo.balled {
            None if active && self.body.on_ground && !in_liquid => {
                self.armadillo.balled = Some(0);
                self.armadillo_roll_up();
            }
            Some(ticks) if active && self.armadillo.phase.threatened() && ticks < 6000 => {
                self.armadillo.balled = Some(ticks + 1);
                self.armadillo_ball_tick();
            }
            Some(_) => {
                self.armadillo.balled = None;
                if !can_stay {
                    self.armadillo_roll_out();
                }
            }
            None => {}
        }
        // The scute.
        if self.health > 0.0 {
            let left = self.armadillo.scute_time.unwrap_or(0) - 1;
            if left <= 0 {
                self.armadillo.shed = true;
                let pitch = (self.random.next_float() - self.random.next_float()) * 0.2 + 1.0;
                self.voices.push((Voice::Event("entity.armadillo.scute_drop", 1.0, pitch), self.body.position));
                let next = self.armadillo_next_scute();
                self.armadillo.scute_time = Some(next);
            } else {
                self.armadillo.scute_time = Some(left);
            }
        }
    }

    /// `ArmadilloBallUp.tick`.
    fn armadillo_ball_tick(&mut self) {
        if self.armadillo.next_peek > 0 {
            self.armadillo.next_peek -= 1;
        }
        if self.armadillo.phase == ArmadilloPhase::Rolling && self.armadillo.in_state_ticks > ArmadilloPhase::Rolling.duration() {
            self.armadillo_switch(ArmadilloPhase::Scared);
            if self.body.on_ground {
                self.armadillo_sound("entity.armadillo.land");
            }
            return;
        }
        let danger = i64::from(self.armadillo.danger);
        let around = danger > 75;
        if around != self.armadillo.danger_was_around {
            self.armadillo.next_peek = 50 + self.random.next_int(301) as i32 + 100;
        }
        self.armadillo.danger_was_around = around;
        match self.armadillo.phase {
            ArmadilloPhase::Scared => {
                if self.armadillo.next_peek == 0 && self.body.on_ground && around {
                    // Entity event 64: the peek and its sound.
                    self.armadillo.peek_start = Some(self.tick_count);
                    self.voices.push((Voice::Event("entity.armadillo.peek", 1.0, 1.0), self.body.position));
                    self.armadillo.next_peek = 50 + self.random.next_int(301) as i32 + 100;
                }
                if danger < ArmadilloPhase::Unrolling.duration() {
                    self.armadillo_sound("entity.armadillo.unroll_start");
                    self.armadillo_switch(ArmadilloPhase::Unrolling);
                }
            }
            ArmadilloPhase::Unrolling if danger > ArmadilloPhase::Unrolling.duration() => {
                self.armadillo_switch(ArmadilloPhase::Scared);
            }
            _ => {}
        }
    }

    /// `Armadillo.hurtServer`: rolled up, it takes half less one.
    pub(super) fn armadillo_damage(&self, amount: f32) -> f32 {
        if self.armadillo_scared() {
            (amount - 1.0) / 2.0
        } else {
            amount
        }
    }
}

impl EntityWorld {
    /// `isScaredBy` over what it sees about `id`: undead within its box grown
    /// 7 across and 2 up and down, what last hurt it, or a sprinting player.
    pub(super) fn armadillo_scared_by(&self, id: u64) -> bool {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return false };
        let (p, half, height) = (e.body.position, f64::from(e.body.width) / 2.0, f64::from(e.body.height));
        let inside = |q: DVec3, w: f32, h: f32| {
            let oh = f64::from(w) / 2.0;
            (q.x - p.x).abs() < half + 7.0 + oh && (q.z - p.z).abs() < half + 7.0 + oh && q.y < p.y + height + 2.0 && q.y + f64::from(h) > p.y - 2.0
        };
        let attacker = e.ai.state.hurt_by.map(|(t, _)| t);
        let undead = |kind: &str| {
            matches!(kind, "minecraft:zombie" | "minecraft:husk" | "minecraft:drowned" | "minecraft:zombie_villager" | "minecraft:skeleton" | "minecraft:stray" | "minecraft:bogged" | "minecraft:parched")
                || crate::species::ALL.iter().any(|k| k.species().id == kind && k.species().undead)
        };
        for m in self.mob_candidates() {
            if m.id == id || !m.alive || !inside(m.position, m.width, m.height) {
                continue;
            }
            if undead(m.kind) || attacker == Some(crate::monster_ai::Target::Mob(m.id)) {
                return true;
            }
        }
        self.pushing_players.iter().any(|b| {
            let sprinting = self.player_vitals.iter().find(|(pid, _)| *pid == b.id).is_some_and(|(_, v)| v.velocity.x.hypot(v.velocity.z) > 0.25);
            b.pushable && inside(b.position, b.width, b.height) && (sprinting || attacker == Some(crate::monster_ai::Target::Player(b.id)))
        })
    }

    /// A shed scute drops (`ARMADILLO_SHED`).
    pub(super) fn armadillo_step(&mut self, id: u64) {
        let Some(e) = self.mob_mut(id) else { return };
        if std::mem::take(&mut e.armadillo.shed) {
            let at = e.body.position;
            self.spawned.items.push((at, ItemStack::new("minecraft:armadillo_scute", 1)));
        }
    }

    /// `brushOffScute`: a brush on a grown armadillo takes off a scute, with
    /// its sound. Whether it did.
    pub fn brush_armadillo(&mut self, id: u64) -> bool {
        let Some(e) = self.mobs.iter_mut().find(|e| e.id == id && e.kind == Kind::Armadillo && e.health > 0.0) else { return false };
        if e.baby() {
            return false;
        }
        let at = e.body.position;
        e.voices.push((Voice::Event("entity.armadillo.brush", 1.0, 1.0), at));
        self.spawned.items.push((at, ItemStack::new("minecraft:armadillo_scute", 1)));
        true
    }
}
