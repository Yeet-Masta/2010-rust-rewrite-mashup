//! `CartographyTableMenu` (26.3): a filled map and paper, an empty map or a
//! glass pane, and the result: the map to zoom out (`map_post_processing`
//! scale, one), to lock (lock, one), or two of it. Taking it uses one of
//! each input, and the map taken is post-processed (`onCraftedBy`): it
//! loses the component, and on a server becomes a new map, scaled or
//! locked.
//!
//! Only a server's menu ([`CartographyTableMenu::set_at_block`]) makes the
//! result, from the level's map data, as vanilla's client leaves it to the
//! server; it also plays the take's sound once a tick and returns the
//! inputs as it closes. There is no map system yet: the server's maps are
//! a [`MapRegistry`] that takes a map it has not seen as a new one, and its
//! screen is sent the data of the map in the table.
use super::{
    Menu, MenuContext, OwnSlots, SlotDef, clear_own_slots, item, move_item_stack_to, put_back,
    remove_from, return_carried, standard_inventory_slots,
};
use crate::inventory::ItemStack;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The map's and the additional item's menu slots.
pub const MAP: usize = 0;
pub const ADDITIONAL: usize = 1;
/// The result's menu slot.
pub const RESULT: usize = 2;
/// The main inventory is 3-29, the hotbar 30-38.
const INVENTORY: usize = 3;
const HOTBAR: usize = 30;
const END: usize = 39;
/// `SoundEvents.UI_CARTOGRAPHY_TABLE_TAKE_RESULT`.
const TAKE_SOUND: &str = "minecraft:ui.cartography_table.take_result";
/// `DataComponents.MAP_ID`.
pub const MAP_ID: &str = "minecraft:map_id";
/// `DataComponents.MAP_POST_PROCESSING`, by its `MapPostProcessing` id.
pub const MAP_POST_PROCESSING: &str = "minecraft:map_post_processing";
/// `MapPostProcessing.LOCK` and `SCALE`.
pub const LOCK: i64 = 0;
pub const SCALE: i64 = 1;
/// `MapItemSavedData.MAX_SCALE`.
pub const MAX_SCALE: u8 = 4;

/// What the table reads of a map's `MapItemSavedData`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MapInfo {
    pub scale: u8,
    pub locked: bool,
}

/// The level's map data as far as the table goes (`MapItem.getSavedData`,
/// `getFreeMapId`, `setMapData`), for the session.
#[derive(Debug, Default)]
pub struct MapRegistry {
    maps: HashMap<i64, MapInfo>,
    /// The next free id: past every id seen.
    next: i64,
}

impl MapRegistry {
    /// `getSavedData`: a map the registry has not seen is taken as a new
    /// one, at scale 0 and unlocked, as there is no map system to know it.
    pub fn data(&mut self, id: i64) -> MapInfo {
        self.next = self.next.max(id + 1);
        self.maps.get(&id).copied().unwrap_or_else(MapInfo::default)
    }

    /// `getFreeMapId` and `setMapData`: a new map; its id.
    pub fn add(&mut self, info: MapInfo) -> i64 {
        let id = self.next.max(0);
        self.next = id + 1;
        self.maps.insert(id, info);
        id
    }
}

/// A stack's `map_id`.
pub fn map_id(stack: &ItemStack) -> Option<i64> {
    stack.components.as_ref()?.get(MAP_ID)?.as_i64()
}

/// The additional slot's items: paper, an empty map or a glass pane.
fn is_additional(stack: &ItemStack) -> bool {
    matches!(
        stack.id.as_str(),
        "minecraft:paper" | "minecraft:map" | "minecraft:glass_pane"
    )
}

