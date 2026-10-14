//! Blocks' outline shapes and the random offsets of their models, as 26.3
//! has them: `BlockState.getShape` with an empty context, as boxes
//! (`VoxelShape.toAabbs`, which the crosshair's ray meets) and as the edges
//! `forAllEdges` walks (the outline drawn around the targeted block), and
//! `BlockBehaviour.Properties.offsetType` (flowers and grass shifted about
//! their cell). The game installs the table it ships
//! (`block-outline-26.3.json`, made by `minecraft/tools/DumpShapes.java`);
//! without it every block keeps a full cube and no offset.
use crate::scene::BlockPos;
use glam::Vec3;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One outline shape: its boxes and its edges, in block units.
#[derive(Clone, Debug, Default)]
pub struct Shape {
    pub boxes: Vec<[f64; 6]>,
    pub edges: Vec<[f32; 6]>,
}

/// `OffsetType.XZ` or `XYZ`, with the block's `getMaxHorizontalOffset` and
/// `getMaxVerticalOffset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offset {
    pub vertical: bool,
    pub max_horizontal: f32,
    pub max_vertical: f32,
}

pub struct Outlines {
    shapes: Vec<Shape>,
    /// Each block state's shape, by state ID.
    states: Vec<u16>,
    offsets: HashMap<String, Offset>,
}

static OUTLINES: OnceLock<Outlines> = OnceLock::new();

/// Installs the table (its JSON); a second call keeps the first.
pub fn install(json: &[u8]) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    let list = |v: &serde_json::Value| -> Vec<[f64; 6]> {
        v.as_array().into_iter().flatten().filter_map(|b| {
            let b = b.as_array()?;
            let mut out = [0.0; 6];
            for (slot, n) in out.iter_mut().zip(b) {
                *slot = n.as_f64()?;
            }
            Some(out)
        }).collect()
    };
    let shapes = value["shapes"].as_array().ok_or("no shapes")?.iter().map(|s| Shape {
        boxes: list(&s["boxes"]),
        edges: list(&s["edges"]).into_iter().map(|e| e.map(|v| v as f32)).collect(),
    }).collect();
    let states = value["states"].as_array().ok_or("no states")?.iter().map(|i| i.as_u64().unwrap_or(1) as u16).collect();
    // Keyed by path: they are all `minecraft:`.
    let offsets = value["offsets"].as_object().ok_or("no offsets")?.iter().filter_map(|(block, o)| {
        Some((block.trim_start_matches("minecraft:").to_owned(), Offset { vertical: o[0].as_str()? == "xyz", max_horizontal: o[1].as_f64()? as f32, max_vertical: o[2].as_f64()? as f32 }))
    }).collect();
    let _ = OUTLINES.set(Outlines { shapes, states, offsets });
    Ok(())
}

/// The installed table.
pub fn outlines() -> Option<&'static Outlines> {
    OUTLINES.get()
}

impl Outlines {
    /// A block state's outline, by state ID.
    pub fn shape(&self, state: u32) -> Option<&Shape> {
        self.shapes.get(usize::from(*self.states.get(state as usize)?))
    }
}

/// `BlockState.getOffset` for a block by its path: one with an offset
/// type shifts by its column's seed (`Mth.getSeed(x, 0, z)`), up to its
/// limits.
pub fn model_offset(block: &str, (x, _, z): BlockPos) -> Vec3 {
    let Some(offset) = outlines().and_then(|o| o.offsets.get(block)) else { return Vec3::ZERO };
    let seed = get_seed(x, 0, z);
    let max = f64::from(offset.max_horizontal);
    let horizontal = |bits: i64| ((f64::from((bits & 15) as f32 / 15.0) - 0.5) * 0.5).clamp(-max, max);
    let y = if offset.vertical { (f64::from((seed >> 4 & 15) as f32 / 15.0) - 1.0) * f64::from(offset.max_vertical) } else { 0.0 };
    Vec3::new(horizontal(seed) as f32, y as f32, horizontal(seed >> 8) as f32)
}

/// `Mth.getSeed`, with its int multiplication.
fn get_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut seed = i64::from(x.wrapping_mul(3_129_871)) ^ i64::from(z).wrapping_mul(116_129_781) ^ i64::from(y);
    seed = seed.wrapping_mul(seed).wrapping_mul(42_317_861).wrapping_add(seed.wrapping_mul(11));
    seed >> 16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_as_mth_does() {
        // `Mth.getSeed(1, 0, 1)` and `(-5, 0, 12)` from the 26.3 JAR.
        assert_eq!(get_seed(0, 0, 0), 0);
        assert_eq!(get_seed(1, 0, 1), 60_165_349_410_229);
        assert_eq!(get_seed(-5, 0, 12), -124_070_260_134_363);
    }
}
