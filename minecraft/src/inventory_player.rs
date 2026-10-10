//! The player in the inventory screens, turned toward the mouse as
//! `InventoryScreen.extractEntityInInventoryFollowsMouse` turns it, and
//! drawn into a texture of its own as `PictureInPictureRenderer` and
//! `GuiEntityRenderer` draw it: the wide player model (`PlayerModel`) and
//! its outer layer, posed as `HumanoidModel.setupAnim` poses it and lit as
//! `Lighting.Entry.ENTITY_IN_UI` lights it, with its armour
//! (`HumanoidArmorLayer`) and where its hands hold items
//! (`ItemInHandLayer`).
use glam::{EulerRot, Mat4, Quat, Vec3};
use minecraft_terrain::mesh::{Atlas, Vertex};
use minecraft_terrain::pack::ResourceId;
use minecraftoss_player::inventory::ItemStack;

use crate::render::GuiModel;

/// What the model shows of the player.
pub struct Pose {
    pub crouching: bool,
    /// Whether the right (main) and left hands hold something
    /// (`HumanoidModel.ArmPose.ITEM`).
    pub holding: [bool; 2],
    /// `ageInTicks`, for the arms' idle sway.
    pub age: f32,
    /// `walkAnimationPos` and `walkAnimationSpeed`.
    pub walk: (f32, f32),
    /// The worn armour's layers.
    pub armor: Vec<ArmorLayer>,
}

/// One layer of a worn armour piece: its slot (0 head, 1 chest, 2 legs,
/// 3 feet), its equipment texture in the atlas and its colour.
pub struct ArmorLayer {
    pub slot: usize,
    pub region: [f32; 4],
    pub tint: [f32; 3],
}

/// `EquipmentLayerRenderer`'s layers for the armour worn in the head,
/// chest, legs and feet slots: each piece's equipment asset's `humanoid`
/// (or `humanoid_leggings`) texture, leather dyed (its `dyed_color`, else
/// its undyed colour) under its plain overlay.
pub fn armor_layers(atlas: &Atlas, worn: [Option<&ItemStack>; 4]) -> Vec<ArmorLayer> {
    let mut layers = Vec::new();
    for (slot, stack) in worn.into_iter().enumerate() {
        let Some(stack) = stack else { continue };
        let Some((asset, fits)) = minecraft_terrain::armor_render::armor_asset(&stack.id) else {
            continue;
        };
        if fits != slot {
            continue;
        }
        let folder = if slot == 2 {
            "humanoid_leggings"
        } else {
            "humanoid"
        };
        let dyed = crate::creative::dyed_color(stack).unwrap_or(0xA06540);
        let rgb = |c: u32| [(c >> 16) & 255, (c >> 8) & 255, c & 255].map(|v| v as f32 / 255.0);
        let textures: &[(&str, [f32; 3])] = if asset == "leather" {
            &[("leather", rgb(dyed)), ("leather_overlay", [1.0; 3])]
        } else {
            &[(asset, [1.0; 3])]
        };
        for &(texture, tint) in textures {
            let Ok(id) =
                ResourceId::parse(&format!("minecraft:entity/equipment/{folder}/{texture}"))
            else {
                continue;
            };
            if atlas.contains(&id) {
                layers.push(ArmorLayer {
                    slot,
                    region: atlas.entity_region(&id),
                    tint,
                });
            }
        }
    }
    layers
}

/// A model part's pose: `PartPose`, then its `xRot`, `yRot` and `zRot`.
#[derive(Clone, Copy, Default)]
struct Part {
    offset: Vec3,
    rotation: Vec3,
}

impl Part {
    fn at(x: f32, y: f32, z: f32) -> Self {
        Self {
            offset: Vec3::new(x, y, z),
            rotation: Vec3::ZERO,
        }
    }

    /// `ModelPart.translateAndRotate`.
    fn matrix(self) -> Mat4 {
        Mat4::from_translation(self.offset / 16.0)
            * Mat4::from_quat(Quat::from_euler(
                EulerRot::ZYX,
                self.rotation.z,
                self.rotation.y,
                self.rotation.x,
            ))
    }
}

/// `INVENTORY_DIFFUSE_LIGHT_0` and `_1`, from a normal in the texture's
/// space.
pub fn shade(normal: Vec3) -> f32 {
    let light0 = Vec3::new(0.2, -1.0, 1.0).normalize();
    let light1 = Vec3::new(-0.2, -1.0, 0.0).normalize();
    let light = light0.dot(normal).max(0.0) + light1.dot(normal).max(0.0);
    (light * 0.6 + 0.4).min(1.0)
}

