//! The first-person hands: held items placed as vanilla's
//! `ItemInHandRenderer.applyItemArmTransform` places them with the item's
//! `firstperson_righthand` or `_lefthand` display, or with nothing in the
//! main hand the player's own right arm (`renderPlayerArm`), swinging as
//! vanilla swings them. Vertices
//! are in view space: x right, y up, z back.
use glam::{Mat3, Mat4, Vec3};
use minecraft_terrain::mesh::{Atlas, SectionVertex};
use minecraft_terrain::pack::ResourceId;

/// Ticks of a swing (`LivingEntity.getCurrentSwingDuration`).
const SWING_TICKS: i32 = 6;

/// The arm's swing as `LivingEntity` keeps it: `swing`, `updateSwingTime`
/// and `getAttackAnim`.
#[derive(Default)]
pub struct Swing {
    time: i32,
    swinging: bool,
    anim: f32,
    previous: f32,
}

impl Swing {
    /// `swing()`: starts over, unless the last one is under half done, so
    /// holding the button keeps the arm going in an even rhythm.
    pub fn start(&mut self) {
        if !self.swinging || self.time >= SWING_TICKS / 2 || self.time < 0 {
            self.time = -1;
            self.swinging = true;
        }
    }

    /// `updateSwingTime`, once a tick after the tick's input.
    pub fn tick(&mut self) {
        self.previous = self.anim;
        if self.swinging {
            self.time += 1;
            if self.time >= SWING_TICKS {
                self.time = 0;
                self.swinging = false;
            }
        } else {
            self.time = 0;
        }
        self.anim = self.time as f32 / SWING_TICKS as f32;
    }

    /// `getAttackAnim`: how far through the swing the arm is, 0 to 1, a
    /// restart carrying on forward to finish the stroke.
    pub fn progress(&self, partial: f32) -> f32 {
        let mut step = self.anim - self.previous;
        if step < 0.0 {
            step += 1.0;
        }
        self.previous + step * partial
    }
}

/// A held item's view-space pose before its display transform: the swing
/// (`renderArmWithItem`'s offsets), `applyItemArmTransform` and
/// `applyItemArmAttackTransform`, mirrored for the left arm.
pub fn item_pose(swing: f32, equip: f32, left: bool) -> Mat4 {
    let invert = if left { -1.0 } else { 1.0 };
    let pi = std::f32::consts::PI;
    let root = swing.sqrt();
    let x = -0.4 * (root * pi).sin();
    let y = 0.2 * (root * std::f32::consts::TAU).sin();
    let z = -0.2 * (swing * pi).sin();
    let y_sin = (swing * swing * pi).sin();
    let xz_sin = (root * pi).sin();
    Mat4::from_translation(Vec3::new(invert * x, y, z))
        * Mat4::from_translation(Vec3::new(invert * 0.56, -0.52 - 0.6 * equip, -0.72))
        * Mat4::from_rotation_y((invert * (45.0 - 20.0 * y_sin)).to_radians())
        * Mat4::from_rotation_z((invert * xz_sin * -20.0).to_radians())
        * Mat4::from_rotation_x((xz_sin * -80.0).to_radians())
        * Mat4::from_rotation_y((invert * -45.0).to_radians())
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
