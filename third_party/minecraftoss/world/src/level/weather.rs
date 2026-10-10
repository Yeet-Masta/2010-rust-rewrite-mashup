//! Thunderstorms on the server level (26.3 `ServerLevel.tickThunder`,
//! `findLightningTargetAround`, `findLightningRod`) and what a lightning
//! bolt does where it lands (`LightningBolt.spawnFire`, `thunderHit` on the
//! level's own entities).
//!
//! Lightning rods (points of interest) and living mobs live with the entity
//! world, so a column the thunder draws waits in [`Level::pending_thunder`]
//! for the server, which resolves it with [`Level::strike_thunder`] once the
//! level's tick is over; and each tick of a bolt's hits waits in
//! [`Level::lightning_hits`] for the mobs and players.

use super::entity::{EntityKind, LightningData};
use super::physics::Aabb;
use super::Level;
use minecraftoss_core::random::RandomSource;
use minecraftoss_core::{BlockPos, ChunkPos, HeightmapKind};

/// A tick of a bolt's hits (`LightningBolt.tick` while `life >= 0`, not
/// visual only), for the entities the level does not hold.
#[derive(Clone, Copy, Debug)]
pub struct LightningHit {
    pub bolt: i32,
    pub pos: [f64; 3],
}

impl Level<'_> {
    /// `ServerLevel.tickThunder` once its odds came up: the column
    /// (`getBlockRandomPos`) waits for the server to find its target.
    pub(super) fn draw_thunder(&mut self, chunk: ChunkPos) {
        let at = self.block_random_pos(chunk.min_block_x(), 0, chunk.min_block_z());
        self.pending_thunder.push(at);
    }

    /// The rest of `tickThunder` for a drawn column: the bolt lands on the
    /// target `findLightningTargetAround` picks when it rains there. `rods`
    /// are the lightning rods' points of interest within 128 blocks of the
    /// column's surface (`PoiManager.findClosest`'s range), `living` the
    /// living entities' feet. Returns the bolt's entity ID.
    pub fn strike_thunder(&mut self, pos: BlockPos, rods: &[BlockPos], living: &[[f64; 3]], spawn_mobs: bool) -> Option<i32> {
        let target = self.lightning_target_around(pos, rods, living);
        if !self.is_raining_at(target) {
            return None;
        }
        // The skeleton trap's roll (`SPAWN_MOBS && nextDouble() < ...`);
        // the trap itself is not simulated.
        if spawn_mobs {
            self.random.next_f64();
        }
        Some(self.summon_lightning([f64::from(target.x) + 0.5, f64::from(target.y), f64::from(target.z) + 0.5]))
    }

    /// `ServerLevel.findLightningTargetAround`: the closest lightning rod on
    /// top of its column (`findLightningRod`), else a random living entity
    /// under the open sky in the column, else the surface.
    fn lightning_target_around(&mut self, pos: BlockPos, rods: &[BlockPos], living: &[[f64; 3]]) -> BlockPos {
        use minecraftoss_generator::feature::World;
        let mut center = self.lightning_column(pos);
        let distance = |p: &BlockPos| {
            let (dx, dy, dz) = (i64::from(p.x - center.x), i64::from(p.y - center.y), i64::from(p.z - center.z));
            dx * dx + dy * dy + dz * dz
        };
        let rod = rods
            .iter()
            .filter(|r| distance(r) <= 128 * 128 && r.y == self.height_at(HeightmapKind::WorldSurface, r.x, r.z) - 1)
            .min_by_key(|r| distance(r));
        if let Some(rod) = rod {
            return rod.above();
        }
        // `AABB.encapsulatingFullBlocks(center, center.atY(maxY + 1)).inflate(3)`.
        let top = f64::from(self.max_y() + 2);
        let (x, y, z) = (f64::from(center.x), f64::from(center.y), f64::from(center.z));
        let area = Aabb::new(x - 3.0, y - 3.0, z - 3.0, x + 4.0, top + 3.0, z + 4.0);
        let mut seen = Vec::new();
        for feet in living {
            let block = BlockPos::new(feet[0].floor() as i32, feet[1].floor() as i32, feet[2].floor() as i32);
            let inside = (0..3).all(|i| feet[i] >= area.min[i] && feet[i] < area.max[i]);
            if inside && self.sky_light(block) >= 15 {
                seen.push(block);
            }
        }
        if !seen.is_empty() {
            let pick = self.random.next_i32_bound(seen.len() as i32) as usize;
            return seen[pick];
        }
        if center.y == self.min_y() - 1 {
            center = center.above().above();
        }
        center
    }

    /// The motion-blocking surface of a drawn column, around which
    /// `findLightningTargetAround` looks.
    pub fn lightning_column(&self, pos: BlockPos) -> BlockPos {
        use minecraftoss_generator::feature::World;
        BlockPos::new(pos.x, self.height_at(HeightmapKind::MotionBlocking, pos.x, pos.z), pos.z)
    }

    /// `LightningBolt.spawnFire`: fire where the bolt stands and at
    /// `extra` random neighbours, where fire may spread, in air it survives.
    pub(super) fn lightning_fire(&mut self, data: &LightningData, pos: [f64; 3], extra: i32) {
        use minecraftoss_generator::feature::update::base_fire_state;
        use minecraftoss_generator::feature::Ctx;
        if data.visual_only {
            return;
        }
        let at = BlockPos::new(pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32);
        if !self.can_spread_fire_around(at) {
            return;
        }
        let light = |level: &mut Self, p: BlockPos| {
            let lib = level.lib;
            let fire = base_fire_state(&Ctx { lib, region: &mut *level }, p);
            if level.registries().blocks.is_air(level.block(p)) && level.can_survive_state(fire, p) {
                level.set_block_and_update(p, fire);
            }
        };
        light(self, at);
        for _ in 0..extra {
            let dx = self.entity_random.next_i32_bound(3) - 1;
            let dy = self.entity_random.next_i32_bound(3) - 1;
            let dz = self.entity_random.next_i32_bound(3) - 1;
            light(self, at.offset(dx, dy, dz));
        }
    }

    /// `thunderHit` on the level's item entities and experience orbs near
    /// a bolt (`hurtServer` for 5: both have 5 health), and the hit kept
    /// for the mobs and players.
    pub(super) fn lightning_hit_entities(&mut self, bolt: i32, pos: [f64; 3]) {
        let area = Aabb::new(pos[0] - 3.0, pos[1] - 3.0, pos[2] - 3.0, pos[0] + 3.0, pos[1] + 9.0, pos[2] + 3.0);
        for e in self.entities.iter_mut().filter(|e| !e.removed && e.id != bolt && e.bb.intersects(&area)) {
            let health = match &mut e.kind {
                EntityKind::Item(data) => &mut data.health,
                EntityKind::ExperienceOrb(data) => &mut data.health,
                _ => continue,
            };
            *health -= 5;
            if *health <= 0 {
                e.removed = true;
            }
        }
        self.lightning_hits.push(LightningHit { bolt, pos });
    }
}