/// A sheet in the atlas: its region, its size in pixels and a colour.
#[derive(Clone, Copy)]
struct Sheet {
    region: [f32; 4],
    size: [f32; 2],
    tint: [f32; 3],
}

/// `ModelPart.Cube` with its box layout on `sheet`, mirrored across x when
/// asked: its corners through `pose` into the texture's pixels, shaded by
/// where each face points there.
#[allow(clippy::too_many_arguments)]
fn cube(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    pose: &Mat4,
    sheet: Sheet,
    [u, v]: [f32; 2],
    origin: [f32; 3],
    [dx, dy, dz]: [f32; 3],
    grow: f32,
    mirror: bool,
) {
    let mut min = Vec3::from(origin) - grow;
    let mut max = Vec3::from(origin) + Vec3::new(dx, dy, dz) + grow;
    if mirror {
        std::mem::swap(&mut min.x, &mut max.x);
    }
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
    let region = sheet.region;
    let to_atlas = |px: f32, py: f32| {
        [
            region[0] + (region[2] - region[0]) * px / sheet.size[0],
            region[1] + (region[3] - region[1]) * py / sheet.size[1],
        ]
    };
    let middle = pose.transform_point3((min + max) / 32.0);
    for (corners, [u1, t1, u2, t2]) in faces {
        let placed = corners.map(|c| pose.transform_point3(c));
        let face_middle = placed.iter().copied().sum::<Vec3>() / 4.0;
        let light = shade((face_middle - middle).normalize_or_zero());
        let [r, g, b] = sheet.tint.map(|c| c * light);
        let first = vertices.len() as u32;
        for (position, (s, t)) in placed.iter().zip([(u2, t1), (u1, t1), (u1, t2), (u2, t2)]) {
            vertices.push(Vertex {
                position: position.to_array(),
                uv: to_atlas(s, t),
                color: [r, g, b, 1.0],
                sky_light: 15.0,
                block_light: 15.0,
            });
        }
        indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }
}

/// A model texture's pixels (y down, z toward the viewer) to clip space,
/// `setOrtho(0, w, h, 0, -1000, 1000)` with reverse-Z depth.
pub fn clip_from_texture(width: f32, tall: f32) -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::new(2.0 / width, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, -2.0 / tall, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, 1.0 / 2000.0, 0.0),
        glam::Vec4::new(-1.0, 1.0, 0.5, 1.0),
    )
}

/// Steve's skin in the atlas.
pub fn skin(atlas: &Atlas) -> Option<[f32; 4]> {
    let skin = ResourceId::parse("minecraft:entity/player/wide/steve").ok()?;
    atlas.contains(&skin).then(|| atlas.entity_region(&skin))
}

