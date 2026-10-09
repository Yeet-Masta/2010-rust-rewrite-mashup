//! GUI icons for items drawn by special model renderers (chests, shulker
//! boxes, banners, heads, shields, decorated pots, the conduit): the item
//! definition's GUI model as 26.3 resolves it (`display_context` "gui",
//! each node's `transformation` composed), the renderer's `ModelPart`
//! cuboids from its layer definition, the base model's GUI `ItemTransform`,
//! and `GuiItemAtlas`'s pose and item lighting (`ITEMS_3D`, or
//! `ITEMS_FLAT` for a front-lit base model).
use crate::mesh::{Atlas, ChunkMesh, Vertex};
use crate::pack::{PackStack, ResourceId};
use anyhow::Result;
use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};
use image::RgbaImage;
use serde_json::Value;

/// `ModelPart.Cube`: its texture offset, corner, size, growth, mirror and
/// the faces it keeps (`Direction` ordinals: down, up, north, south,
/// west, east).
#[derive(Clone, Copy)]
struct Cube {
    uv: [f32; 2],
    from: [f32; 3],
    size: [f32; 3],
    grow: f32,
    mirror: bool,
    faces: u8,
}

const ALL: u8 = 0b11_1111;
const NORTH: u8 = 1 << 2;
const WEST: u8 = 1 << 4;
const EAST: u8 = 1 << 5;

fn cube(u: f32, v: f32, from: [f32; 3], size: [f32; 3]) -> Cube {
    Cube {
        uv: [u, v],
        from,
        size,
        grow: 0.0,
        mirror: false,
        faces: ALL,
    }
}

/// `PartDefinition`: its pose, cuboids and children.
#[derive(Clone, Default)]
pub(crate) struct Part {
    pub(crate) offset: [f32; 3],
    pub(crate) rotation: [f32; 3],
    scale: f32,
    cubes: Vec<Cube>,
    children: Vec<Part>,
}

fn part(offset: [f32; 3], cubes: Vec<Cube>) -> Part {
    Part {
        offset,
        rotation: [0.0; 3],
        scale: 1.0,
        cubes,
        children: Vec::new(),
    }
}

impl Part {
    fn rotated(mut self, rotation: [f32; 3]) -> Self {
        self.rotation = rotation;
        self
    }

    fn with(mut self, children: Vec<Part>) -> Self {
        self.children = children;
        self
    }

    /// `ModelPart.translateAndRotate`.
    fn matrix(&self) -> Mat4 {
        Mat4::from_translation(Vec3::from(self.offset) / 16.0)
            * Mat4::from_quat(Quat::from_euler(
                EulerRot::ZYX,
                self.rotation[2],
                self.rotation[1],
                self.rotation[0],
            ))
            * Mat4::from_scale(Vec3::splat(self.scale))
    }
}

/// One model drawn with one texture: its sheet, the sheet's size the
/// layer's UVs count in, a colour and whether it blends over what is there
/// (a banner's pattern layers).
struct Layer {
    texture: String,
    size: [f32; 2],
    tint: u32,
    overlay: bool,
    parts: Vec<Part>,
}

fn layer(texture: impl Into<String>, size: [f32; 2], parts: Vec<Part>) -> Layer {
    Layer {
        texture: texture.into(),
        size,
        tint: 0xFFFFFF,
        overlay: false,
        parts,
    }
}

/// `DyeColor.getTextureDiffuseColor`.
fn dye(name: &str) -> u32 {
    match name {
        "orange" => 16351261,
        "magenta" => 13061821,
        "light_blue" => 3847130,
        "yellow" => 16701501,
        "lime" => 8439583,
        "pink" => 15961002,
        "gray" => 4673362,
        "light_gray" => 10329495,
        "cyan" => 1481884,
        "purple" => 8991416,
        "blue" => 3949738,
        "brown" => 8606770,
        "green" => 6192150,
        "red" => 11546150,
        "black" => 1908001,
        _ => 16383998,
    }
}

/// The sheet path a `minecraft:x` texture id names under `entity/<dir>`.
fn sheet(dir: &str, id: &str) -> String {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("{namespace}:entity/{dir}/{path}")
}

