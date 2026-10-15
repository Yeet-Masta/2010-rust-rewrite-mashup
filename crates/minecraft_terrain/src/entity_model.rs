//! The 26.3 entity models as data: every layer `LayerDefinitions.createRoots`
//! registers, baked (`LayerDefinition.bakeRoot`), and every keyframe
//! animation of `client.animation.definitions`, read out of the jar by
//! `minecraft/tools/DumpModels.java`. A model is a tree of parts, each with
//! its initial pose (`PartPose`) and its cubes' polygons (vertex positions
//! in pixels, texture coordinates over the sheet, a normal); a renderer
//! poses the parts as its `setupAnim` would and draws them through the
//! pose stack (`ModelPart.translateAndRotate`: translate by the offset in
//! sixteenths, rotate Z then Y then X, scale), inside the living renderer's
//! flip (`scale(-1, -1, 1)`, `translate(0, -1.501, 0)`).
use crate::mesh::{ChunkMesh, Vertex};
use glam::{DVec3, Mat3, Mat4, Quat, Vec3};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// A part's pose: offset in pixels, rotation in radians, scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartPose {
    pub offset: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,
}

impl PartPose {
    /// `ModelPart.translateAndRotate` as a matrix.
    fn matrix(&self) -> Mat4 {
        let rotation = Quat::from_euler(glam::EulerRot::ZYX, self.rotation.z, self.rotation.y, self.rotation.x);
        Mat4::from_scale_rotation_translation(self.scale, rotation, self.offset / 16.0)
    }
}

/// One polygon: four corners (pixels) with their texture coordinates, and
/// the face's normal.
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub corners: [(Vec3, [f32; 2]); 4],
    pub normal: Vec3,
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub pose: PartPose,
    pub quads: Vec<Quad>,
    pub children: Vec<usize>,
}

/// A baked model: its parts, the root first.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub parts: Vec<Part>,
    names: HashMap<String, usize>,
}

impl Model {
    /// `ModelPart.getChild` anywhere in the tree (`createPartLookup`).
    pub fn part(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }

    fn read(value: &Value, name: &str, model: &mut Model) -> usize {
        let p = value.get("p").and_then(Value::as_array).map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect::<Vec<_>>()).unwrap_or_default();
        let at = |i: usize, default: f32| p.get(i).copied().unwrap_or(default);
        let pose = PartPose {
            offset: Vec3::new(at(0, 0.0), at(1, 0.0), at(2, 0.0)),
            rotation: Vec3::new(at(3, 0.0), at(4, 0.0), at(5, 0.0)),
            scale: Vec3::new(at(6, 1.0), at(7, 1.0), at(8, 1.0)),
        };
        let mut quads = Vec::new();
        if let Some(q) = value.get("q").and_then(Value::as_array) {
            let numbers: Vec<f32> = q.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
            for polygon in numbers.chunks_exact(23) {
                let corner = |i: usize| {
                    let c = &polygon[i * 5..i * 5 + 5];
                    (Vec3::new(c[0], c[1], c[2]), [c[3], c[4]])
                };
                quads.push(Quad { corners: [corner(0), corner(1), corner(2), corner(3)], normal: Vec3::new(polygon[20], polygon[21], polygon[22]) });
            }
        }
        let index = model.parts.len();
        model.parts.push(Part { name: name.to_owned(), pose, quads, children: Vec::new() });
        // A model's own `root` part shadows the tree's top.
        if index == 0 {
            model.names.entry(name.to_owned()).or_insert(index);
        } else {
            model.names.insert(name.to_owned(), index);
        }
        if let Some(children) = value.get("c").and_then(Value::as_object) {
            for (child, v) in children {
                let i = Self::read(v, child, model);
                model.parts[index].children.push(i);
            }
        }
        index
    }
}

/// `AnimationChannel.Target`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Position,
    Rotation,
    Scale,
}

#[derive(Clone, Copy, Debug)]
pub struct Keyframe {
    pub time: f32,
    pub pre: Vec3,
    pub post: Vec3,
    pub catmull_rom: bool,
}

#[derive(Clone, Debug)]
pub struct Channel {
    pub bone: String,
    pub target: Target,
    pub keyframes: Vec<Keyframe>,
}

/// `AnimationDefinition`.
#[derive(Clone, Debug)]
pub struct Animation {
    pub length: f32,
    pub looping: bool,
    pub channels: Vec<Channel>,
}

/// `Mth.catmullrom`.
fn catmull_rom(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t * t * t)
}