/// The player in the GUI box `x0, y0` to `x1, y1` (GUI pixels), `size`
/// pixels to a block, looking toward the mouse; drawn into a texture of
/// the box's size in window pixels. Also where its right and left hands
/// hold items: `ItemInHandLayer`'s pose for each, from item space to the
/// texture's.
pub fn model(
    region: [f32; 4],
    [x0, y0, x1, y1]: [f32; 4],
    size: f32,
    offset_y: f32,
    mouse: (f32, f32),
    gui_scale: f32,
    pose: &Pose,
) -> (GuiModel, [Mat4; 2]) {
    let (center_x, center_y) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let x_angle = ((center_x - mouse.0) / 40.0).atan();
    let y_angle = ((center_y - mouse.1) / 40.0).atan();
    let body_rot = 180.0 + x_angle * 20.0;
    let head_yaw = (x_angle * 20.0).to_radians();
    let head_pitch = (-y_angle * 20.0).to_radians();
    let rotation = Quat::from_rotation_z(std::f32::consts::PI)
        * Quat::from_rotation_x((y_angle * 20.0).to_radians());
    let height = if pose.crouching { 1.5 } else { 1.8 };
    let (width, tall) = (
        ((x1 - x0) * gui_scale).round().max(1.0),
        ((y1 - y0) * gui_scale).round().max(1.0),
    );
    let scale = gui_scale * size;
    // `PictureInPictureRenderer`, `GuiEntityRenderer`, the crouch's render
    // offset, then `LivingEntityRenderer.submit` and `AvatarRenderer.scale`.
    let entity = Mat4::from_translation(Vec3::new(width / 2.0, tall / 2.0, 0.0))
        * Mat4::from_scale(Vec3::new(scale, scale, -scale))
        * Mat4::from_translation(Vec3::new(0.0, height / 2.0 + offset_y, 0.0))
        * Mat4::from_quat(rotation)
        * Mat4::from_translation(Vec3::new(
            0.0,
            if pose.crouching { -2.0 / 16.0 } else { 0.0 },
            0.0,
        ))
        * Mat4::from_rotation_y((180.0 - body_rot).to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(Vec3::splat(0.9375))
        * Mat4::from_translation(Vec3::new(0.0, -1.501, 0.0));

    // `HumanoidModel.setupAnim`.
    let mut head = Part::at(0.0, 0.0, 0.0);
    let mut body = Part::at(0.0, 0.0, 0.0);
    let mut right_arm = Part::at(-5.0, 2.0, 0.0);
    let mut left_arm = Part::at(5.0, 2.0, 0.0);
    let mut right_leg = Part::at(-1.9, 12.0, 0.0);
    let mut left_leg = Part::at(1.9, 12.0, 0.0);
    head.rotation = Vec3::new(head_pitch, head_yaw, 0.0);
    let (walk_pos, walk_speed) = pose.walk;
    let pi = std::f32::consts::PI;
    right_arm.rotation.x = (walk_pos * 0.6662 + pi).cos() * 2.0 * walk_speed * 0.5;
    left_arm.rotation.x = (walk_pos * 0.6662).cos() * 2.0 * walk_speed * 0.5;
    right_leg.rotation = Vec3::new((walk_pos * 0.6662).cos() * 1.4 * walk_speed, 0.005, 0.005);
    left_leg.rotation = Vec3::new(
        (walk_pos * 0.6662 + pi).cos() * 1.4 * walk_speed,
        -0.005,
        -0.005,
    );
    for (arm, holding) in [
        (&mut right_arm, pose.holding[0]),
        (&mut left_arm, pose.holding[1]),
    ] {
        if holding {
            arm.rotation.x = arm.rotation.x * 0.5 - pi / 10.0;
        }
        arm.rotation.y = 0.0;
    }
    if pose.crouching {
        body.rotation.x = 0.5;
        right_arm.rotation.x += 0.4;
        left_arm.rotation.x += 0.4;
        right_leg.offset.z += 4.0;
        left_leg.offset.z += 4.0;
        head.offset.y += 4.2;
        body.offset.y += 3.2;
        left_arm.offset.y += 3.2;
        right_arm.offset.y += 3.2;
    }
    // `AnimationUtils.bobModelPart`.
    for (arm, side) in [(&mut right_arm, 1.0), (&mut left_arm, -1.0)] {
        arm.rotation.z += side * ((pose.age * 0.09).cos() * 0.05 + 0.05);
        arm.rotation.x += side * ((pose.age * 0.067).sin() * 0.05);
    }

    // `PlayerModel.createMesh` for the wide arms: each part and its layer.
    type Box = ([f32; 2], [f32; 3], [f32; 3]);
    let parts: [(Part, Box, Box, f32); 6] = [
        (
            head,
            ([0.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]),
            ([32.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]),
            0.5,
        ),
        (
            body,
            ([16.0, 16.0], [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0]),
            ([16.0, 32.0], [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0]),
            0.25,
        ),
        (
            right_arm,
            ([40.0, 16.0], [-3.0, -2.0, -2.0], [4.0, 12.0, 4.0]),
            ([40.0, 32.0], [-3.0, -2.0, -2.0], [4.0, 12.0, 4.0]),
            0.25,
        ),
        (
            left_arm,
            ([32.0, 48.0], [-1.0, -2.0, -2.0], [4.0, 12.0, 4.0]),
            ([48.0, 48.0], [-1.0, -2.0, -2.0], [4.0, 12.0, 4.0]),
            0.25,
        ),
        (
            right_leg,
            ([0.0, 16.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0]),
            ([0.0, 32.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0]),
            0.25,
        ),
        (
            left_leg,
            ([16.0, 48.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0]),
            ([0.0, 48.0], [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0]),
            0.25,
        ),
    ];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let skin = Sheet {
        region,
        size: [64.0, 64.0],
        tint: [1.0; 3],
    };
    for (part, inner, outer, grow) in parts {
        let pose = entity * part.matrix();
        for ((uv, origin, extent), inflate) in [(inner, 0.0), (outer, grow)] {
            cube(
                &mut vertices,
                &mut indices,
                &pose,
                skin,
                uv,
                origin,
                extent,
                inflate,
                false,
            );
        }
    }
    // `HumanoidArmorLayer` on `HumanoidModel.createArmorMeshSet`: the outer
    // set (1.0) for the head, chest and feet, the inner (0.5) for the legs,
    // armour legs a tenth thinner, posed as the player's own parts.
    for layer in &pose.armor {
        let sheet = Sheet {
            region: layer.region,
            size: [64.0, 32.0],
            tint: layer.tint,
        };
        let grow = if layer.slot == 2 { 0.5 } else { 1.0 };
        let mut add =
            |part: Part, uv: [f32; 2], origin: [f32; 3], extent: [f32; 3], g: f32, mirror: bool| {
                let pose = entity * part.matrix();
                cube(
                    &mut vertices,
                    &mut indices,
                    &pose,
                    sheet,
                    uv,
                    origin,
                    extent,
                    g,
                    mirror,
                );
            };
        let limb = [4.0, 12.0, 4.0];
        match layer.slot {
            0 => {
                add(head, [0.0, 0.0], [-4.0, -8.0, -4.0], [8.0; 3], grow, false);
                add(
                    head,
                    [32.0, 0.0],
                    [-4.0, -8.0, -4.0],
                    [8.0; 3],
                    grow + 0.5,
                    false,
                );
            }
            1 => {
                add(
                    body,
                    [16.0, 16.0],
                    [-4.0, 0.0, -2.0],
                    [8.0, 12.0, 4.0],
                    grow,
                    false,
                );
                add(
                    right_arm,
                    [40.0, 16.0],
                    [-3.0, -2.0, -2.0],
                    limb,
                    grow,
                    false,
                );
                add(left_arm, [40.0, 16.0], [-1.0, -2.0, -2.0], limb, grow, true);
            }
            _ => {
                if layer.slot == 2 {
                    add(
                        body,
                        [16.0, 16.0],
                        [-4.0, 0.0, -2.0],
                        [8.0, 12.0, 4.0],
                        grow,
                        false,
                    );
                }
                add(
                    right_leg,
                    [0.0, 16.0],
                    [-2.0, 0.0, -2.0],
                    limb,
                    grow - 0.1,
                    false,
                );
                add(
                    left_leg,
                    [0.0, 16.0],
                    [-2.0, 0.0, -2.0],
                    limb,
                    grow - 0.1,
                    true,
                );
            }
        }
    }
    // `ItemInHandLayer.submitArmWithItem`: the arm, then a quarter turn
    // down and a half turn round, a pixel out, two down and ten forward.
    let hands = [(right_arm, 1.0), (left_arm, -1.0)].map(|(arm, side)| {
        entity
            * arm.matrix()
            * Mat4::from_rotation_x((-90.0f32).to_radians())
            * Mat4::from_rotation_y(180.0f32.to_radians())
            * Mat4::from_translation(Vec3::new(side / 16.0, 2.0 / 16.0, -10.0 / 16.0))
    });
    (
        GuiModel {
            vertices,
            indices,
            clip_from_model: clip_from_texture(width, tall),
            size: (width as u32, tall as u32),
        },
        hands,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_the_viewer_and_turns_toward_the_mouse() {
        let pose = Pose {
            crouching: false,
            holding: [false; 2],
            age: 0.0,
            walk: (0.0, 0.0),
            armor: Vec::new(),
        };
        let model = |mouse| {
            model(
                [0.0, 0.0, 1.0, 1.0],
                [26.0, 8.0, 75.0, 78.0],
                30.0,
                0.0625,
                mouse,
                2.0,
                &pose,
            )
            .0
        };
        let ahead = model((50.5, 43.0));
        assert_eq!(ahead.size, (98, 140));
        // The head's front face (the first cube's NORTH face) is nearest.
        let z = |m: &GuiModel, i: usize| m.vertices[i].position[2];
        assert!(z(&ahead, 12) > z(&ahead, 20), "front nearer than back");
        // Feet below the head: y grows down the texture.
        let head_top = ahead.vertices[4].position[1];
        let foot = ahead.vertices.last().unwrap().position[1];
        assert!(head_top < foot);
        // To the right, the head turns right: its front moves right.
        let right = model((200.0, 43.0));
        let front_x = |m: &GuiModel| (12..16).map(|i| m.vertices[i].position[0]).sum::<f32>();
        assert!(front_x(&right) > front_x(&ahead));
    }
}