/// The special renderer's layers (`SpecialModelRenderers`), for an item's
/// components.
fn layers(model: &Value, components: Option<&Value>) -> Result<Option<Vec<Layer>>> {
    let kind = model["type"].as_str().unwrap_or("");
    Ok(Some(match kind {
        "minecraft:chest" => {
            let texture = model["texture"].as_str().unwrap_or("minecraft:normal");
            vec![layer(sheet("chest", texture), [64.0, 64.0], chest_model("single"))]
        }
        // Closed.
        "minecraft:shulker_box" => {
            let texture = model["texture"].as_str().unwrap_or("minecraft:shulker");
            vec![layer(sheet("shulker", texture), [64.0, 64.0], shulker_box_model())]
        }
        // `BannerModel` and `BannerFlagModel` standing, then the flag's
        // pattern layers (`BannerRenderer.submitPatterns`).
        "minecraft:banner" => {
            let base = model["color"].as_str().unwrap_or("white");
            let flag = || {
                // `setupAnim(0)`.
                vec![part(
                    [0.0, -44.0, 0.0],
                    vec![cube(0.0, 0.0, [-10.0, 0.0, -2.0], [20.0, 40.0, 1.0])],
                )
                .rotated([(-0.0125 + 0.01) * std::f32::consts::PI, 0.0, 0.0])]
            };
            let mut layers = vec![
                layer(
                    "minecraft:entity/banner/banner_base",
                    [64.0, 64.0],
                    vec![
                        part([0.0; 3], vec![cube(44.0, 0.0, [-1.0, -42.0, -1.0], [2.0, 42.0, 2.0])]),
                        part([0.0; 3], vec![cube(0.0, 42.0, [-10.0, -44.0, -1.0], [20.0, 2.0, 2.0])]),
                    ],
                ),
                layer("minecraft:entity/banner/banner_base", [64.0, 64.0], flag()),
            ];
            let mut patterns = vec![("minecraft:base".to_owned(), base.to_owned())];
            for entry in components
                .and_then(|c| c["minecraft:banner_patterns"].as_array())
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .iter()
                .take(16)
            {
                let (Some(pattern), Some(color)) = (entry["pattern"].as_str(), entry["color"].as_str()) else {
                    continue;
                };
                // Vanilla's patterns' `asset_id`s are their own ids.
                patterns.push((pattern.to_owned(), color.to_owned()));
            }
            for (asset, color) in patterns {
                let mut pattern = layer(sheet("banner", &asset), [64.0, 64.0], flag());
                pattern.tint = dye(&color);
                pattern.overlay = true;
                layers.push(pattern);
            }
            layers
        }
        // `ShieldModel`, unpatterned.
        "minecraft:shield" => vec![layer(
            "minecraft:entity/shield/shield_base_nopattern",
            [64.0, 64.0],
            vec![
                part([0.0; 3], vec![cube(0.0, 0.0, [-6.0, -11.0, -2.0], [12.0, 22.0, 1.0])]),
                part([0.0; 3], vec![cube(26.0, 0.0, [-1.0, -3.0, -1.0], [2.0, 6.0, 6.0])]),
            ],
        )],
        // `TridentModel.createLayer`.
        "minecraft:trident" => {
            let mut right_spike = cube(4.0, 3.0, [1.5, -3.0, -0.5], [1.0, 4.0, 1.0]);
            right_spike.mirror = true;
            vec![layer(
                "minecraft:entity/trident/trident",
                [32.0, 32.0],
                vec![part([0.0; 3], vec![cube(0.0, 6.0, [-0.5, 2.0, -0.5], [1.0, 25.0, 1.0])]).with(vec![
                    part([0.0; 3], vec![cube(4.0, 0.0, [-1.5, 0.0, -0.5], [3.0, 2.0, 1.0])]),
                    part([0.0; 3], vec![cube(4.0, 3.0, [-2.5, -3.0, -0.5], [1.0, 4.0, 1.0])]),
                    part([0.0; 3], vec![cube(0.0, 0.0, [-0.5, -4.0, -0.5], [1.0, 4.0, 1.0])]),
                    part([0.0; 3], vec![right_spike]),
                ])],
            )]
        }
        // `ConduitRenderer.createShellLayer`.
        "minecraft:conduit" => vec![layer(
            "minecraft:entity/conduit/base",
            [32.0, 16.0],
            vec![part([0.0; 3], vec![cube(0.0, 0.0, [-3.0, -3.0, -3.0], [6.0, 6.0, 6.0])])],
        )],
        // `DecoratedPotRenderer`'s base and plain sides.
        "minecraft:decorated_pot" => {
            let pi = std::f32::consts::PI;
            let half = std::f32::consts::FRAC_PI_2;
            let mut neck_inner = cube(0.0, 0.0, [4.0, 17.0, 4.0], [8.0, 3.0, 8.0]);
            neck_inner.grow = -0.1;
            let mut neck_outer = cube(0.0, 5.0, [5.0, 20.0, 5.0], [6.0, 1.0, 6.0]);
            neck_outer.grow = 0.2;
            let plane = cube(-14.0, 13.0, [0.0; 3], [14.0, 0.0, 14.0]);
            let mut side = cube(1.0, 0.0, [0.0; 3], [14.0, 16.0, 0.0]);
            side.faces = NORTH;
            vec![
                layer(
                    "minecraft:entity/decorated_pot/decorated_pot_base",
                    [32.0, 32.0],
                    vec![
                        part([0.0, 37.0, 16.0], vec![neck_inner, neck_outer]).rotated([pi, 0.0, 0.0]),
                        part([1.0, 16.0, 1.0], vec![plane]),
                        part([1.0, 0.0, 1.0], vec![plane]),
                    ],
                ),
                layer(
                    "minecraft:entity/decorated_pot/decorated_pot_side",
                    [16.0, 16.0],
                    vec![
                        part([15.0, 16.0, 1.0], vec![side]).rotated([0.0, 0.0, pi]),
                        part([1.0, 16.0, 1.0], vec![side]).rotated([0.0, -half, pi]),
                        part([15.0, 16.0, 15.0], vec![side]).rotated([0.0, half, pi]),
                        part([1.0, 16.0, 15.0], vec![side]).rotated([pi, 0.0, 0.0]),
                    ],
                ),
            ]
        }
        "minecraft:player_head" => vec![humanoid_head("minecraft:entity/player/wide/steve")],
        "minecraft:head" => {
            let kind = model["kind"].as_str().unwrap_or("skeleton");
            let texture = model["texture"].as_str().map(str::to_owned);
            match kind {
                "zombie" | "player" => vec![humanoid_head(&texture.unwrap_or_else(|| {
                    if kind == "zombie" {
                        "minecraft:entity/zombie/zombie".into()
                    } else {
                        "minecraft:entity/player/wide/steve".into()
                    }
                }))],
                "piglin" => vec![piglin_head(
                    &texture.unwrap_or_else(|| "minecraft:entity/piglin/piglin".into()),
                )],
                "dragon" => vec![dragon_head(
                    &texture.unwrap_or_else(|| "minecraft:entity/enderdragon/dragon".into()),
                )],
                _ => {
                    let default = match kind {
                        "wither_skeleton" => "minecraft:entity/skeleton/wither_skeleton",
                        "creeper" => "minecraft:entity/creeper/creeper",
                        _ => "minecraft:entity/skeleton/skeleton",
                    };
                    // `SkullModel.createMobHeadLayer`.
                    vec![layer(
                        texture.unwrap_or_else(|| default.into()),
                        [64.0, 32.0],
                        vec![part([0.0; 3], vec![cube(0.0, 0.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0])])],
                    )]
                }
            }
        }
        "minecraft:copper_golem_statue" => {
            // The texture is named by its file: `textures/<path>.png`.
            let texture = model["texture"].as_str().unwrap_or("minecraft:textures/entity/copper_golem/copper_golem.png");
            let (namespace, path) = texture.split_once(':').unwrap_or(("minecraft", texture));
            let path = path.strip_prefix("textures/").unwrap_or(path);
            let path = path.strip_suffix(".png").unwrap_or(path);
            vec![copper_golem(model["pose"].as_str().unwrap_or("standing"), &format!("{namespace}:{path}"))]
        }
        _ => return Ok(None),
    }))
}