impl Animation {
    /// `KeyframeAnimation.apply(millis, targetScale)`: each channel's value
    /// at the elapsed time (looping over its length), added to its bone.
    pub fn apply(&self, posed: &mut Posed, millis: f32, target_scale: f32) {
        let seconds = millis / 1000.0;
        let seconds = if self.looping && self.length > 0.0 { seconds % self.length } else { seconds };
        for channel in &self.channels {
            let Some(index) = posed.model.part(&channel.bone) else { continue };
            let k = &channel.keyframes;
            if k.is_empty() {
                continue;
            }
            // `Mth.binarySearch`: the first frame at or after now, less one.
            let first_after = k.iter().position(|f| seconds <= f.time).unwrap_or(k.len());
            let prev = first_after.saturating_sub(1);
            let next = (prev + 1).min(k.len() - 1);
            let alpha = if next != prev { ((seconds - k[prev].time) / (k[next].time - k[prev].time)).clamp(0.0, 1.0) } else { 0.0 };
            let value = if k[next].catmull_rom {
                let p0 = k[prev.saturating_sub(1)].post;
                let p1 = k[prev].post;
                let p2 = k[next].post;
                let p3 = k[(next + 1).min(k.len() - 1)].post;
                Vec3::new(catmull_rom(alpha, p0.x, p1.x, p2.x, p3.x), catmull_rom(alpha, p0.y, p1.y, p2.y, p3.y), catmull_rom(alpha, p0.z, p1.z, p2.z, p3.z)) * target_scale
            } else {
                k[prev].post.lerp(k[next].pre, alpha) * target_scale
            };
            let pose = &mut posed.poses[index];
            match channel.target {
                Target::Position => pose.offset += value,
                Target::Rotation => pose.rotation += value,
                Target::Scale => pose.scale += value,
            }
        }
    }

    /// `applyWalk(animationPos, animationSpeed, speedFactor, scaleFactor)`.
    pub fn apply_walk(&self, posed: &mut Posed, position: f32, speed: f32, speed_factor: f32, scale_factor: f32) {
        let millis = (position * 50.0 * speed_factor).floor();
        self.apply(posed, millis, (speed * scale_factor).min(1.0));
    }
}

struct Library {
    models: HashMap<String, Model>,
    animations: HashMap<String, Animation>,
}

static LIBRARY: OnceLock<Library> = OnceLock::new();

/// Installs the dumped models (`entity-models-26.3.json`).
pub fn install(json: &[u8]) -> Result<(), String> {
    let root: Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    let mut models = HashMap::new();
    for (name, layer) in root.get("layers").and_then(Value::as_object).ok_or("no layers")? {
        let mut model = Model::default();
        Model::read(layer, "root", &mut model);
        models.insert(name.clone(), model);
    }
    let mut animations = HashMap::new();
    for (name, a) in root.get("animations").and_then(Value::as_object).ok_or("no animations")? {
        let mut channels = Vec::new();
        for (bone, list) in a.get("bones").and_then(Value::as_object).into_iter().flatten() {
            for channel in list.as_array().into_iter().flatten() {
                let items = channel.as_array().cloned().unwrap_or_default();
                let target = match items.first().and_then(Value::as_i64) {
                    Some(0) => Target::Position,
                    Some(1) => Target::Rotation,
                    _ => Target::Scale,
                };
                let keyframes = items[1..]
                    .iter()
                    .filter_map(|k| {
                        let n: Vec<f32> = k.as_array()?.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
                        (n.len() >= 8).then(|| Keyframe { time: n[0], pre: Vec3::new(n[1], n[2], n[3]), post: Vec3::new(n[4], n[5], n[6]), catmull_rom: n[7] != 0.0 })
                    })
                    .collect();
                channels.push(Channel { bone: bone.clone(), target, keyframes });
            }
        }
        let length = a.get("len").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let looping = a.get("loop").and_then(Value::as_bool).unwrap_or(false);
        animations.insert(name.clone(), Animation { length, looping, channels });
    }
    LIBRARY.set(Library { models, animations }).map_err(|_| "entity models already installed".to_owned())
}

/// A layer's model (`minecraft:cat#main`), once installed.
pub fn model(layer: &str) -> Option<&'static Model> {
    LIBRARY.get()?.models.get(layer)
}

/// A keyframe animation (`CamelAnimation.CAMEL_WALK`).
pub fn animation(name: &str) -> Option<&'static Animation> {
    LIBRARY.get()?.animations.get(name)
}

/// A model with its parts' poses for one frame (`resetPose` first).
pub struct Posed<'a> {
    pub model: &'a Model,
    pub poses: Vec<PartPose>,
    pub visible: Vec<bool>,
}

impl<'a> Posed<'a> {
    pub fn new(model: &'a Model) -> Self {
        Self { model, poses: model.parts.iter().map(|p| p.pose).collect(), visible: vec![true; model.parts.len()] }
    }

    /// A part's pose to change, by name.
    pub fn get(&mut self, name: &str) -> Option<&mut PartPose> {
        let i = self.model.part(name)?;
        Some(&mut self.poses[i])
    }

    /// Changes a part's pose, when the model has it.
    pub fn with(&mut self, name: &str, f: impl FnOnce(&mut PartPose)) {
        if let Some(pose) = self.get(name) {
            f(pose);
        }
    }

