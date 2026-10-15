//! A wither's spawn-in (26.3 `WitherBoss` with `getInvulnerableTicks`
//! above zero, as `WitherSkullBlock.checkSpawn` makes it): for 220 ticks
//! `WitherDoNothingGoal` holds its moves and looks, nothing short of what
//! bypasses invulnerability hurts it (`hurtServer`), and it heals 10 every
//! ten ticks (`customServerAiStep`), from a third of its health to full.
//! When the count runs out it bursts with power 7 at its eyes
//! (`ExplosionInteraction.MOB`, no fire) and every player hears it arrive
//! (global level event 1023). The count is saved as `Invul`.
use super::mobs::MobEntity;
use super::*;
use crate::species::Kind;

/// `LevelEvent.SOUND_WITHER_BOSS_SPAWN` (a global event).
pub const WITHER_SPAWNED: i32 = 1023;
/// `WitherBoss.makeInvulnerable`'s count.
pub const INVULNERABLE_TICKS: i32 = 220;

/// What a hurt does that does not land.
pub(super) const UNHURT: DamageResult = DamageResult { applied: false, dealt: 0.0, died: false, full: false };

impl MobEntity {
    /// Still gathering its strength (`getInvulnerableTicks() > 0`).
    pub fn invulnerable(&self) -> bool {
        self.kind == Kind::Wither && self.invulnerable_ticks > 0
    }

    /// `WitherBossRenderer.scale` and `getTextureLocation` between ticks:
    /// twice a model's size, a quarter smaller as the spawn-in starts and
    /// growing through it, and whether it wears the invulnerable skin
    /// (throughout, blinking every five ticks over the last 80).
    pub fn wither_spawn_look(&self, partial: f32) -> (f32, bool) {
        let ticks = if self.invulnerable() { self.invulnerable_ticks as f32 - partial } else { 0.0 };
        let scale = 2.0 - if ticks > 0.0 { ticks / INVULNERABLE_TICKS as f32 * 0.5 } else { 0.0 };
        let whole = ticks.floor() as i32;
        (scale, whole > 0 && (whole > 80 || whole / 5 % 2 != 1))
    }

    /// One tick of the spawn-in, in place of its goals and flight: the
    /// count runs down (`customServerAiStep`), it heals 10 on every tenth
    /// tick, and it moves only as its motion and gravity take it
    /// (`aiStep`'s vertical damping by 0.6; `FlyingMoveControl` with
    /// nowhere to go lets it fall). True when the count ran out.
    pub(super) fn wither_spawn_in(&mut self, world: &impl World) -> bool {
        let left = self.invulnerable_ticks - 1;
        self.invulnerable_ticks = left;
        if self.tick_count % 10 == 0 {
            let max = self.max_health();
            heal(&mut self.health, max, 10.0);
        }
        self.body.trim_small_velocity();
        self.body.velocity.y *= 0.6;
        let gravity = self.body.gravity;
        self.body.gravity = 0.08;
        let _ = self.body.travel_air_jumping(world, DVec3::ZERO, 0.0, self.yaw, false);
        self.body.gravity = gravity;
        self.body.fall_distance = 0.0;
        self.no_action_time = 0;
        left <= 0
    }
}

impl EntityWorld {
    /// The end of a wither's spawn-in: `level.explode` at its eyes with
    /// power 7 (the blocks break where mobs may grief, as a creeper's do),
    /// and `globalLevelEvent(1023)` at its block for the clients' sound.
    pub(super) fn wither_spawn_blast(&mut self, id: u64, world: &impl World, players: &[PlayerCandidate]) {
        let Some(e) = self.mobs.iter().find(|e| e.id == id) else { return };
        let feet = e.body.position;
        let eyes = feet + DVec3::new(0.0, f64::from(e.eye_height()), 0.0);
        let blast = crate::creeper::CreeperExplosion { position: eyes, radius: 7.0, powered: false, fire: false };
        self.apply_explosion(world, id, blast, players);
        self.explosions.push(blast);
        let cell = (feet.x.floor() as i32, feet.y.floor() as i32, feet.z.floor() as i32);
        self.spawned.global_events.push((WITHER_SPAWNED, cell, 0));
    }
}