/// `ChestModel`'s layer for a chest of `chest_type` (`single`, `left`,
/// `right`): `createSingleBodyLayer`, or `createDoubleBodyLeftLayer` and
/// `createDoubleBodyRightLayer`, whose halves run to the join and leave
/// out its face. The bottom, then the lid and the lock, both hinged at
/// 9 up and 1 in.
pub(crate) fn chest_model(chest_type: &str) -> Vec<Part> {
    let (x, width, lock_x, lock_width, faces) = match chest_type {
        "left" => (0.0, 15.0, 0.0, 1.0, ALL & !WEST),
        "right" => (1.0, 15.0, 15.0, 1.0, ALL & !EAST),
        _ => (1.0, 14.0, 7.0, 2.0, ALL),
    };
    let sided = |mut cube: Cube| {
        cube.faces = faces;
        cube
    };
    vec![
        part([0.0; 3], vec![sided(cube(0.0, 19.0, [x, 0.0, 1.0], [width, 10.0, 14.0]))]),
        part([0.0, 9.0, 1.0], vec![sided(cube(0.0, 0.0, [x, 0.0, 0.0], [width, 5.0, 14.0]))]),
        part([0.0, 9.0, 1.0], vec![sided(cube(0.0, 0.0, [lock_x, -2.0, 14.0], [lock_width, 4.0, 1.0]))]),
    ]
}

/// `ShulkerModel.createBoxLayer` (`createShellMesh`): the lid, then the
/// base, both at 24 down in the model's flipped space.
pub(crate) fn shulker_box_model() -> Vec<Part> {
    vec![
        part([0.0, 24.0, 0.0], vec![cube(0.0, 0.0, [-8.0, -16.0, -8.0], [16.0, 12.0, 16.0])]),
        part([0.0, 24.0, 0.0], vec![cube(0.0, 28.0, [-8.0, -8.0, -8.0], [16.0, 8.0, 16.0])]),
    ]
}

fn grown(mut cube: Cube, grow: f32) -> Cube {
    cube.grow = grow;
    cube
}