    /// The part's initial pose.
    pub fn initial(&self, name: &str) -> Option<PartPose> {
        Some(self.model.parts[self.model.part(name)?].pose)
    }

    pub fn hide(&mut self, name: &str) {
        if let Some(i) = self.model.part(name) {
            self.visible[i] = false;
        }
    }

    /// The head's look (`head.yRot`, `head.xRot`) in degrees.
    pub fn look(&mut self, part: &str, yaw: f32, pitch: f32) {
        self.with(part, |p| {
            p.rotation.y = yaw.to_radians();
            p.rotation.x = pitch.to_radians();
        });
    }
}

/// Where and how a posed model is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub feet: DVec3,
    /// The body's turn (and death flip).
    pub rotation: Quat,
    /// The renderer's model scale (`scale` on the pose stack, babies' age
    /// scale among it).
    pub scale: Vec3,
    /// The texture's region in the atlas.
    pub region: [f32; 4],
    pub sky: f32,
    pub block: f32,
    pub tint: [f32; 4],
}

/// Draws a posed model: every visible part with its children, each
/// polygon through its parts' poses, the living renderer's flip and the
/// body's turn, lit by the entity shader's two lights.
pub fn draw(mesh: &mut ChunkMesh, posed: &Posed, placement: &Placement) {
    draw_part(mesh, posed, 0, Mat4::IDENTITY, placement);
}

fn draw_part(mesh: &mut ChunkMesh, posed: &Posed, index: usize, parent: Mat4, placement: &Placement) {
    if !posed.visible[index] {
        return;
    }
    let part = &posed.model.parts[index];
    let matrix = parent * posed.poses[index].matrix();
    let normal_matrix = Mat3::from_mat4(matrix);
    let flip = |v: Vec3| Vec3::new(-v.x, 1.501 - v.y, v.z);
    let region = placement.region;
    for quad in &part.quads {
        let n = normal_matrix * quad.normal;
        let n = placement.rotation * (Vec3::new(-n.x, -n.y, n.z) / placement.scale).normalize_or_zero();
        let shade = crate::cow_render::entity_shade(n);
        let start = mesh.vertices.len() as u32;
        for (corner, uv) in quad.corners {
            let model = matrix.transform_point3(corner / 16.0);
            let local = flip(model) * placement.scale;
            let world = placement.feet.as_vec3() + placement.rotation * local;
            mesh.vertices.push(Vertex {
                position: world.to_array(),
                uv: [region[0] + (region[2] - region[0]) * uv[0], region[1] + (region[3] - region[1]) * uv[1]],
                color: [shade * placement.tint[0], shade * placement.tint[1], shade * placement.tint[2], placement.tint[3]],
                sky_light: placement.sky,
                block_light: placement.block,
            });
        }
        mesh.indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        mesh.faces += 1;
    }
    for &child in &part.children {
        draw_part(mesh, posed, child, matrix, placement);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The data path draws the cow's head where the hand-built cow renderer
    /// puts it (`ENTITY_MODELS_JSON`: the unpacked dump; skipped without).
    #[test]
    fn cow_head_matches_the_hand_built_cube() {
        let Ok(path) = std::env::var("ENTITY_MODELS_JSON") else { return };
        install(&std::fs::read(path).unwrap()).unwrap();
        let cow = model("minecraft:cow#main").unwrap();
        let mut posed = Posed::new(cow);
        for part in ["body", "left_front_leg", "left_hind_leg", "right_front_leg", "right_hind_leg"] {
            posed.hide(part);
        }
        posed.look("head", 30.0, 10.0);
        let rotation = Quat::from_rotation_y(0.7);
        let placement = Placement { feet: DVec3::new(1.0, 2.0, 3.0), rotation, scale: Vec3::ONE, region: [0.0, 0.0, 1.0, 1.0], sky: 15.0, block: 0.0, tint: [1.0; 4] };
        let mut data = ChunkMesh::default();
        draw(&mut data, &posed, &placement);
        let mut hand = ChunkMesh::default();
        let pose = Quat::from_euler(glam::EulerRot::ZYX, 0.0, 30.0_f32.to_radians(), 10.0_f32.to_radians());
        crate::cow_render::cube_scaled(&mut hand, placement.feet, rotation, Vec3::ONE, [0.0, 0.0, 1.0, 1.0], 15.0, 0.0, [-4., -4., -6.], [4., 4., 0.], [0., 0.], [0., 4., -8.], pose, [1.0; 3], [64.0, 64.0], None, false);
        for (a, b) in data.vertices.iter().zip(&hand.vertices).take(24) {
            for i in 0..3 {
                assert!((a.position[i] - b.position[i]).abs() < 1.0e-4, "{:?} vs {:?}", a.position, b.position);
            }
            assert!((a.uv[0] - b.uv[0]).abs() < 1.0e-5 && (a.uv[1] - b.uv[1]).abs() < 1.0e-5, "{:?} vs {:?}", a.uv, b.uv);
        }
    }
}
