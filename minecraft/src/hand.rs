//! The first-person hand: the held item placed as vanilla's
//! `ItemInHandRenderer.applyItemArmTransform` places it with the item's
//! `firstperson_righthand` display, or with nothing held the player's own
//! right arm (`renderPlayerArm`), swinging as vanilla swings them. Vertices
//! are in view space: x right, y up, z back.
use glam::{Mat3, Mat4, Vec3};
use minecraft_terrain::mesh::{Atlas, SectionVertex};
use minecraft_terrain::pack::ResourceId;

/// Ticks of a swing (`LivingEntity.getCurrentSwingDuration`).
pub const SWING_TICKS: f32 = 6.0;

/// The item's view-space pose (`applyItemArmTransform` then its display).
pub fn item_pose(display: Mat4, swing: f32, equip: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(0.56, -0.52 - 0.6 * equip, -0.72))
        * item_swing_transform(swing)
        * display
}

fn item_swing_transform(swing: f32) -> Mat4 {
    let root = swing.sqrt();
    let x = -0.4 * (root * std::f32::consts::PI).sin();
    let y = 0.2 * (root * std::f32::consts::TAU).sin();
    let z = -0.2 * (swing * std::f32::consts::PI).sin();
    let y_rotation = (swing * swing * std::f32::consts::PI).sin();
    let xz_rotation = (root * std::f32::consts::PI).sin();
    Mat4::from_translation(Vec3::new(x, y, z))
        * Mat4::from_rotation_y((45.0 - 20.0 * y_rotation).to_radians())
        * Mat4::from_rotation_z((-20.0 * xz_rotation).to_radians())
        * Mat4::from_rotation_x((-80.0 * xz_rotation).to_radians())
        * Mat4::from_rotation_y((-45.0f32).to_radians())
}

/// The bare right arm: the wide player model's arm cuboid with Steve's skin,
/// lit by the light at the eye (sky and block levels).
/// `world_from_view` turns view-space normals to the world, where the
/// entity lights are.
pub fn arm_mesh(
    atlas: &Atlas,
    swing: f32,
    equip: f32,
    light: [u8; 2],
    world_from_view: Mat3,
) -> (Vec<SectionVertex>, Vec<u32>) {
    let Ok(skin) = ResourceId::parse("minecraft:entity/player/wide/steve") else {
        return (Vec::new(), Vec::new());
    };
    if !atlas.contains(&skin) {
        return (Vec::new(), Vec::new());
    }
    let region = atlas.entity_region(&skin);
    let root = swing.sqrt();
    let pi = std::f32::consts::PI;
    let h = -0.3 * (root * pi).sin();
    let i = 0.4 * (root * pi * 2.0).sin();
    let j = -0.4 * (swing * pi).sin();
    let k = (swing * swing * pi).sin();
    let l = (root * pi).sin();
    let pose = Mat4::from_translation(Vec3::new(h + 0.640_000_05, i - 0.6 - equip * 0.6, j - 0.719_999_97))
        * Mat4::from_rotation_y(45f32.to_radians())
        * Mat4::from_rotation_y((l * 70.0).to_radians())
        * Mat4::from_rotation_z((k * -20.0).to_radians())
        * Mat4::from_translation(Vec3::new(-1.0, 3.6, 3.5))
        * Mat4::from_rotation_z(120f32.to_radians())
        * Mat4::from_rotation_x(200f32.to_radians())
        * Mat4::from_rotation_y((-135f32).to_radians())
        * Mat4::from_translation(Vec3::new(5.6, 0.0, 0.0))
        // `ModelPart.translateAndRotate`: the right arm's pivot.
        * Mat4::from_translation(Vec3::new(-5.0, 2.0, 0.0) / 16.0);
    // The arm and its sleeve layer, a hair larger.
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (uv, grow) in [([40.0, 16.0], 0.0), ([40.0, 32.0], 0.25)] {
        cuboid(
            &mut vertices,
            &mut indices,
            &pose,
            region,
            uv,
            [-3.0, -2.0, -2.0],
            [4.0, 12.0, 4.0],
            grow,
            light,
            world_from_view,
        );
    }
    (vertices, indices)
}

/// `ModelPart.Cube` with its box texture layout on a 64-pixel skin.
#[allow(clippy::too_many_arguments)]
fn cuboid(
    vertices: &mut Vec<SectionVertex>,
    indices: &mut Vec<u32>,
    pose: &Mat4,
    region: [f32; 4],
    [u, v]: [f32; 2],
    origin: [f32; 3],
    [dx, dy, dz]: [f32; 3],
    grow: f32,
    light: [u8; 2],
    world_from_view: Mat3,
) {
    let min = Vec3::from(origin) - grow;
    let max = Vec3::from(origin) + Vec3::new(dx, dy, dz) + grow;
    let corner = |x: bool, y: bool, z: bool| {
        Vec3::new(
            if x { max.x } else { min.x },
            if y { max.y } else { min.y },
            if z { max.z } else { min.z },
        ) / 16.0
    };
    let (v1, v2, v3, v4) = (
        corner(false, false, false),
        corner(true, false, false),
        corner(true, true, false),
        corner(false, true, false),
    );
    let (v5, v6, v7, v8) = (
        corner(false, false, true),
        corner(true, false, true),
        corner(true, true, true),
        corner(false, true, true),
    );
    let (f, g, h, i, j, k) = (
        u,
        u + dz,
        u + dz + dx,
        u + dz + dx + dx,
        u + dz + dx + dz,
        u + dz + dx + dz + dx,
    );
    let (l, m, n) = (v, v + dz, v + dz + dy);
    let faces = [
        ([v6, v5, v1, v2], [g, l, h, m]),
        ([v3, v4, v8, v7], [h, m, i, l]),
        ([v1, v5, v8, v4], [f, m, g, n]),
        ([v2, v1, v4, v3], [g, m, h, n]),
        ([v6, v2, v3, v7], [h, m, j, n]),
        ([v5, v6, v7, v8], [j, m, k, n]),
    ];
    let to_atlas = |px: f32, py: f32| {
        [
            region[0] + (region[2] - region[0]) * px / 64.0,
            region[1] + (region[3] - region[1]) * py / 64.0,
        ]
    };
    for (corners, [u1, t1, u2, t2]) in faces {
        let placed: Vec<Vec3> = corners.iter().map(|&c| pose.transform_point3(c)).collect();
        let middle = pose.transform_point3((min + max) / 32.0);
        let face_middle = placed.iter().copied().sum::<Vec3>() / 4.0;
        let normal = world_from_view * (face_middle - middle).normalize_or_zero();
        let shade = minecraft_terrain::cow_render::entity_shade(normal);
        let colour = (shade * 255.0) as u8;
        let first = vertices.len() as u32;
        for (position, (s, t)) in placed.iter().zip([(u2, t1), (u1, t1), (u1, t2), (u2, t2)]) {
            vertices.push(SectionVertex {
                position: position.to_array(),
                uv: to_atlas(s, t),
                color: [colour, colour, colour, 255],
                light: [light[0] * 16, light[1] * 16],
                pad: [0; 2],
            });
        }
        indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }
}