/// `CopperGolemModel`'s layer for a statue's pose (`createBodyLayer`,
/// `createSittingPoseBodyLayer`, `createStarPoseBodyLayer` or
/// `createRunningPoseBodyLayer`), its root posed by
/// `CopperGolemStatueModel.setupAnim`.
fn copper_golem(pose: &str, texture: &str) -> Layer {
    let z = 0.015;
    let arm = |offset: [f32; 3], children: Vec<Part>| part(offset, Vec::new()).with(children);
    let parts = match pose {
        "sitting" => vec![
            part(
                [0.0, -3.0, 2.325],
                vec![
                    cube(3.0, 19.0, [-3.0, -4.0, -4.525], [6.0, 1.0, 6.0]),
                    cube(0.0, 15.0, [-4.0, -3.0, -3.525], [8.0, 6.0, 6.0]),
                ],
            )
            .with(vec![
                part([0.0, -1.0, -4.325], vec![cube(3.0, 18.0, [-4.0, -3.0, -2.2], [8.0, 6.0, 3.0])])
                    .rotated([0.0, 0.0, -std::f32::consts::PI]),
                part(
                    [0.0, -6.0, -0.2],
                    vec![
                        grown(cube(37.0, 8.0, [-1.0, -7.0, -3.3], [2.0, 4.0, 2.0]), -z),
                        grown(cube(37.0, 0.0, [-2.0, -11.0, -4.3], [4.0, 4.0, 4.0]), -z),
                        cube(0.0, 0.0, [-4.0, -3.0, -7.325], [8.0, 5.0, 10.0]),
                        cube(56.0, 0.0, [-1.0, 0.0, -8.325], [2.0, 3.0, 2.0]),
                    ],
                ),
                arm(
                    [-4.0, -5.6, -1.8],
                    vec![part(
                        [0.0, 0.0893, 0.1198],
                        vec![cube(36.0, 16.0, [-3.075, -0.9733, -1.9966], [3.0, 10.0, 4.0])],
                    )
                    .rotated([-std::f32::consts::FRAC_PI_3, 0.0, 0.0])],
                )
                .rotated([0.4363, 0.0, 0.0]),
                arm(
                    [4.0, -5.6, -1.7],
                    vec![part(
                        [0.0, -0.0015, -0.0808],
                        vec![cube(50.0, 16.0, [0.075, -1.0443, -1.8997], [3.0, 10.0, 4.0])],
                    )
                    .rotated([-std::f32::consts::FRAC_PI_3, 0.0, 0.0])],
                )
                .rotated([0.4363, 0.0, 0.0]),
            ]),
            arm(
                [-2.1, -2.1, -2.075],
                vec![part([0.05, -1.9, 1.075], vec![cube(0.0, 27.0, [-2.0, 0.975, 0.0], [4.0, 5.0, 4.0])])
                    .rotated([-std::f32::consts::FRAC_PI_2, 0.0, 0.0])],
            ),
            arm(
                [2.0, -2.0, -2.075],
                vec![part([0.05, -2.0, 1.075], vec![cube(16.0, 27.0, [-2.0, 0.975, 0.0], [4.0, 5.0, 4.0])])
                    .rotated([-std::f32::consts::FRAC_PI_2, 0.0, 0.0])],
            ),
        ],
        "running" => vec![
            arm(
                [-1.064, -5.0, 0.0],
                vec![
                    part([1.1, 0.1, 0.7], vec![cube(0.0, 15.0, [-4.02, -6.116, -3.5], [8.0, 6.0, 6.0])])
                        .rotated([0.1204, -0.0064, -0.0779]),
                    part(
                        [0.7, -5.6, -1.8],
                        vec![
                            cube(0.0, 0.0, [-4.0, -5.1, -5.0], [8.0, 5.0, 10.0]),
                            cube(56.0, 0.0, [-1.02, -2.1, -6.0], [2.0, 3.0, 2.0]),
                            grown(cube(37.0, 8.0, [-1.02, -9.1, -1.0], [2.0, 4.0, 2.0]), -z),
                            grown(cube(37.0, 0.0, [-2.0, -13.1, -2.0], [4.0, 4.0, 4.0]), -z),
                        ],
                    ),
                    arm(
                        [-4.0, -6.0, 0.0],
                        vec![part(
                            [0.7, -0.248, -1.62],
                            vec![cube(36.0, 16.0, [-3.052, -1.11, -2.036], [3.0, 10.0, 4.0])],
                        )
                        .rotated([1.0036, 0.0, 0.0])],
                    ),
                    arm(
                        [4.0, -6.0, 0.0],
                        vec![part([0.732, 0.0, 0.0], vec![cube(50.0, 16.0, [0.032, -1.1, -2.0], [3.0, 10.0, 4.0])])
                            .rotated([-0.8715, -0.0535, -0.0449])],
                    ),
                ],
            ),
            arm(
                [-3.064, -5.0, 0.0],
                vec![part([1.048, 0.0, -0.9], vec![cube(0.0, 27.0, [-1.856, -0.1, -1.09], [4.0, 5.0, 4.0])])
                    .rotated([-0.8727, 0.0, 0.0])],
            ),
            arm(
                [0.936, -5.0, 0.0],
                vec![part([1.0, 0.0, 0.0], vec![cube(16.0, 27.0, [-2.088, -0.1, -2.0], [4.0, 5.0, 4.0])])
                    .rotated([std::f32::consts::FRAC_PI_4, 0.0, 0.0])],
            ),
        ],
        _ => {
            let star = pose == "star";
            let head = part(
                [0.0, -6.0, 0.0],
                vec![
                    grown(cube(0.0, 0.0, [-4.0, -5.0, -5.0], [8.0, 5.0, 10.0]), if star { 0.0 } else { z }),
                    cube(56.0, 0.0, [-1.0, -2.0, -6.0], [2.0, 3.0, 2.0]),
                    grown(cube(37.0, 8.0, [-1.0, -9.0, -1.0], [2.0, 4.0, 2.0]), -z),
                    grown(cube(37.0, 0.0, [-2.0, -13.0, -2.0], [4.0, 4.0, 4.0]), -z),
                ],
            );
            let right_arm = cube(36.0, 16.0, [-3.0, -1.0, -2.0], [3.0, 10.0, 4.0]);
            let left_arm = cube(50.0, 16.0, [0.0, -1.0, -2.0], [3.0, 10.0, 4.0]);
            let right_leg = cube(0.0, 27.0, [-4.0, 0.0, -2.0], [4.0, 5.0, 4.0]);
            let left_leg = cube(16.0, 27.0, [0.0, 0.0, -2.0], [4.0, 5.0, 4.0]);
            let (arms, legs) = if star {
                let spread = 1.9199;
                let tilt = 0.2618;
                (
                    [
                        arm(
                            [-4.0, -6.0, 0.0],
                            vec![part([1.0, 1.0, 0.0], vec![cube(36.0, 16.0, [-1.5, -5.0, -2.0], [3.0, 10.0, 4.0])])
                                .rotated([0.0, 0.0, spread])],
                        ),
                        arm(
                            [4.0, -6.0, 0.0],
                            vec![part([-1.0, 1.0, 0.0], vec![cube(50.0, 16.0, [-1.5, -5.0, -2.0], [3.0, 10.0, 4.0])])
                                .rotated([0.0, 0.0, -spread])],
                        ),
                    ],
                    [
                        arm(
                            [-3.0, -5.0, 0.0],
                            vec![part([0.35, 2.0, 0.01], vec![cube(0.0, 27.0, [-2.0, -2.5, -2.0], [4.0, 5.0, 4.0])])
                                .rotated([0.0, 0.0, tilt])],
                        ),
                        arm(
                            [1.0, -5.0, 0.0],
                            vec![part([1.65, 2.0, 0.0], vec![cube(16.0, 27.0, [-2.0, -2.5, -2.0], [4.0, 5.0, 4.0])])
                                .rotated([0.0, 0.0, -tilt])],
                        ),
                    ],
                )
            } else {
                (
                    [part([-4.0, -6.0, 0.0], vec![right_arm]), part([4.0, -6.0, 0.0], vec![left_arm])],
                    [part([0.0, -5.0, 0.0], vec![right_leg]), part([0.0, -5.0, 0.0], vec![left_leg])],
                )
            };
            let [right_arm, left_arm] = arms;
            let [right_leg, left_leg] = legs;
            vec![
                part([0.0, -5.0, 0.0], vec![cube(0.0, 15.0, [-4.0, -6.0, -3.0], [8.0, 6.0, 6.0])])
                    .with(vec![head, right_arm, left_arm]),
                right_leg,
                left_leg,
            ]
        }
    };
    layer(
        texture,
        [64.0, 64.0],
        vec![part([0.0; 3], Vec::new()).rotated([0.0, 0.0, std::f32::consts::PI]).with(parts)],
    )
}

/// `SkullModel.createHumanoidHeadLayer`.
fn humanoid_head(texture: &str) -> Layer {
    let mut hat = cube(32.0, 0.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]);
    hat.grow = 0.25;
    layer(
        texture,
        [64.0, 64.0],
        vec![part([0.0; 3], vec![cube(0.0, 0.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0])])
            .with(vec![part([0.0; 3], vec![hat])])],
    )
}

/// `PiglinHeadModel`: `PiglinModel.addHead` with its ears at rest.
fn piglin_head(texture: &str) -> Layer {
    let ears = (0.0f32.cos() + 2.5) * 0.2;
    layer(
        texture,
        [64.0, 64.0],
        vec![part(
            [0.0; 3],
            vec![
                cube(0.0, 0.0, [-5.0, -8.0, -4.0], [10.0, 8.0, 8.0]),
                cube(31.0, 1.0, [-2.0, -4.0, -5.0], [4.0, 4.0, 1.0]),
                cube(2.0, 4.0, [2.0, -2.0, -5.0], [1.0, 2.0, 1.0]),
                cube(2.0, 0.0, [-3.0, -2.0, -5.0], [1.0, 2.0, 1.0]),
            ],
        )
        .with(vec![
            part([4.5, -6.0, 0.0], vec![cube(51.0, 6.0, [0.0, 0.0, -2.0], [1.0, 5.0, 4.0])])
                .rotated([0.0, 0.0, -ears]),
            part([-4.5, -6.0, 0.0], vec![cube(39.0, 6.0, [-1.0, 0.0, -2.0], [1.0, 5.0, 4.0])])
                .rotated([0.0, 0.0, ears]),
        ])],
    )
}