/// `CartographyTableMenu` (`cartography_table`).
#[derive(Debug)]
pub struct CartographyTableMenu {
    slots: Vec<SlotDef>,
    /// The map, the additional item, then the result.
    items: OwnSlots,
    /// The menu is a server's, at its block.
    at_block: bool,
    /// The server's maps.
    maps: Option<Arc<Mutex<MapRegistry>>>,
    /// On a client, the data of the map in the table, as the server sent it.
    shown: Option<MapInfo>,
    /// `lastSoundTime`.
    last_sound_time: i64,
}

impl CartographyTableMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![
            SlotDef::own(MAP, 15, 15),
            SlotDef::own(ADDITIONAL, 15, 52),
            SlotDef::own(RESULT, 145, 39),
        ];
        slots.extend(standard_inventory_slots(8, 84));
        items.resize(3, None);
        Self {
            slots,
            items: OwnSlots::new(items),
            at_block: false,
            maps: None,
            shown: None,
            last_sound_time: i64::MIN,
        }
    }

    /// The menu is a server's, at its block (`ContainerLevelAccess.create`).
    pub fn set_at_block(&mut self, at_block: bool) {
        self.at_block = at_block;
    }

    /// The server's maps, which make the results.
    pub fn set_maps(&mut self, maps: Option<Arc<Mutex<MapRegistry>>>) {
        self.maps = maps;
    }

    /// The data of the map in the table (`MapItem.getSavedData`): the
    /// server's, or on a client what the server sent.
    pub fn map_info(&self) -> Option<MapInfo> {
        let id = self.items.get(MAP).and_then(map_id)?;
        match &self.maps {
            Some(maps) => maps.lock().ok().map(|mut maps| maps.data(id)),
            None => self.shown,
        }
    }

    /// A client's copy takes the server's [`CartographyTableMenu::map_info`].
    pub fn set_map_info(&mut self, info: Option<MapInfo>) {
        self.shown = info;
    }

    /// `setupResultSlot`, on a server: what the inputs make of the map, by
    /// its data.
    fn setup_result(&mut self, cx: &MenuContext) {
        let (Some(map), Some(additional)) = (self.items.get(MAP), self.items.get(ADDITIONAL)) else {
            return;
        };
        let Some(data) = self.map_info().filter(|_| self.at_block) else {
            return;
        };
        let extendable = cx
            .inventory
            .recipes
            .item_in_tag("minecraft:extendable_maps", &map.id);
        let processed = |processing: i64| {
            let mut result = ItemStack {
                count: 1,
                ..map.clone()
            };
            if let Some(Value::Object(components)) = result.components.as_mut() {
                components.insert(MAP_POST_PROCESSING.to_owned(), processing.into());
            }
            result
        };
        let result = match additional.id.as_str() {
            "minecraft:paper" if extendable && !data.locked && data.scale < MAX_SCALE => {
                processed(SCALE)
            }
            "minecraft:glass_pane" if !data.locked => processed(LOCK),
            "minecraft:map" => ItemStack {
                count: 2,
                ..map.clone()
            },
            // `removeItemNoUpdate`.
            _ => {
                self.items.set(RESULT, None);
                return;
            }
        };
        self.items.set(RESULT, Some(result));
    }

    /// `MapItem.onCraftedPostProcess` on a map taken from the result: the
    /// component goes, and on a server the map becomes a new one, its data
    /// scaled (`scaleMap`) or locked (`lockMap`).
    fn post_process(&self, stack: &mut ItemStack) {
        let Some(Value::Object(components)) = stack.components.as_mut() else {
            return;
        };
        let Some(processing) = components.remove(MAP_POST_PROCESSING) else {
            return;
        };
        let (Some(maps), Some(id)) = (self.maps.as_ref(), components.get(MAP_ID).and_then(Value::as_i64)) else {
            return;
        };
        let Ok(mut maps) = maps.lock() else {
            return;
        };
        let mut data = maps.data(id);
        match processing.as_i64() {
            Some(LOCK) => data.locked = true,
            Some(SCALE) => data.scale = (data.scale + 1).min(MAX_SCALE),
            _ => return,
        }
        components.insert(MAP_ID.to_owned(), maps.add(data).into());
    }
}

