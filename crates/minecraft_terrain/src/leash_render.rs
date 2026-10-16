//! Leads and their fence knots (26.3 `LeashFeatureRenderer`,
//! `EntityRenderer.extractRenderState`'s leash state, `LeashKnotRenderer`).
//!
//! A lead is two crossed ribbons 0.05 wide in 24 steps from the mob's
//! attachment to its holder's rope hold, sagging (`slack`) toward the
//! lower end, its steps alternately brown and darker, lit along its length
//! from the light at each end's eyes. A knot is its little cube on the
//! fence post.
use crate::{
    entity_model::{self, Placement, Posed},
    lighting::SkyLight,
    mesh::{Atlas, ChunkMesh, Vertex},
    pack::ResourceId,
};
use glam::{DVec3, Quat, Vec3};

/// The textures these draw, for the atlas: the knot's, and a plain white
/// tile under the untextured ribbons.
pub const TEXTURES: [&str; 2] = ["minecraft:entity/lead_knot/lead_knot", "minecraft:misc/white"];

/// One lead: from the mob's end (`start`) to the holder's (`end`), with
/// the cells whose light each end takes.
pub struct Rope {
    pub start: DVec3,
    pub end: DVec3,
    pub start_light_at: DVec3,
    pub end_light_at: DVec3,
}

/// `getLeashOffset`: the mob's eye height up and 0.4 of its width ahead,
/// turned with its body (`yRot(-bodyRot)`).
pub fn leash_offset(eye_height: f32, width: f32, body_rot_degrees: f32) -> DVec3 {
    rot_y(DVec3::new(0.0, f64::from(eye_height), f64::from(width * 0.4)), -body_rot_degrees.to_radians())
}

/// `Player.getRopeHoldPosition` on foot: by the main (right) hand, a metre
/// below the top of the box, a little ahead (behind, crouching).
pub fn player_rope_hold(feet: DVec3, body_rot_degrees: f32, crouching: bool) -> DVec3 {
    let height = if crouching { 1.5 } else { 1.8 };
    let z = if crouching { -0.2 } else { 0.07 };
    feet + rot_y(DVec3::new(-0.22, height - 1.0, z), -body_rot_degrees.to_radians())
}

/// `Vec3.yRot`.
fn rot_y(v: DVec3, angle: f32) -> DVec3 {
    let (sin, cos) = (f64::from(angle.sin()), f64::from(angle.cos()));
    DVec3::new(v.x * cos + v.z * sin, v.y, v.z * cos - v.x * sin)
}

fn cell(p: DVec3) -> (i32, i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

/// Draws the leads.
pub fn append_leashes(models: &mut ChunkMesh, ropes: &[Rope], atlas: &Atlas, light: &SkyLight) {
    let Ok(white) = ResourceId::parse("minecraft:misc/white") else { return };
    if !atlas.contains(&white) {
        return;
    }
    let [u0, v0, u1, v1] = atlas.entity_region(&white);
    let uv = [(u0 + u1) / 2.0, (v0 + v1) / 2.0];
    for rope in ropes {
        let (start, end) = (cell(rope.start_light_at), cell(rope.end_light_at));
        let (sb, eb) = (light.get_block(start) as f32, light.get_block(end) as f32);
        let (ss, es) = (light.get(start) as f32, light.get(end) as f32);
        let d = (rope.end - rope.start).as_vec3();
        let horizontal = (d.x * d.x + d.z * d.z).sqrt();
        if horizontal == 0.0 && d.y == 0.0 {
            continue;
        }
        let factor = if horizontal > 0.0 { 0.05 / 2.0 / horizontal } else { 0.0 };
        let (dx_off, dz_off) = (d.z * factor, d.x * factor);
        let origin = rope.start.as_vec3();
        // Each step's pair of points on one ribbon (`addVertexPair`).
        let pair = |k: usize, fudge: f32, backwards: bool| -> [Vertex; 2] {
            let progress = k as f32 / 24.0;
            let block = (sb + (eb - sb) * progress).floor();
            let sky = (ss + (es - ss) * progress).floor();
            let shade = if k % 2 == usize::from(backwards) { 0.7 } else { 1.0 };
            let color = [0.5 * shade, 0.4 * shade, 0.3 * shade, 1.0];
            let x = d.x * progress;
            let y = if d.y > 0.0 { d.y * progress * progress } else { d.y - d.y * (1.0 - progress) * (1.0 - progress) };
            let z = d.z * progress;
            let at = |p: Vec3| Vertex { position: (origin + p).to_array(), uv, color, sky_light: sky, block_light: block };
            [at(Vec3::new(x - dx_off, y + fudge, z + dz_off)), at(Vec3::new(x + dx_off, y + 0.05 - fudge, z - dz_off))]
        };
        for (fudge, backwards) in [(0.05, false), (0.0, true)] {
            for k in 0..24 {
                let (a, b) = (pair(k, fudge, backwards), pair(k + 1, fudge, backwards));
                // Seen from both sides.
                for quad in [[a[0], a[1], b[1], b[0]], [b[0], b[1], a[1], a[0]]] {
                    let first = models.vertices.len() as u32;
                    models.vertices.extend_from_slice(&quad);
                    models.indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
                    models.faces += 1;
                }
            }
        }
    }
}

/// Draws the fence knots: the knot model at the post, half a block
/// above its middle's 0.375, flipped as entities are.
pub fn append_knots(models: &mut ChunkMesh, knots: &[(i32, i32, i32)], atlas: &Atlas, light: &SkyLight) {
    let (Some(model), Ok(texture)) = (entity_model::model("minecraft:leash_knot#main"), ResourceId::parse("minecraft:entity/lead_knot/lead_knot")) else { return };
    if !atlas.contains(&texture) {
        return;
    }
    let region = atlas.entity_region(&texture);
    for &(x, y, z) in knots {
        let posed = Posed::new(model);
        let (sky, block) = (light.get((x, y, z)) as f32, light.get_block((x, y, z)) as f32);
        // `draw` lifts by 1.501 as for a living mob; the knot's renderer
        // only flips.
        let feet = DVec3::new(f64::from(x) + 0.5, f64::from(y) + 0.375 - 1.501, f64::from(z) + 0.5);
        entity_model::draw(models, &posed, &Placement { feet, rotation: Quat::IDENTITY, scale: Vec3::ONE, region, sky, block, tint: [1.0; 4] });
    }
}