/// `DragonHeadModel.createHeadLayer`, its jaw at rest.
fn dragon_head(texture: &str) -> Layer {
    let mirrored = |mut c: Cube| {
        c.mirror = true;
        c
    };
    let mut head = part(
        [0.0, -7.986666, 0.0],
        vec![
            cube(176.0, 44.0, [-6.0, -1.0, -24.0], [12.0, 5.0, 16.0]),
            cube(112.0, 30.0, [-8.0, -8.0, -10.0], [16.0, 16.0, 16.0]),
            mirrored(cube(0.0, 0.0, [-5.0, -12.0, -4.0], [2.0, 4.0, 6.0])),
            mirrored(cube(112.0, 0.0, [-5.0, -3.0, -22.0], [2.0, 2.0, 4.0])),
            cube(0.0, 0.0, [3.0, -12.0, -4.0], [2.0, 4.0, 6.0]),
            cube(112.0, 0.0, [3.0, -3.0, -22.0], [2.0, 2.0, 4.0]),
        ],
    )
    .with(vec![
        part([0.0, 4.0, -8.0], vec![cube(176.0, 65.0, [-6.0, 0.0, -16.0], [12.0, 4.0, 16.0])])
            .rotated([(0.0f32.sin() + 1.0) * 0.2, 0.0, 0.0]),
    ]);
    head.scale = 0.75;
    layer(texture, [256.0, 256.0], vec![head])
}

/// The GUI's model node of an item definition, with the transformations
/// above it composed.
fn gui_node(value: &Value, transform: Mat4) -> Option<(&Value, Mat4)> {
    node_for(value, "gui", transform)
}

/// The special model node an item definition picks in a display context
/// (`ItemDisplayContext`'s name), with the transformations above it
/// composed.
fn node_for<'a>(value: &'a Value, context: &str, transform: Mat4) -> Option<(&'a Value, Mat4)> {
    let transform = transform * transformation(value.get("transformation"));
    match value["type"].as_str()? {
        "minecraft:special" => Some((value, transform)),
        "minecraft:select" => {
            let display = value["property"] == "minecraft:display_context";
            let case = value["cases"].as_array().and_then(|cases| {
                cases.iter().find(|case| {
                    display
                        && case["when"]
                            .as_array()
                            .map(Vec::as_slice)
                            .unwrap_or(&[])
                            .iter()
                            .chain(std::iter::once(&case["when"]))
                            .any(|when| when == context)
                })
            });
            match case {
                Some(case) => node_for(&case["model"], context, transform),
                None => node_for(value.get("fallback")?, context, transform),
            }
        }
        "minecraft:condition" => node_for(value.get("on_false")?, context, transform),
        "minecraft:range_dispatch" => node_for(value.get("fallback")?, context, transform),
        _ => None,
    }
}

/// `Transformation`: translation, left rotation, scale, right rotation.
fn transformation(value: Option<&Value>) -> Mat4 {
    let Some(value) = value else {
        return Mat4::IDENTITY;
    };
    let vector = |key: &str, default: f32| {
        let v = &value[key];
        Vec3::new(
            v[0].as_f64().map_or(default, |x| x as f32),
            v[1].as_f64().map_or(default, |x| x as f32),
            v[2].as_f64().map_or(default, |x| x as f32),
        )
    };
    let quat = |key: &str| {
        let v = &value[key];
        let q = Quat::from_xyzw(
            v[0].as_f64().unwrap_or(0.0) as f32,
            v[1].as_f64().unwrap_or(0.0) as f32,
            v[2].as_f64().unwrap_or(0.0) as f32,
            v[3].as_f64().unwrap_or(1.0) as f32,
        );
        if q.length_squared() > 0.0 { q.normalize() } else { Quat::IDENTITY }
    };
    Mat4::from_translation(vector("translation", 0.0))
        * Mat4::from_quat(quat("left_rotation"))
        * Mat4::from_scale(vector("scale", 1.0))
        * Mat4::from_quat(quat("right_rotation"))
}

/// The base model's GUI `ItemTransform` (`apply`, with its closing
/// half-block shift) and whether it is lit from the front.
fn display(packs: &PackStack, base: &str) -> Result<(Mat4, bool)> {
    display_for(packs, base, "gui", None)
}

/// The base model's `ItemTransform` for `context`; with `mirror_of`, the
/// left hand's (its own, else `mirror_of`'s, with `applyLeftHandFix`).
fn display_for(packs: &PackStack, base: &str, context: &str, mirror_of: Option<&str>) -> Result<(Mat4, bool)> {
    let mut gui = None;
    let mut fallback = None;
    let mut light = None;
    let mut current = Some(ResourceId::parse(base)?);
    for _ in 0..12 {
        let Some(id) = current.take() else { break };
        let Some(value) = packs.model(&id)? else { break };
        if gui.is_none() {
            gui = value.get("display").and_then(|d| d.get(context)).cloned();
        }
        if let (None, Some(other)) = (&fallback, mirror_of) {
            fallback = value.get("display").and_then(|d| d.get(other)).cloned();
        }
        if light.is_none() {
            light = value.get("gui_light").and_then(Value::as_str).map(str::to_owned);
        }
        current = value
            .get("parent")
            .and_then(Value::as_str)
            .map(ResourceId::parse)
            .transpose()?;
    }
    let gui = gui.or(fallback);
    let get = |key: &str, default: f32| {
        let v = gui.as_ref().map(|g| &g[key]);
        let at = |i: usize| {
            v.and_then(|v| v[i].as_f64())
                .map_or(default, |x| x as f32)
        };
        Vec3::new(at(0), at(1), at(2))
    };
    let mirror = if mirror_of.is_some() { Vec3::new(1.0, -1.0, -1.0) } else { Vec3::ONE };
    let rotation = get("rotation", 0.0) * (std::f32::consts::PI / 180.0) * mirror;
    let translation = get("translation", 0.0).clamp(Vec3::splat(-80.0), Vec3::splat(80.0))
        * if mirror_of.is_some() { Vec3::new(-1.0, 1.0, 1.0) } else { Vec3::ONE };
    let matrix = Mat4::from_translation(translation / 16.0)
        * Mat4::from_quat(Quat::from_euler(EulerRot::XYZ, rotation.x, rotation.y, rotation.z))
        * Mat4::from_scale(get("scale", 1.0))
        * Mat4::from_translation(Vec3::splat(-0.5));
    Ok((matrix, light.as_deref() == Some("front")))
}

