//! Lightning on the mobs (26.3 `LightningBolt.tick`'s hits and the
//! `thunderHit` overrides): every living mob near a bolt is set on fire and
//! hurt, creepers become charged, mooshrooms change colour and villagers
//! turn into witches. Pigs would become zombified piglins, which are not
//! simulated, so they take the plain hit (`convertTo` failing).

use super::*;
use crate::witch::Witch;

/// `Difficulty.PEACEFUL`.
const PEACEFUL: i32 = 0;

impl EntityWorld {
    /// Living mobs' feet (`LivingEntity`, `isAlive`), for
    /// `ServerLevel.findLightningTargetAround`.
    pub fn living_feet(&mut self) -> Vec<DVec3> {
        let mut out: Vec<DVec3> = self.bats.iter().filter(|e| e.bat.health > 0.0).map(|e| e.bat.body.position).collect();
        for key in self.order.clone() {
            if let Some((body, true)) = self.push_body(key) {
                out.push(body.position);
            }
        }
        out
    }

    /// One tick of a bolt's hits (`LightningBolt.tick` while `life >= 0`):
    /// `Entity.thunderHit` on every living mob whose box meets the bolt's
    /// (3 blocks around, 6 up), in world order. `bolt` stands for the
    /// bolt's UUID, which mooshrooms remember.
    /// Players' hits come back as hits to apply (`take_player_hits`).
    pub fn lightning_hits(&mut self, bolt: u64, center: DVec3, difficulty: i32, players: &[PlayerCandidate]) {
        let (lo, hi) = (center - DVec3::splat(3.0), center + DVec3::new(3.0, 9.0, 3.0));
        let touches = |body: &Body| {
            let half = f64::from(body.width) / 2.0;
            let p = body.position;
            p.x - half < hi.x && p.x + half > lo.x && p.y < hi.y && p.y + f64::from(body.height) > lo.y && p.z - half < hi.z && p.z + half > lo.z
        };
        let mut struck: Vec<EntityKey> = self.bats.iter().filter(|e| e.bat.health > 0.0 && touches(&e.bat.body)).map(|e| EntityKey::Bat(e.id)).collect();
        for key in self.order.clone() {
            if let Some((body, true)) = self.push_body(key) {
                if touches(body) {
                    struck.push(key);
                }
            }
        }
        for key in struck {
            self.thunder_hit(key, bolt, difficulty);
        }
        for player in players.iter().filter(|p| p.alive && !p.spectator) {
            // A standing, crouching or swimming player's box.
            let height = match player.eye_height {
                e if e >= 1.5 => 1.8,
                e if e >= 1.0 => 1.5,
                _ => 0.6,
            };
            if touches(&Body::new(player.position, 0.6, height)) {
                self.player_hits.push(PlayerHit { player_id: player.id, damage: 5.0, kind: PlayerHitKind::Lightning, source: None });
            }
        }
    }

    /// `thunderHit` by kind.
    fn thunder_hit(&mut self, key: EntityKey, bolt: u64, difficulty: i32) {
        match key {
            EntityKey::Creeper(id) => {
                // `Creeper.thunderHit`: the plain hit, then charged.
                self.plain_thunder_hit(key);
                if let Some(e) = self.creepers.iter_mut().find(|e| e.id == id) {
                    e.creeper.powered = true;
                }
            }
            EntityKey::Cow(id) if self.cows.iter().any(|e| e.id == id && e.mooshroom.is_some()) => {
                // `MushroomCow.thunderHit`: no hurt, a new colour once per bolt.
                let Some(e) = self.cows.iter_mut().find(|e| e.id == id) else { return };
                let uuid = format!("lightning-{bolt}");
                if e.mooshroom.as_mut().is_some_and(|m| m.lightning_hit(&uuid)) {
                    let position = e.cow.body.position;
                    self.sounds.push(MobSound { event: "entity.mooshroom.convert".to_owned(), position, volume: 2.0, pitch: 1.0, category: "neutral" });
                }
            }
            EntityKey::Villager(id) if difficulty != PEACEFUL => self.villager_to_witch(id),
            _ => self.plain_thunder_hit(key),
        }
    }

    /// `Entity.thunderHit`: a tick more of fire, or eight seconds of it
    /// when not burning (`remainingFireTicks` reaching 0 from vanilla's
    /// resting -1, which is 0 here), then 5 `lightning_bolt` damage.
    fn plain_thunder_hit(&mut self, key: EntityKey) {
        let body = match key {
            EntityKey::Bat(id) => self.bats.iter_mut().find(|e| e.id == id).map(|e| &mut e.bat.body),
            key => self.push_body(key).map(|(body, _)| body),
        };
        if let Some(body) = body {
            body.fire_ticks = if body.fire_ticks <= 0 { 160 } else { body.fire_ticks + 1 };
        }
        self.hurt_by_lightning(key, 5.0);
    }

    /// `hurtServer` with `lightning_bolt` (`#panic_environmental_causes`,
    /// no attacker).
    fn hurt_by_lightning(&mut self, key: EntityKey, damage: f32) -> Option<DamageResult> {
        let time = self.game_time;
        let source = DamageSourceKind::Hazard;
        let kind = "minecraft:lightning_bolt";
        match key {
            EntityKey::Bat(id) => self.bats.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage)),
            EntityKey::Zombie(id) => self.zombies.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage_after_armor(damage, 2.0, 0.0))),
            EntityKey::Skeleton(id) => self.skeletons.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage)),
            EntityKey::Creeper(id) => self.creepers.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage)),
            EntityKey::Spider(id) => self.spiders.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage)),
            EntityKey::Slime(id) => self.slimes.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage)),
            EntityKey::Enderman(id) => self.endermen.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage)),
            EntityKey::Witch(id) => self.witches.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage, false, false)),
            EntityKey::IronGolem(id) => self.iron_golems.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage)),
            EntityKey::Wolf(id) => self.wolves.iter_mut().find(|e| e.id == id).map(|e| e.hurt_from(damage, kind, None, time)),
            EntityKey::Villager(id) => self.villagers.iter_mut().find(|e| e.id == id).map(|e| e.hurt_from(damage, kind, None, time)),
            EntityKey::Cow(id) => self.cows.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage, source)),
            EntityKey::Sheep(id) => self.sheep.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage, source)),
            EntityKey::Pig(id) => self.pigs.iter_mut().find(|e| e.id == id).map(|e| e.hurt(damage, source)),
            EntityKey::Chicken(id) => self.chickens.iter_mut().find(|e| e.id == id).map(|e| e.hurt_with_source(damage, source)),
            EntityKey::Arrow(_) | EntityKey::Potion(_) => None,
        }
    }

    /// `Villager.thunderHit` off peaceful: `convertTo(WITCH,
    /// ConversionParams.single)` at its place and facing, kept for good
    /// (`setPersistenceRequired`), after it gives back its points of
    /// interest (`releaseAllPois`).
    fn villager_to_witch(&mut self, id: u64) {
        let Some(old) = self.villagers.iter().find(|e| e.id == id) else { return };
        let mut witch = Witch::new(old.villager.body.position);
        witch.body.velocity = old.villager.body.velocity;
        witch.body.on_ground = old.villager.body.on_ground;
        witch.yaw = old.ai.as_deref().map_or(old.yaw, |ai| ai.yaw);
        witch.persistence_required = true;
        let no_ai = old.no_ai;
        self.release_villager_pois(id);
        self.villagers.retain(|e| e.id != id);
        self.order.retain(|key| !matches!(key, EntityKey::Villager(v) if *v == id));
        self.spawn_witch(witch, no_ai);
    }
}
