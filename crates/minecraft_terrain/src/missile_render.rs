//! Pack-backed pinned 26.3 mob projectiles, all fully lit
//! (`getBlockLightLevel` 15):
//! - fireballs as `ThrownItemRenderer`s of the fire charge, facing the
//!   camera, a blaze's at 0.75 scale and a ghast's at 3;
//! - `WitherSkullRenderer`: the skull model turned to its heading;
//! - `ShulkerBulletRenderer`: the spark model tumbling with its age, then
//!   again half as big again in a faint translucent shell;
//! - `LlamaSpitRenderer`: the spit model turned to its heading.
use crate::{
    entity_model::{self, Placement, Posed},
    lighting::SkyLight,
    mesh::{Atlas, ChunkMesh, Vertex},
    pack::ResourceId,
};
use glam::{DVec3, Quat, Vec2, Vec3};
use minecraftoss_entities::world::{FangsEntity, MissileEntity, MissileKind};

/// The textures these draw, for the atlas.
pub const TEXTURES: [&str; 4] = ["minecraft:entity/shulker/spark", "minecraft:entity/llama/llama_spit", "minecraft:entity/wither/wither", "minecraft:entity/illager/evoker_fangs"];

/// `ProjectileUtil.rotateTowardsMovement`'s heading, in degrees: its yaw
/// and pitch along the motion.
fn heading(velocity: DVec3) -> (f32, f32) {
    let horizontal = velocity.x.hypot(velocity.z);
    let yaw = velocity.x.atan2(velocity.z).to_degrees() as f32;
    let pitch = velocity.y.atan2(horizontal).to_degrees() as f32;
    (yaw, pitch)
}

