//! What the crosshair is on: the first block along the look whose shape the
//! ray meets. Blocks with collision are hit by their exact collision boxes;
//! those without (plants, torches, rails, buttons) by an outline like
//! vanilla's, so they can be broken and used too. Fluids are passed through.
use glam::DVec3;
use minecraft_terrain::scene::{Block, BlockPos};
use minecraftoss_player::{Face, Hit};

use crate::world::World;

/// The block hit within `reach` of `eye` along `look`.
pub fn target(world: &World, eye: DVec3, look: DVec3, reach: f64) -> Option<Hit> {
    let dir = look.normalize_or_zero();
    if dir == DVec3::ZERO {
        return None;
    }
    let mut cell = [
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    ];
    let step = [
        dir.x.signum() as i32,
        dir.y.signum() as i32,
        dir.z.signum() as i32,
    ];
    let delta = [1.0 / dir.x.abs(), 1.0 / dir.y.abs(), 1.0 / dir.z.abs()];
    // Distance along the ray to the first boundary on each axis; an axis
    // the ray runs parallel to is never crossed.
    let boundary = |origin: f64, cell: i32, d: f64, delta: f64| {
        if d == 0.0 {
            f64::INFINITY
        } else if d > 0.0 {
            (f64::from(cell) + 1.0 - origin) * delta
        } else {
            (origin - f64::from(cell)) * delta
        }
    };
    let mut next = [
        boundary(eye.x, cell[0], dir.x, delta[0]),
        boundary(eye.y, cell[1], dir.y, delta[1]),
        boundary(eye.z, cell[2], dir.z, delta[2]),
    ];
    let mut entered = 0.0;
    while entered <= reach {
        let pos = (cell[0], cell[1], cell[2]);
        if let Some(block) = world.block(pos)
            && let Some(hit) = hit_block(world, pos, block, eye, dir, reach)
        {
            return Some(hit);
        }
        let axis = if next[0] <= next[1] && next[0] <= next[2] {
            0
        } else if next[1] <= next[2] {
            1
        } else {
            2
        };
        entered = next[axis];
        cell[axis] += step[axis];
        next[axis] += delta[axis];
    }
    None
}

fn hit_block(
    world: &World,
    pos: BlockPos,
    block: &Block,
    eye: DVec3,
    dir: DVec3,
    reach: f64,
) -> Option<Hit> {
    let origin = DVec3::new(f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
    let mut best: Option<(f64, Face)> = None;
    for b in shape(world, pos, block) {
        // A fence's taller collision still outlines as one block.
        let (min, max) = (
            origin + DVec3::new(b[0], b[1], b[2]),
            origin + DVec3::new(b[3], b[4].min(1.0), b[5]),
        );
        if let Some((distance, face)) = ray_box(eye, dir, min, max)
            && distance <= reach
            && best.is_none_or(|(d, _)| distance < d)
        {
            best = Some((distance, face));
        }
    }
    best.map(|(distance, face)| Hit {
        pos,
        face,
        distance,
        point: eye + dir * distance,
    })
}

/// The boxes the ray is tested against, in block-local units.
fn shape(world: &World, pos: BlockPos, block: &Block) -> Vec<[f64; 6]> {
    let path = block.id.path.as_str();
    if matches!(
        path,
        "water"
            | "lava"
            | "air"
            | "cave_air"
            | "void_air"
            | "bubble_column"
            | "light"
            | "structure_void"
    ) {
        return Vec::new();
    }
    let boxes = world.collision_boxes(pos);
    if !boxes.is_empty() {
        return boxes;
    }
    let px = |x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64| {
        vec![[
            x0 / 16.0,
            y0 / 16.0,
            z0 / 16.0,
            x1 / 16.0,
            y1 / 16.0,
            z1 / 16.0,
        ]]
    };
    let facing = block.properties.get("facing").map(String::as_str);
    let on_wall = path.contains("wall_")
        || block.properties.get("face").is_some_and(|f| f == "wall")
        || matches!(path, "vine" | "glow_lichen");
    if on_wall && let Some(facing) = facing {
        // Hugs the block it hangs on, which is behind its facing.
        return match facing {
            "north" => px(3.0, 3.0, 11.0, 13.0, 13.0, 16.0),
            "south" => px(3.0, 3.0, 0.0, 13.0, 13.0, 5.0),
            "east" => px(0.0, 3.0, 3.0, 5.0, 13.0, 13.0),
            _ => px(11.0, 3.0, 3.0, 16.0, 13.0, 13.0),
        };
    }
    if path.ends_with("torch") {
        return px(6.0, 0.0, 6.0, 10.0, 10.0, 10.0);
    }
    if path.ends_with("rail") || path == "redstone_wire" || path == "tripwire" {
        return px(0.0, 0.0, 0.0, 16.0, 2.0, 16.0);
    }
    if path.ends_with("_pressure_plate") {
        return px(1.0, 0.0, 1.0, 15.0, 1.0, 15.0);
    }
    if matches!(path, "leaf_litter" | "pink_petals" | "wildflowers" | "snow") {
        return px(0.0, 0.0, 0.0, 16.0, 3.0, 16.0);
    }
    if path.ends_with("_button") || path == "lever" {
        let ceiling = block.properties.get("face").is_some_and(|f| f == "ceiling");
        return if ceiling {
            px(4.0, 13.0, 4.0, 12.0, 16.0, 12.0)
        } else {
            px(4.0, 0.0, 4.0, 12.0, 3.0, 12.0)
        };
    }
    if path.ends_with("_sign")
        || path == "cobweb"
        || path.contains("portal")
        || path.ends_with("fire")
    {
        return px(0.0, 0.0, 0.0, 16.0, 16.0, 16.0);
    }
    // Grass, flowers, saplings, crops and the other plants.
    px(2.0, 0.0, 2.0, 14.0, 13.0, 14.0)
}

/// Where a ray enters a box, and through which face.
fn ray_box(origin: DVec3, dir: DVec3, min: DVec3, max: DVec3) -> Option<(f64, Face)> {
    let mut near = f64::NEG_INFINITY;
    let mut far = f64::INFINITY;
    let mut face = Face::Up;
    for (o, d, lo, hi, negative, positive) in [
        (origin.x, dir.x, min.x, max.x, Face::West, Face::East),
        (origin.y, dir.y, min.y, max.y, Face::Down, Face::Up),
        (origin.z, dir.z, min.z, max.z, Face::North, Face::South),
    ] {
        if d == 0.0 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((lo - o) / d, (hi - o) / d);
        let (enter, exit, entered) = if t0 < t1 {
            (t0, t1, negative)
        } else {
            (t1, t0, positive)
        };
        if enter > near {
            near = enter;
            face = entered;
        }
        far = far.min(exit);
        if near > far {
            return None;
        }
    }
    (far >= 0.0).then_some((near.max(0.0), face))
}
