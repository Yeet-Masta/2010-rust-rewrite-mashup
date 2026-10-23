//! The chests copper golems carry items between, as the level sees them
//! (26.3 `TransportItemsBetweenContainers.TransportItemTarget`): copper
//! chests to take from, chests and trapped chests to put into.
use super::container::{ContainerRef, Stack};
use super::Level;
use minecraftoss_core::BlockPos;

/// A chest a golem may visit, and what it can tell of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransportTarget {
    pub pos: (i32, i32, i32),
    /// In `#copper_chests` (a source), or a chest or trapped chest (a
    /// destination).
    pub copper: bool,
    pub chest: bool,
    /// Its other half, for a double chest.
    pub connected: Option<(i32, i32, i32)>,
    /// What it holds: whether nothing, and the items in it.
    pub empty: bool,
    pub items: Vec<String>,
    /// `isLocked`, `ChestBlock.isChestBlockedAt`, and someone has it open.
    pub locked: bool,
    pub blocked: bool,
    pub in_use: bool,
}

impl Level<'_> {
    fn transport_target(&self, pos: BlockPos) -> Option<TransportTarget> {
        let state = self.block(pos);
        if !self.is_a(state, "ChestBlock") || self.block_entity(pos).is_none() {
            return None;
        }
        let name = self.name(state);
        let copper = name.ends_with("copper_chest");
        let chest = matches!(name, "minecraft:chest" | "minecraft:trapped_chest");
        let container = self.container_at(pos, true)?;
        let items = self.container_items(container);
        let connected = match container {
            ContainerRef::Double(a, b) => Some(if a == pos { b } else { a }),
            ContainerRef::Single(..) => None,
        };
        let in_use = std::iter::once(pos).chain(connected).any(|p| self.opener_count(p) > 0);
        Some(TransportTarget {
            pos: (pos.x, pos.y, pos.z),
            copper,
            chest,
            connected: connected.map(|p| (p.x, p.y, p.z)),
            empty: items.iter().all(Stack::is_empty),
            items: items.iter().filter(|s| !s.is_empty()).map(|s| s.id.clone()).collect(),
            locked: self.block_entity(pos).and_then(|t| t.get("lock")).is_some(),
            blocked: self.chest_blocked(pos),
            in_use,
        })
    }

    /// One chest, if `pos` holds one.
    pub fn transport_target_at(&self, (x, y, z): (i32, i32, i32)) -> Option<TransportTarget> {
        self.transport_target(BlockPos::new(x, y, z))
    }

    /// `getTransportTarget`'s candidates: the chests in the loaded chunks
    /// about `center`, inside the box `horizontal` across and `vertical`
    /// up and down.
    pub fn transport_targets(&self, (x, y, z): (i32, i32, i32), horizontal: i32, vertical: i32) -> Vec<TransportTarget> {
        let reach = horizontal.div_euclid(16) + 1;
        let (cx, cz) = (x >> 4, z >> 4);
        let mut out = Vec::new();
        for chunk_x in cx - reach..=cx + reach {
            for chunk_z in cz - reach..=cz + reach {
                let Some(chunk) = self.chunk(minecraftoss_core::ChunkPos::new(chunk_x, chunk_z)) else { continue };
                for &(bx, by, bz) in chunk.block_entities.entities.keys() {
                    let inside = (bx - x).abs() <= horizontal && (by - y).abs() <= vertical && (bz - z).abs() <= horizontal;
                    if inside {
                        if let Some(t) = self.transport_target(BlockPos::new(bx, by, bz)) {
                            out.push(t);
                        }
                    }
                }
            }
        }
        out
    }

    /// `pickupItemFromContainer`: up to sixteen of the first stack in it.
    pub fn transport_take(&mut self, (x, y, z): (i32, i32, i32)) -> Option<Stack> {
        let pos = BlockPos::new(x, y, z);
        let container = self.container_at(pos, true)?;
        let items = self.container_items(container);
        let (slot, stack) = items.into_iter().enumerate().find(|(_, s)| !s.is_empty())?;
        let taken = stack.count.min(16);
        let mut left = stack.clone();
        left.count -= taken;
        let left = if left.count <= 0 { Stack::empty() } else { left };
        self.container_set_item(container, slot, left);
        self.container_set_changed(container);
        let mut out = stack;
        out.count = taken;
        Some(out)
    }

    /// `addItemsToContainer`: into the first empty slot, or onto stacks of
    /// the same item with room. What it could not put down.
    pub fn transport_put(&mut self, (x, y, z): (i32, i32, i32), mut stack: Stack) -> Stack {
        let pos = BlockPos::new(x, y, z);
        let Some(container) = self.container_at(pos, true) else { return stack };
        let items = self.container_items(container);
        for (slot, there) in items.into_iter().enumerate() {
            if there.is_empty() {
                self.container_set_item(container, slot, stack);
                self.container_set_changed(container);
                return Stack::empty();
            }
            let max = self.item_max_stack(&there);
            if there.same_item_same_components(&stack) && there.count < max {
                let room = max - there.count;
                let added = room.min(stack.count);
                let mut grown = there;
                grown.count += added;
                stack.count -= room;
                self.container_set_item(container, slot, grown);
                if stack.count <= 0 {
                    self.container_set_changed(container);
                    return Stack::empty();
                }
            }
        }
        self.container_set_changed(container);
        stack
    }
}