/// Draws the missiles: fireballs into the item mesh, the rest into the
/// model mesh (and the bullets' shells into the translucent one).
#[allow(clippy::too_many_arguments)]
pub fn append_missiles<'a>(
    models: &mut ChunkMesh,
    translucent: &mut ChunkMesh,
    items: &mut ChunkMesh,
    missiles: impl IntoIterator<Item = (&'a MissileEntity, DVec3)>,
    atlas: &Atlas,
    forward: Vec3,
    light: &SkyLight,
    partial: f32,
) {
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    let charge = ResourceId::parse("minecraft:item/fire_charge").ok().filter(|id| atlas.contains(id));
    for (missile, at) in missiles {
        let cell = (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
        let sky = light.get(cell) as f32;
        let age = missile.tick_count as f32 + partial;
        let (yaw, pitch) = heading(missile.velocity);
        match missile.kind {
            MissileKind::SmallFireball | MissileKind::LargeFireball => {
                let Some(sprite) = &charge else { continue };
                let scale = if missile.kind == MissileKind::SmallFireball { 0.75 } else { 3.0 };
                let [u0, v0, u1, v1] = atlas.region(sprite);
                let center = at.as_vec3() + up * 0.125 * scale;
                let start = items.vertices.len() as u32;
                for (corner, uv) in [(Vec2::new(-0.25, -0.25), [u0, v1]), (Vec2::new(0.25, -0.25), [u1, v1]), (Vec2::new(0.25, 0.25), [u1, v0]), (Vec2::new(-0.25, 0.25), [u0, v0])] {
                    let point = center + (right * corner.x + up * corner.y) * scale;
                    items.vertices.push(Vertex { position: point.to_array(), uv, color: [1.0; 4], sky_light: sky, block_light: 15.0 });
                }
                items.indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
                items.faces += 1;
            }
            MissileKind::WitherSkull => {
                let (Some(model), Ok(texture)) = (entity_model::model("minecraft:wither_skull#main"), ResourceId::parse("minecraft:entity/wither/wither")) else { continue };
                if !atlas.contains(&texture) {
                    continue;
                }
                let mut posed = Posed::new(model);
                // `SkullModel.setupAnim`: the head takes the heading.
                posed.with("head", |p| {
                    p.rotation.y = yaw.to_radians();
                    p.rotation.x = pitch.to_radians();
                });
                // `scale(-1, -1, 1)` about its position, without a living
                // renderer's lift.
                let feet = at - DVec3::Y * 1.501;
                entity_model::draw(models, &posed, &Placement { feet, rotation: Quat::IDENTITY, scale: Vec3::ONE, region: atlas.entity_region(&texture), sky, block: 15.0, tint: [1.0; 4] });
            }
            MissileKind::ShulkerBullet => {
                let (Some(model), Ok(texture)) = (entity_model::model("minecraft:shulker_bullet#main"), ResourceId::parse("minecraft:entity/shulker/spark")) else { continue };
                if !atlas.contains(&texture) {
                    continue;
                }
                let posed = Posed::new(model);
                let rotation = Quat::from_rotation_y(((age * 0.1).sin() * 180.0).to_radians())
                    * Quat::from_rotation_x(((age * 0.1).cos() * 180.0).to_radians())
                    * Quat::from_rotation_z(((age * 0.15).sin() * 360.0).to_radians());
                let region = atlas.entity_region(&texture);
                // The core at half size, then the shell half again as big,
                // `0x26FFFFFF` (a faint white).
                for (mesh, scale, tint) in [(&mut *models, 0.5_f32, [1.0; 4]), (&mut *translucent, 0.75, [1.0, 1.0, 1.0, 38.0 / 255.0])] {
                    let feet = at + DVec3::Y * 0.15 - (rotation * Vec3::Y * 1.501 * scale).as_dvec3();
                    entity_model::draw(mesh, &posed, &Placement { feet, rotation, scale: Vec3::splat(scale), region, sky, block: 15.0, tint });
                }
            }
            MissileKind::LlamaSpit => {
                let (Some(model), Ok(texture)) = (entity_model::model("minecraft:llama_spit#main"), ResourceId::parse("minecraft:entity/llama/llama_spit")) else { continue };
                if !atlas.contains(&texture) {
                    continue;
                }
                let posed = Posed::new(model);
                let rotation = Quat::from_rotation_y((yaw - 90.0).to_radians()) * Quat::from_rotation_z(pitch.to_radians());
                let block = light.get_block(cell) as f32;
                let feet = at + DVec3::Y * 0.15 - (rotation * Vec3::Y * 1.501).as_dvec3();
                entity_model::draw(models, &posed, &Placement { feet, rotation, scale: Vec3::ONE, region: atlas.entity_region(&texture), sky, block, tint: [1.0; 4] });
            }
        }
    }
}

/// `EvokerFangsRenderer` with `EvokerFangsModel.setupAnim`: nothing until
/// they bite, then the jaws rise out of the ground snapping shut, and shrink
/// away over their last tenth.
pub fn append_fangs<'a>(models: &mut ChunkMesh, fangs: impl IntoIterator<Item = &'a FangsEntity>, atlas: &Atlas, light: &SkyLight, partial: f32) {
    let (Some(model), Ok(texture)) = (entity_model::model("minecraft:evoker_fangs#main"), ResourceId::parse("minecraft:entity/illager/evoker_fangs")) else { return };
    if !atlas.contains(&texture) {
        return;
    }
    let region = atlas.entity_region(&texture);
    for f in fangs {
        let bite = f.bite_progress(partial);
        if bite == 0.0 {
            continue;
        }
        let mut posed = Posed::new(model);
        let shut = 1.0 - (bite * 2.0).min(1.0).powi(3);
        posed.with("upper_jaw", |p| p.rotation.z = std::f32::consts::PI - shut * 0.35 * std::f32::consts::PI);
        posed.with("lower_jaw", |p| p.rotation.z = std::f32::consts::PI + shut * 0.35 * std::f32::consts::PI);
        posed.with("base", |p| p.offset.y -= (bite + (bite * 2.7).sin()) * 7.2);
        let shrink = if bite > 0.9 { (1.0 - bite) / 0.1 } else { 1.0 };
        posed.with("root", |p| {
            p.offset.y = 24.0 - 20.0 * shrink;
            p.scale = Vec3::splat(shrink);
        });
        let cell = (f.position.x.floor() as i32, f.position.y.floor() as i32, f.position.z.floor() as i32);
        let (sky, block) = (light.get(cell) as f32, light.get_block(cell) as f32);
        let rotation = Quat::from_rotation_y((90.0 - f.yaw).to_radians());
        entity_model::draw(models, &posed, &Placement { feet: f.position, rotation, scale: Vec3::ONE, region, sky, block, tint: [1.0; 4] });
    }
}
