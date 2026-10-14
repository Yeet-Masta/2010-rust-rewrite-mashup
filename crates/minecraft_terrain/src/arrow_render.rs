//! Arrows and thrown tridents (26.3 `ArrowRenderer` with `ArrowModel`,
//! `TippedArrowRenderer`, `SpectralArrowRenderer`, and
//! `ThrownTridentRenderer` with `TridentModel`): each turned to its yaw
//! less 90 degrees and its pitch, an arrow quivering about its length for
//! the ticks it shakes after it lands (`ArrowModel.setupAnim`), lit at its
//! cell.
use crate::lighting::SkyLight;
use crate::mesh::{Atlas, ChunkMesh};
use crate::pack::ResourceId;
use crate::special_icon::{arrow_model, emit_part, trident_model};
use glam::{DVec3, Mat4, Vec3};
use minecraftoss_entities::projectile::ArrowKind;
use minecraftoss_entities::world::ArrowEntity;

/// The sheet an arrow is drawn with: a tipped one's when it carries a
/// potion (`Arrow.getColor` set).
fn texture(entity: &ArrowEntity) -> &'static str {
    match entity.arrow.kind {
        ArrowKind::Spectral => "minecraft:entity/projectiles/spectral_arrow",
        ArrowKind::Trident => "minecraft:entity/trident/trident",
        ArrowKind::Arrow => {
            let tipped = entity.arrow.item.as_ref().is_some_and(|item| item.components.as_ref().is_some_and(|c| c.get("minecraft:potion_contents").is_some()));
            if tipped { "minecraft:entity/projectiles/tipped_arrow" } else { "minecraft:entity/projectiles/arrow" }
        }
    }
}

/// The sheets arrows and tridents are drawn with.
pub const SHEETS: [&str; 4] = [
    "minecraft:entity/projectiles/arrow",
    "minecraft:entity/projectiles/tipped_arrow",
    "minecraft:entity/projectiles/spectral_arrow",
    "minecraft:entity/trident/trident",
];

/// Appends each arrow at its drawn position, `partial` through the tick.
pub fn append_arrows<'a>(mesh: &mut ChunkMesh, arrows: impl IntoIterator<Item = (&'a ArrowEntity, DVec3)>, atlas: &Atlas, light: &SkyLight, partial: f32) {
    for (entity, at) in arrows {
        let Ok(sheet) = ResourceId::parse(texture(entity)) else { continue };
        if !atlas.contains(&sheet) {
            continue;
        }
        let region = atlas.entity_region(&sheet);
        let arrow = &entity.arrow;
        let cell = (at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32);
        let lit = [light.get(cell) as f32, light.get_block(cell) as f32];
        let shade = |normal: Vec3| crate::cow_render::entity_shade(normal);
        let mut pose = Mat4::from_translation(at.as_vec3()) * Mat4::from_rotation_y((arrow.yaw - 90.0).to_radians());
        if arrow.kind == ArrowKind::Trident {
            // `ThrownTridentRenderer`: the model stands along its pitch.
            pose *= Mat4::from_rotation_z((arrow.pitch + 90.0).to_radians());
            for part in trident_model() {
                emit_part(mesh, &part, pose, region, [32.0, 32.0], [1.0; 3], &shade, lit);
            }
            continue;
        }
        pose *= Mat4::from_rotation_z(arrow.pitch.to_radians());
        let shake = arrow.shake_time as f32 - partial;
        if shake > 0.0 {
            let quiver = -(shake * 3.0).sin() * shake;
            pose *= Mat4::from_rotation_z(quiver.to_radians());
        }
        for (part, size) in arrow_model() {
            emit_part(mesh, &part, pose, region, size, [1.0; 3], &shade, lit);
        }
    }
}