/// `Lighting`'s item light directions in the GUI's space.
fn item_lights(flat: bool) -> [Vec3; 2] {
    let pose = if flat {
        Mat4::from_rotation_y(-std::f32::consts::PI / 8.0)
            * Mat4::from_rotation_x(std::f32::consts::PI * 3.0 / 4.0)
    } else {
        Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
            * Mat4::from_rotation_y(1.0821041)
            * Mat4::from_rotation_x(3.2375858)
            * Mat4::from_rotation_y(-std::f32::consts::PI / 8.0)
            * Mat4::from_rotation_x(std::f32::consts::PI * 3.0 / 4.0)
    };
    [
        pose.transform_vector3(Vec3::new(0.2, 1.0, -0.7).normalize()).normalize(),
        pose.transform_vector3(Vec3::new(-0.2, 1.0, 0.7).normalize()).normalize(),
    ]
}

/// A face to raster: its corners in icon pixels with depth, its texture
/// coordinates, and its light.
struct Face {
    corners: [Vec3; 4],
    uvs: [[f32; 2]; 4],
    shade: f32,
}

/// `ModelPart.Cube`'s polygons through `pose` (item space) and `gui` (to
/// icon pixels), front faces only.
fn cube_faces(c: &Cube, pose: Mat4, gui: Mat4, size: [f32; 2], lights: [Vec3; 2], out: &mut Vec<Face>) {
    let [x, y, z] = c.from;
    let [w, h, d] = c.size;
    let (mut x0, y0, z0) = (x - c.grow, y - c.grow, z - c.grow);
    let (mut x1, y1, z1) = (x + w + c.grow, y + h + c.grow, z + d + c.grow);
    if c.mirror {
        std::mem::swap(&mut x0, &mut x1);
    }
    let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) / 16.0;
    let (t0, t1, t2, t3) = (p(x0, y0, z0), p(x1, y0, z0), p(x1, y1, z0), p(x0, y1, z0));
    let (l0, l1, l2, l3) = (p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1));
    let [u, v] = c.uv;
    let (u0, u1, u2, u22, u3, u4) = (u, u + d, u + d + w, u + d + w + w, u + d + w + d, u + d + w + d + w);
    let (v0, v1, v2) = (v, v + d, v + d + h);
    // Polygon order and `Direction` ordinal, with each face's normal.
    let polygons = [
        ([l1, l0, t0, t1], [u1, v0, u2, v1], 0, Vec3::NEG_Y),
        ([t2, t3, l3, l2], [u2, v1, u22, v0], 1, Vec3::Y),
        ([t0, l0, l3, t3], [u0, v1, u1, v2], 4, Vec3::NEG_X),
        ([t1, t0, t3, t2], [u1, v1, u2, v2], 2, Vec3::NEG_Z),
        ([l1, t1, t2, l2], [u2, v1, u3, v2], 5, Vec3::X),
        ([l0, l1, l2, l3], [u3, v1, u4, v2], 3, Vec3::Z),
    ];
    let full = gui * pose;
    let normals = Mat3::from_mat4(full).inverse().transpose();
    for (corners, [ua, va, ub, vb], direction, normal) in polygons {
        if c.faces & (1 << direction) == 0 {
            continue;
        }
        let normal = if c.mirror { normal * Vec3::new(-1.0, 1.0, 1.0) } else { normal };
        let n = (normals * normal).normalize_or_zero();
        // Back faces are culled; the icon looks down -z.
        if n.z <= 0.0 {
            continue;
        }
        let mut uvs = [[ub, va], [ua, va], [ua, vb], [ub, vb]].map(|[s, t]| [s / size[0], t / size[1]]);
        let mut corners = corners.map(|c| full.transform_point3(c));
        if c.mirror {
            corners.reverse();
            uvs.reverse();
        }
        // The GUI's (16, -16, 16) scale turns the normal's y.
        let lit = Vec3::new(n.x, -n.y, n.z).normalize_or_zero();
        let light = lights[0].dot(lit).max(0.0) + lights[1].dot(lit).max(0.0);
        out.push(Face {
            corners,
            uvs,
            shade: (light * 0.6 + 0.4).min(1.0),
        });
    }
}

fn walk(part: &Part, parent: Mat4, gui: Mat4, size: [f32; 2], lights: [Vec3; 2], out: &mut Vec<Face>) {
    let pose = parent * part.matrix();
    for c in &part.cubes {
        cube_faces(c, pose, gui, size, lights, out);
    }
    for child in &part.children {
        walk(child, pose, gui, size, lights, out);
    }
}

/// An item's icon from its special model renderer, if its GUI model is
/// one (`None` for every other item).
pub fn special_icon(
    packs: &PackStack,
    key: &str,
    icon_size: usize,
    components: Option<&Value>,
) -> Result<Option<RgbaImage>> {
    let id = ResourceId::parse(key)?;
    let Some(definition) = packs.item_definition(&id)? else {
        return Ok(None);
    };
    let Some((node, local)) = gui_node(&definition["model"], Mat4::IDENTITY) else {
        return Ok(None);
    };
    let Some(layers) = layers(&node["model"], components)? else {
        return Ok(None);
    };
    let (display, flat) = display(packs, node["base"].as_str().unwrap_or("minecraft:item/generated"))?;
    let lights = item_lights(flat);
    // `GuiItemAtlas`: the slot's middle, 16 pixels to a block, y down.
    let pixels = icon_size as f32 / 16.0;
    let gui = Mat4::from_scale(Vec3::splat(pixels))
        * Mat4::from_translation(Vec3::new(8.0, 8.0, 0.0))
        * Mat4::from_scale(Vec3::new(16.0, -16.0, 16.0))
        * display
        * local;
    let mut output = RgbaImage::new(icon_size as u32, icon_size as u32);
    let mut depth = vec![f32::NEG_INFINITY; icon_size * icon_size];
    for layer in layers {
        let Some(bytes) = packs.texture(&ResourceId::parse(&layer.texture)?)? else {
            continue;
        };
        let texture = image::load_from_memory(&bytes)?.to_rgba8();
        let mut faces = Vec::new();
        for part in &layer.parts {
            walk(part, Mat4::IDENTITY, gui, layer.size, lights, &mut faces);
        }
        for face in faces {
            for [a, b, c] in [[0, 1, 2], [0, 2, 3]] {
                raster(
                    &mut output,
                    &mut depth,
                    &texture,
                    &layer,
                    face.shade,
                    [face.corners[a], face.corners[b], face.corners[c]],
                    [face.uvs[a], face.uvs[b], face.uvs[c]],
                );
            }
        }
    }
    Ok(output.pixels().any(|p| p[3] > 0).then_some(output))
}

