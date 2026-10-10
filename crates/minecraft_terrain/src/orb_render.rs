//! Experience orbs (pinned 26.3 `ExperienceOrbRenderer`): one 16-pixel
//! frame of the 4 by 4 `entity/experience/experience_orb` sheet, picked by
//! the orb's value (`ExperienceOrb.getIcon`), on a quad turned with the
//! camera (`camera.orientation`) 0.1 above the orb's feet, 0.3 across and
//! reaching from -0.075 to 0.225 up. Green throughout, its red and blue
//! pulse with the age in ticks, at half alpha (`entityTranslucent`); it
//! takes seven more block light than its cell has (`getBlockLightLevel`).
use crate::{
    lighting::SkyLight,
    mesh::{Atlas, ChunkMesh, Vertex},
    pack::ResourceId,
};
use glam::{DVec3, Vec3};

/// `ExperienceOrb.getIcon`: the sheet's frame for an orb's value, larger
/// orbs from 3, 7, 17, 37, 73, 149, 307, 617, 1237 and 2477 points.
pub fn icon(value: i32) -> usize {
    const THRESHOLDS: [i32; 10] = [3, 7, 17, 37, 73, 149, 307, 617, 1237, 2477];
    THRESHOLDS.iter().filter(|&&t| value >= t).count()
}

/// The vertex colour at `age` ticks (`tickCount` plus the partial tick):
/// red `(sin(age / 2) + 1) / 2`, full green, and blue a fifth of that wave
/// a third of a turn behind, each cut to whole bytes; alpha 128.
pub fn colour(age: f32) -> [f32; 4] {
    let phase = age / 2.0;
    let red = ((phase.sin() + 1.0) * 0.5 * 255.0) as i32;
    let blue = (((phase + std::f32::consts::PI * 4.0 / 3.0).sin() + 1.0) * 0.1 * 255.0) as i32;
    [red as f32 / 255.0, 1.0, blue as f32 / 255.0, 128.0 / 255.0]
}

/// Appends each orb: its rendered feet position, value and age in ticks.
pub fn append_orbs(mesh: &mut ChunkMesh, orbs: impl IntoIterator<Item = (DVec3, i32, f32)>, atlas: &Atlas, forward: Vec3, light: &SkyLight) {
    let Ok(sheet) = ResourceId::parse("minecraft:entity/experience/experience_orb") else { return };
    if !atlas.contains(&sheet) {
        return;
    }
    let [su0, sv0, su1, sv1] = atlas.region(&sheet);
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    for (at, value, age) in orbs {
        let frame = icon(value);
        let (column, row) = ((frame % 4) as f32, (frame / 4) as f32);
        let u = |x: f32| su0 + (su1 - su0) * (column + x) / 4.0;
        let v = |y: f32| sv0 + (sv1 - sv0) * (row + y) / 4.0;
        let (u0, u1, v0, v1) = (u(0.0), u(1.0), v(0.0), v(1.0));
        // `getLightProbePosition`: the eye, 0.425 up a 0.5 tall orb.
        let cell = (at.x.floor() as i32, (at.y + 0.425).floor() as i32, at.z.floor() as i32);
        let sky = light.get(cell) as f32;
        let block = (light.get_block(cell) + 7).min(15) as f32;
        let color = colour(age);
        let center = at.as_vec3() + Vec3::Y * 0.1;
        let start = mesh.vertices.len() as u32;
        for (x, y, uv) in [(-0.5, -0.25, [u0, v1]), (0.5, -0.25, [u1, v1]), (0.5, 0.75, [u1, v0]), (-0.5, 0.75, [u0, v0])] {
            let point = center + (right * x + up * y) * 0.3;
            mesh.vertices.push(Vertex { position: point.to_array(), uv, color, sky_light: sky, block_light: block });
        }
        mesh.indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        mesh.faces += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_follow_the_value_thresholds() {
        assert_eq!(icon(0), 0);
        assert_eq!(icon(2), 0);
        assert_eq!(icon(3), 1);
        assert_eq!(icon(16), 2);
        assert_eq!(icon(17), 3);
        assert_eq!(icon(2476), 9);
        assert_eq!(icon(2477), 10);
        assert_eq!(icon(32767), 10);
    }
}