impl Menu for CartographyTableMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:cartography_table"
    }

    fn slots(&self) -> &[SlotDef] {
        &self.slots
    }

    fn own(&self) -> &OwnSlots {
        &self.items
    }

    fn own_mut(&mut self) -> &mut OwnSlots {
        &mut self.items
    }

    fn may_place(&self, _cx: &MenuContext, slot: usize, stack: &ItemStack) -> bool {
        match slot {
            MAP => map_id(stack).is_some(),
            ADDITIONAL => is_additional(stack),
            RESULT => false,
            _ => true,
        }
    }

    /// `CartographyTableMenu.quickMoveStack`: the result, post-processed,
    /// into the inventory, last first; an input into the inventory; a map
    /// into the map's slot, paper, an empty map or glass into the other;
    /// otherwise between the main inventory and the hotbar.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let moved = match slot {
            RESULT => {
                self.post_process(&mut stack);
                move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true)
            }
            MAP | ADDITIONAL => move_item_stack_to(self, cx, &mut stack, INVENTORY, END, false),
            _ if map_id(&stack).is_some() => move_item_stack_to(self, cx, &mut stack, MAP, ADDITIONAL, false),
            _ if is_additional(&stack) => move_item_stack_to(self, cx, &mut stack, ADDITIONAL, RESULT, false),
            INVENTORY..HOTBAR => move_item_stack_to(self, cx, &mut stack, HOTBAR, END, false),
            HOTBAR..END => move_item_stack_to(self, cx, &mut stack, INVENTORY, HOTBAR, false),
            _ => true,
        };
        if !moved {
            return None;
        }
        let left = stack.clone();
        put_back(self, cx, slot, stack);
        if left.count == clicked.count {
            return None;
        }
        self.on_take(cx, slot, &left);
        Some(clicked)
    }

    /// `ResultContainer.removeItem`: the whole result, which the take
    /// post-processes (`onCraftedBy`).
    fn remove(&mut self, cx: &mut MenuContext, slot: usize, amount: i32) -> Option<ItemStack> {
        if slot != RESULT {
            return remove_from(self, cx, slot, amount);
        }
        let mut result = self.items.get(RESULT).cloned()?;
        self.items.set(RESULT, None);
        self.post_process(&mut result);
        Some(result)
    }

    /// A hotbar key took the result: the stack it put in the inventory is
    /// post-processed there, as `onTake` does to the slot's own stack.
    fn on_swap_craft(&mut self, cx: &mut MenuContext, slot: usize, _count: i32) {
        let Some(result) = self.items.get(RESULT).cloned().filter(|_| slot == RESULT) else {
            return;
        };
        let holding: Vec<usize> = cx
            .player_slots_written()
            .filter(|&index| cx.inventory.slots.get(index).and_then(Option::as_ref) == Some(&result))
            .collect();
        for index in holding {
            let mut stack = result.clone();
            self.post_process(&mut stack);
            cx.set_player(index, Some(stack));
        }
    }

    /// The result slot's `onTake`: one of each input goes; at the block,
    /// its sound, once a tick.
    fn on_take(&mut self, cx: &mut MenuContext, slot: usize, _taken: &ItemStack) {
        if slot != RESULT {
            super::set_changed(self, cx, slot);
            return;
        }
        remove_from(self, cx, MAP, 1);
        remove_from(self, cx, ADDITIONAL, 1);
        if self.at_block && self.last_sound_time != cx.game_time {
            cx.sounds.push((TAKE_SOUND, 1.0, 1.0));
            self.last_sound_time = cx.game_time;
        }
        super::set_changed(self, cx, slot);
    }

    /// `slotsChanged`: with both inputs, the result again; a result left
    /// without them goes.
    fn slots_changed(&mut self, cx: &mut MenuContext, _slot: usize) {
        let inputs = self.items.get(MAP).is_some() && self.items.get(ADDITIONAL).is_some();
        if inputs {
            self.setup_result(cx);
        } else if self.items.get(RESULT).is_some() {
            self.items.set(RESULT, None);
        }
    }

    /// `removed`: the result goes, then on a server the inputs come back
    /// (`clearContainer`).
    fn removed(&mut self, cx: &mut MenuContext) {
        return_carried(cx);
        self.items.set(RESULT, None);
        if self.at_block {
            clear_own_slots(self, cx, MAP..RESULT);
        }
    }

    /// The result is not gathered by a double click.
    fn can_take_for_pick_all(&self, _carried: &ItemStack, slot: usize) -> bool {
        slot != RESULT
    }

    fn own_container(&self, slot: usize) -> usize {
        usize::from(slot == RESULT)
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ContainerInput, MenuInput, handle, set};
    use super::*;
    use crate::crafting::RecipeBook;
    use crate::inventory::Inventory;
    use crate::rng::LegacyRandom;
    use serde_json::json;

    fn map(id: i64, count: u8) -> ItemStack {
        ItemStack {
            components: Some(json!({ MAP_ID: id })),
            ..ItemStack::new("minecraft:filled_map", count)
        }
    }

    #[test]
    fn paper_zooms_out_to_a_new_map_until_scale_four_and_glass_locks() {
        let mut inventory = Inventory::default();
        inventory.recipes = Arc::new(RecipeBook::from_files(vec![(
            "data/minecraft/tags/item/extendable_maps.json".to_owned(),
            json!({"values": ["minecraft:filled_map"]}),
        )]));
        let mut random = LegacyRandom::new(1);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        let maps = Arc::new(Mutex::new(MapRegistry::default()));
        let mut menu = CartographyTableMenu::new(Vec::new());
        menu.set_at_block(true);
        menu.set_maps(Some(maps.clone()));
        set(&mut menu, &mut cx, MAP, Some(map(7, 1)));
        set(&mut menu, &mut cx, ADDITIONAL, Some(ItemStack::new("minecraft:map", 3)));
        assert_eq!(menu.own().get(RESULT), Some(&map(7, 2)), "two of it");
        set(&mut menu, &mut cx, ADDITIONAL, Some(ItemStack::new("minecraft:paper", 9)));
        let result = menu.own().get(RESULT).unwrap();
        assert_eq!(result.components.as_ref().unwrap()[MAP_POST_PROCESSING], SCALE);
        // Each zoom takes a new id one scale up, until scale 4.
        let take = MenuInput::Click { slot: RESULT as i32, button: 0, kind: ContainerInput::Pickup };
        for scale in 1..=4 {
            handle(&mut menu, &mut cx, &take);
            let taken = cx.inventory.cursor.take().unwrap();
            assert_eq!(taken.components, Some(json!({ MAP_ID: 7 + scale })));
            assert_eq!(maps.lock().unwrap().data(7 + scale), MapInfo { scale: scale as u8, locked: false });
            set(&mut menu, &mut cx, MAP, Some(taken));
        }
        assert_eq!(menu.own().get(RESULT), None, "already at scale 4");
        assert_eq!(menu.own().get(ADDITIONAL).map(|paper| paper.count), Some(5));
        assert_eq!(cx.sounds.len(), 1, "one take sound a tick");
        // Glass locks it; a locked map takes no more glass.
        set(&mut menu, &mut cx, ADDITIONAL, Some(ItemStack::new("minecraft:glass_pane", 2)));
        handle(&mut menu, &mut cx, &take);
        let locked = cx.inventory.cursor.take().unwrap();
        assert!(maps.lock().unwrap().data(map_id(&locked).unwrap()).locked);
        set(&mut menu, &mut cx, MAP, Some(locked));
        assert_eq!(menu.own().get(RESULT), None);
    }
}