/// The sheets special model renderers draw items held in a hand with, for
/// the world atlas.
pub const HAND_SHEETS: &[&str] = &[
    "minecraft:entity/shield/shield_base_nopattern",
    "minecraft:entity/trident/trident",
    "minecraft:entity/chest/normal",
    "minecraft:entity/chest/trapped",
    "minecraft:entity/chest/ender",
    "minecraft:entity/chest/christmas",
    "minecraft:entity/chest/copper",
    "minecraft:entity/chest/copper_exposed",
    "minecraft:entity/chest/copper_weathered",
    "minecraft:entity/chest/copper_oxidized",
    "minecraft:entity/conduit/base",
    "minecraft:entity/decorated_pot/decorated_pot_base",
    "minecraft:entity/decorated_pot/decorated_pot_side",
    "minecraft:entity/banner/banner_base",
    "minecraft:entity/banner/base",
    "minecraft:entity/skeleton/wither_skeleton",
    "minecraft:entity/piglin/piglin",
    "minecraft:entity/enderdragon/dragon",
    "minecraft:entity/shulker/shulker",
    "minecraft:entity/shulker/shulker_white",
    "minecraft:entity/shulker/shulker_orange",
    "minecraft:entity/shulker/shulker_magenta",
    "minecraft:entity/shulker/shulker_light_blue",
    "minecraft:entity/shulker/shulker_yellow",
    "minecraft:entity/shulker/shulker_lime",
    "minecraft:entity/shulker/shulker_pink",
    "minecraft:entity/shulker/shulker_gray",
    "minecraft:entity/shulker/shulker_light_gray",
    "minecraft:entity/shulker/shulker_cyan",
    "minecraft:entity/shulker/shulker_purple",
    "minecraft:entity/shulker/shulker_blue",
    "minecraft:entity/shulker/shulker_brown",
    "minecraft:entity/shulker/shulker_green",
    "minecraft:entity/shulker/shulker_red",
    "minecraft:entity/shulker/shulker_black",
    "minecraft:entity/copper_golem/copper_golem",
    "minecraft:entity/copper_golem/copper_golem_exposed",
    "minecraft:entity/copper_golem/copper_golem_weathered",
    "minecraft:entity/copper_golem/copper_golem_oxidized",
];

/// An item a special model renderer draws, held in a hand (in third
/// person `ItemInHandLayer`'s `THIRD_PERSON_RIGHT_HAND` or `_LEFT_HAND`,
/// in first person `ItemInHandRenderer`'s `FIRST_PERSON_*`): its
/// cuboids under `pose` (item space to the target's), the base model's
/// display transform for that hand and the item's transformations,
/// textured from the atlas, full bright and shaded by `shade` from each
/// face's normal under the pose. False when the item's model for that
/// hand is not a special one.
#[allow(clippy::too_many_arguments)]
pub fn append_special_in_hand(
    mesh: &mut ChunkMesh,
    packs: &PackStack,
    atlas: &Atlas,
    key: &str,
    components: Option<&Value>,
    pose: Mat4,
    left: bool,
    first_person: bool,
    shade: &dyn Fn(Vec3) -> f32,
) -> Result<bool> {
    let id = ResourceId::parse(key)?;
    let Some(definition) = packs.item_definition(&id)? else {
        return Ok(false);
    };
    let person = if first_person { "firstperson" } else { "thirdperson" };
    let context = format!("{person}_{}hand", if left { "left" } else { "right" });
    let Some((node, local)) = node_for(&definition["model"], &context, Mat4::IDENTITY) else {
        return Ok(false);
    };
    let Some(layers) = layers(&node["model"], components)? else {
        return Ok(false);
    };
    let base = node["base"].as_str().unwrap_or("minecraft:item/generated");
    let right = format!("{person}_righthand");
    let (display, _) = display_for(packs, base, &context, left.then_some(right.as_str()))?;
    let root = pose * display * local;
    for layer in layers.iter().filter(|layer| !layer.overlay) {
        let sheet = ResourceId::parse(&layer.texture)?;
        if !atlas.contains(&sheet) {
            continue;
        }
        let region = atlas.entity_region(&sheet);
        let tint = [(layer.tint >> 16) & 255, (layer.tint >> 8) & 255, layer.tint & 255].map(|c| c as f32 / 255.0);
        for part in &layer.parts {
            emit_part(mesh, part, root, region, layer.size, tint, shade, [15.0; 2]);
        }
    }
    Ok(true)
}

/// A part's cuboids and its children's under `parent` (`ModelPart.render`),
/// textured from `region` of the atlas, tinted, shaded by `shade` from each
/// face's normal under the pose, and lit by `light` (sky, block).
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_part(
    mesh: &mut ChunkMesh,
    part: &Part,
    parent: Mat4,
    region: [f32; 4],
    size: [f32; 2],
    tint: [f32; 3],
    shade: &dyn Fn(Vec3) -> f32,
    light: [f32; 2],
) {
    let pose = parent * part.matrix();
    let normals = Mat3::from_mat4(pose).inverse().transpose();
    for c in &part.cubes {
        let [x, y, z] = c.from;
        let [w, h, d] = c.size;
        let (mut x0, y0, z0) = (x - c.grow, y - c.grow, z - c.grow);
        let (mut x1, y1, z1) = (x + w + c.grow, y + h + c.grow, z + d + c.grow);
        if c.mirror {
            std::mem::swap(&mut x0, &mut x1);
        }
        let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) / 16.0;
        let (t0, t1, t2, t3) = (p(x0, y0, z0), p(x1, y0, z0), p(x1, y1, z0), p(x0, y1, z0));
        let (l0, l1, l2, l3) = (p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1));
        let [u, v] = c.uv;
        let (u0, u1, u2, u22, u3, u4) = (u, u + d, u + d + w, u + d + w + w, u + d + w + d, u + d + w + d + w);
        let (v0, v1, v2) = (v, v + d, v + d + h);
        let polygons = [
            ([l1, l0, t0, t1], [u1, v0, u2, v1], 0, Vec3::NEG_Y),
            ([t2, t3, l3, l2], [u2, v1, u22, v0], 1, Vec3::Y),
            ([t0, l0, l3, t3], [u0, v1, u1, v2], 4, Vec3::NEG_X),
            ([t1, t0, t3, t2], [u1, v1, u2, v2], 2, Vec3::NEG_Z),
            ([l1, t1, t2, l2], [u2, v1, u3, v2], 5, Vec3::X),
            ([l0, l1, l2, l3], [u3, v1, u4, v2], 3, Vec3::Z),
        ];
        for (corners, [ua, va, ub, vb], direction, normal) in polygons {
            if c.faces & (1 << direction) == 0 {
                continue;
            }
            let normal = if c.mirror { normal * Vec3::new(-1.0, 1.0, 1.0) } else { normal };
            let brightness = shade((normals * normal).normalize_or_zero());
            let start = mesh.vertices.len() as u32;
            for (corner, [s, t]) in corners.into_iter().zip([[ub, va], [ua, va], [ua, vb], [ub, vb]]) {
                mesh.vertices.push(Vertex {
                    position: pose.transform_point3(corner).to_array(),
                    uv: [
                        region[0] + (region[2] - region[0]) * s / size[0],
                        region[1] + (region[3] - region[1]) * t / size[1],
                    ],
                    color: [tint[0] * brightness, tint[1] * brightness, tint[2] * brightness, 1.0],
                    sky_light: light[0],
                    block_light: light[1],
                });
            }
            mesh.indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
            mesh.faces += 1;
        }
    }
    for child in &part.children {
        emit_part(mesh, child, pose, region, size, tint, shade, light);
    }
}

fn raster(
    output: &mut RgbaImage,
    depth: &mut [f32],
    texture: &RgbaImage,
    layer: &Layer,
    shade: f32,
    points: [Vec3; 3],
    uvs: [[f32; 2]; 3],
) {
    let size = output.width() as usize;
    let edge = |a: Vec3, b: Vec3, x: f32, y: f32| (x - a.x) * (b.y - a.y) - (y - a.y) * (b.x - a.x);
    let area = edge(points[0], points[1], points[2].x, points[2].y);
    if area.abs() < 1e-4 {
        return;
    }
    let min_x = points[0].x.min(points[1].x).min(points[2].x);
    let max_x = points[0].x.max(points[1].x).max(points[2].x);
    let min_y = points[0].y.min(points[1].y).min(points[2].y);
    let max_y = points[0].y.max(points[1].y).max(points[2].y);
    let (tw, th) = (texture.width(), texture.height());
    let tint = [(layer.tint >> 16) & 255, (layer.tint >> 8) & 255, layer.tint & 255].map(|c| c as f32 / 255.0);
    for y in (min_y.floor().max(0.0) as usize)..(max_y.ceil().min(size as f32) as usize) {
        for x in (min_x.floor().max(0.0) as usize)..(max_x.ceil().min(size as f32) as usize) {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let a = edge(points[1], points[2], px, py) / area;
            let b = edge(points[2], points[0], px, py) / area;
            let c = 1.0 - a - b;
            if a < -1e-4 || b < -1e-4 || c < -1e-4 {
                continue;
            }
            let z = a * points[0].z + b * points[1].z + c * points[2].z;
            let at = y * size + x;
            if layer.overlay {
                if z < depth[at] - 1e-3 {
                    continue;
                }
            } else if z < depth[at] {
                continue;
            }
            let u = a * uvs[0][0] + b * uvs[1][0] + c * uvs[2][0];
            let v = a * uvs[0][1] + b * uvs[1][1] + c * uvs[2][1];
            let texel = texture.get_pixel(
                ((u * tw as f32).floor().max(0.0) as u32).min(tw - 1),
                ((v * th as f32).floor().max(0.0) as u32).min(th - 1),
            );
            // Cutout below one tenth.
            if texel[3] < 26 {
                continue;
            }
            let color: [f32; 3] = std::array::from_fn(|i| texel[i] as f32 * shade * tint[i]);
            let pixel = output.get_pixel_mut(x as u32, y as u32);
            if layer.overlay {
                let alpha = texel[3] as f32 / 255.0;
                for i in 0..3 {
                    pixel[i] = (color[i] * alpha + pixel[i] as f32 * (1.0 - alpha)).round() as u8;
                }
                pixel[3] = pixel[3].max(texel[3]);
            } else {
                for i in 0..3 {
                    pixel[i] = color[i].round().min(255.0) as u8;
                }
                pixel[3] = 255;
                depth[at] = z;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With `ICON_PACK` set to a resource pack and `ICON_SHEET` to a PNG
    /// path, draws the special items' icons side by side for a look.
    #[test]
    #[ignore]
    fn contact_sheet() {
        let (Ok(pack), Ok(sheet)) = (std::env::var("ICON_PACK"), std::env::var("ICON_SHEET")) else {
            return;
        };
        let packs = PackStack::open(vec![pack.into()]).unwrap();
        let items = [
            "chest", "trapped_chest", "ender_chest", "copper_chest", "white_shulker_box", "shulker_box",
            "red_banner", "black_banner", "shield", "conduit", "decorated_pot", "skeleton_skull",
            "wither_skeleton_skull", "zombie_head", "player_head", "creeper_head", "piglin_head", "dragon_head",
            "copper_golem_statue", "exposed_copper_golem_statue", "weathered_copper_golem_statue",
            "oxidized_copper_golem_statue",
        ];
        let size: u32 = std::env::var("ICON_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(64);
        let mut out = RgbaImage::from_pixel(size * items.len() as u32, size, image::Rgba([139, 139, 139, 255]));
        for (i, item) in items.iter().enumerate() {
            if let Some(icon) = special_icon(&packs, &format!("minecraft:{item}"), size as usize, None).unwrap() {
                image::imageops::overlay(&mut out, &icon, i64::from(i as u32 * size), 0);
            } else {
                eprintln!("no icon for {item}");
            }
        }
        out.save(sheet).unwrap();
    }
}
