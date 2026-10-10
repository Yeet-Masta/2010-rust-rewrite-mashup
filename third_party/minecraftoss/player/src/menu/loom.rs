//! `LoomMenu` (26.3): a banner, a dye and an optional pattern item, the
//! patterns they can weave (the banner pattern tag `#no_item_required`
//! without a pattern item, otherwise the item's `provides_banner_patterns`),
//! the one the screen's buttons select (the data value
//! `selectedBannerPatternIndex`), and the result: the banner with that
//! pattern added as a layer in the dye's colour, unless it already has six.
//! Taking it uses one banner and one dye; the pattern item stays.
//!
//! The patterns come from the recipe book's banner pattern registry on both
//! sides. A server's menu ([`LoomMenu::set_at_block`]) plays the take's
//! sound, once a tick, and returns the inputs as it closes.
use super::{
    Menu, MenuContext, OwnSlots, SlotDef, clear_own_slots, item, move_item_stack_to, put_back,
    remove_from, return_carried, standard_inventory_slots, storage::DYE_COLOURS,
};
use crate::inventory::ItemStack;
use serde_json::{Value, json};

/// The banner, dye and pattern item's menu slots.
pub const BANNER: usize = 0;
pub const DYE: usize = 1;
pub const PATTERN: usize = 2;
/// The result's menu slot.
pub const RESULT: usize = 3;
/// The main inventory is 4-30, the hotbar 31-39.
const INVENTORY: usize = 4;
const HOTBAR: usize = 31;
const END: usize = 40;
/// The most layers a banner takes from a loom.
pub const MAX_LAYERS: usize = 6;
/// `SoundEvents.UI_LOOM_TAKE_RESULT`.
const TAKE_SOUND: &str = "minecraft:ui.loom.take_result";
/// The component that holds a banner's layers.
pub const BANNER_PATTERNS: &str = "minecraft:banner_patterns";

/// `LoomMenu` (`loom`).
#[derive(Clone, Debug)]
pub struct LoomMenu {
    slots: Vec<SlotDef>,
    /// The banner, the dye, the pattern item, then the result.
    items: OwnSlots,
    /// The menu is a server's, at its block.
    at_block: bool,
    /// The data value `selectedBannerPatternIndex`: -1 for none.
    selected: i32,
    /// `selectablePatterns` as `slotsChanged` last made them.
    patterns: Vec<String>,
    /// `lastSoundTime`.
    last_sound_time: i64,
}

/// `DataComponents.DYE` of a dye item: its colour.
pub fn dye_colour(stack: &ItemStack) -> Option<&'static str> {
    let colour = stack.id.strip_prefix("minecraft:")?.strip_suffix("_dye")?;
    DYE_COLOURS.iter().copied().find(|dye| *dye == colour)
}

/// `BannerItem.getColor`: a banner item's base colour.
pub fn banner_colour(stack: &ItemStack) -> Option<&'static str> {
    let colour = stack.id.strip_prefix("minecraft:")?.strip_suffix("_banner")?;
    DYE_COLOURS.iter().copied().find(|dye| *dye == colour)
}

/// A banner's layers (`BANNER_PATTERNS`), as many as it has.
pub fn layer_count(stack: &ItemStack) -> usize {
    stack
        .components
        .as_ref()
        .and_then(|components| components.get(BANNER_PATTERNS))
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

/// `isDyeItem`: in `#loom_dyes`, with a `dye` component.
fn is_dye(cx: &MenuContext, stack: &ItemStack) -> bool {
    cx.inventory.recipes.item_in_tag("minecraft:loom_dyes", &stack.id) && dye_colour(stack).is_some()
}

/// `stack.getItem() instanceof BannerItem`: the 16 banners.
fn is_banner(stack: &ItemStack) -> bool {
    banner_colour(stack).is_some()
}

/// `DataComponents.PROVIDES_BANNER_PATTERNS`, which `Items` gives each
/// `<x>_banner_pattern` as the tag `#pattern_item/<x>`.
fn provided_tag(stack: &ItemStack) -> Option<String> {
    let name = stack
        .id
        .strip_prefix("minecraft:")?
        .strip_suffix("_banner_pattern")?;
    Some(format!("minecraft:pattern_item/{name}"))
}

/// `isPatternItem`: in `#loom_patterns`, providing patterns.
fn is_pattern_item(cx: &MenuContext, stack: &ItemStack) -> bool {
    cx.inventory
        .recipes
        .item_in_tag("minecraft:loom_patterns", &stack.id)
        && provided_tag(stack).is_some()
}

impl LoomMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![
            SlotDef::own(BANNER, 13, 26).with_icon("container/slot/banner"),
            SlotDef::own(DYE, 33, 26).with_icon("container/slot/dye"),
            SlotDef::own(PATTERN, 23, 45).with_icon("container/slot/banner_pattern"),
            SlotDef::own(RESULT, 143, 57),
        ];
        slots.extend(standard_inventory_slots(8, 84));
        items.resize(4, None);
        Self {
            slots,
            items: OwnSlots::new(items),
            at_block: false,
            selected: -1,
            patterns: Vec::new(),
            last_sound_time: i64::MIN,
        }
    }

    /// The menu is a server's, at its block (`ContainerLevelAccess.create`).
    pub fn set_at_block(&mut self, at_block: bool) {
        self.at_block = at_block;
    }

    /// `getSelectedBannerPatternIndex`.
    pub fn selected(&self) -> i32 {
        self.selected
    }

    /// `getSelectablePatterns(patternStack)`: `#no_item_required` without a
    /// pattern item, otherwise the patterns it provides.
    fn patterns_for(cx: &MenuContext, pattern: Option<&ItemStack>) -> Vec<String> {
        let tag = match pattern {
            None => Some("minecraft:no_item_required".to_owned()),
            Some(stack) => provided_tag(stack),
        };
        tag.and_then(|tag| cx.inventory.recipes.banner_pattern_tag(&tag))
            .unwrap_or_else(Vec::new)
    }

    /// `getSelectablePatterns()`, as the slots now make them: none unless
    /// both a banner and a dye are in place.
    pub fn patterns(&self, cx: &MenuContext) -> Vec<String> {
        if self.items.get(BANNER).is_none() || self.items.get(DYE).is_none() {
            return Vec::new();
        }
        Self::patterns_for(cx, self.items.get(PATTERN))
    }

    /// `setupResultSlot`: the banner, one of it, with the pattern added in
    /// the dye's colour.
    fn setup_result(&mut self, pattern: &str) {
        let (Some(banner), Some(dye)) = (self.items.get(BANNER), self.items.get(DYE)) else {
            self.items.set(RESULT, None);
            return;
        };
        let result = dye_colour(dye).map(|colour| {
            let mut components = banner.components.clone().unwrap_or_else(|| json!({}));
            let mut layers = components
                .get(BANNER_PATTERNS)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_else(Vec::new);
            layers.push(json!({"pattern": pattern, "color": colour}));
            components[BANNER_PATTERNS] = Value::Array(layers);
            ItemStack {
                count: 1,
                components: Some(components),
                ..banner.clone()
            }
        });
        self.items.set(RESULT, result);
    }

    /// The result slot's `onTake`: a banner and a dye go; with either gone
    /// nothing is selected; at the block, its sound, once a tick.
    fn take_result(&mut self, cx: &mut MenuContext) {
        remove_from(self, cx, BANNER, 1);
        remove_from(self, cx, DYE, 1);
        if self.items.get(BANNER).is_none() || self.items.get(DYE).is_none() {
            self.selected = -1;
        }
        if self.at_block && self.last_sound_time != cx.game_time {
            cx.sounds.push((TAKE_SOUND, 1.0, 1.0));
            self.last_sound_time = cx.game_time;
        }
    }
}

impl Menu for LoomMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:loom"
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

    fn may_place(&self, cx: &MenuContext, slot: usize, stack: &ItemStack) -> bool {
        match slot {
            BANNER => is_banner(stack),
            DYE => is_dye(cx, stack),
            PATTERN => is_pattern_item(cx, stack),
            RESULT => false,
            _ => true,
        }
    }

    /// `LoomMenu.quickMoveStack`: the result into the inventory, last first;
    /// an input into the inventory; a banner, dye or pattern item into its
    /// slot; otherwise between the main inventory and the hotbar.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let moved = match slot {
            RESULT => move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true),
            BANNER | DYE | PATTERN => move_item_stack_to(self, cx, &mut stack, INVENTORY, END, false),
            _ if is_banner(&stack) => move_item_stack_to(self, cx, &mut stack, BANNER, DYE, false),
            _ if is_dye(cx, &stack) => move_item_stack_to(self, cx, &mut stack, DYE, PATTERN, false),
            _ if is_pattern_item(cx, &stack) => {
                move_item_stack_to(self, cx, &mut stack, PATTERN, RESULT, false)
            }
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

    fn on_take(&mut self, cx: &mut MenuContext, slot: usize, _taken: &ItemStack) {
        if slot == RESULT {
            self.take_result(cx);
        } else {
            super::set_changed(self, cx, slot);
        }
    }

    /// `slotsChanged` for the inputs: with a banner and a dye, the patterns
    /// again, keeping the selected one where it went (the only one when
    /// there is one), and its result unless the banner is full; otherwise
    /// none.
    fn slots_changed(&mut self, cx: &mut MenuContext, slot: usize) {
        if slot == RESULT {
            return;
        }
        let Some(banner) = self.items.get(BANNER).cloned() else {
            self.clear_patterns();
            return;
        };
        if self.items.get(DYE).is_none() {
            self.clear_patterns();
            return;
        }
        let previous = std::mem::take(&mut self.patterns);
        self.patterns = Self::patterns_for(cx, self.items.get(PATTERN));
        let shown = if self.patterns.len() == 1 {
            self.selected = 0;
            Some(self.patterns[0].clone())
        } else {
            let kept = usize::try_from(self.selected)
                .ok()
                .and_then(|index| previous.get(index))
                .and_then(|pattern| self.patterns.iter().position(|p| p == pattern));
            self.selected = kept.map_or(-1, |index| index as i32);
            kept.map(|index| self.patterns[index].clone())
        };
        match shown {
            Some(_) if layer_count(&banner) >= MAX_LAYERS => {
                self.selected = -1;
                self.items.set(RESULT, None);
            }
            Some(pattern) => self.setup_result(&pattern),
            None => self.items.set(RESULT, None),
        }
    }

    /// `clickMenuButton`: one of the patterns is selected, with its result.
    fn click_button(&mut self, cx: &mut MenuContext, id: i32) -> bool {
        let patterns = self.patterns(cx);
        let Some(pattern) = usize::try_from(id).ok().and_then(|index| patterns.get(index)) else {
            return false;
        };
        self.selected = id;
        self.setup_result(pattern);
        true
    }

    /// `removed`: the result goes, then on a server the inputs come back
    /// (`clearContainer`).
    fn removed(&mut self, cx: &mut MenuContext) {
        return_carried(cx);
        self.items.set(RESULT, None);
        if self.at_block {
            clear_own_slots(self, cx, BANNER..RESULT);
        }
    }

    fn data(&self) -> Vec<i32> {
        vec![self.selected]
    }

    fn set_data(&mut self, id: usize, value: i32) {
        if id == 0 {
            self.selected = value;
        }
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

impl LoomMenu {
    /// `slotsChanged` without a banner or a dye.
    fn clear_patterns(&mut self) {
        self.items.set(RESULT, None);
        self.patterns.clear();
        self.selected = -1;
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ContainerInput, MenuInput, handle, set};
    use super::*;
    use crate::crafting::RecipeBook;
    use crate::inventory::Inventory;
    use crate::rng::LegacyRandom;
    use std::sync::Arc;

    fn book() -> Arc<RecipeBook> {
        let file = |name: &str, value: Value| (format!("data/minecraft/{name}.json"), value);
        let pattern = |name: &str| {
            file(
                &format!("banner_pattern/{name}"),
                json!({"asset_id": format!("minecraft:{name}"), "translation_key": format!("block.minecraft.banner.{name}")}),
            )
        };
        Arc::new(RecipeBook::from_files(vec![
            pattern("stripe_top"),
            pattern("cross"),
            pattern("flower"),
            file(
                "tags/banner_pattern/no_item_required",
                json!({"values": ["minecraft:stripe_top", "minecraft:cross"]}),
            ),
            file(
                "tags/banner_pattern/pattern_item/flower",
                json!({"values": ["minecraft:flower"]}),
            ),
            file("tags/item/dyes", json!({"values": ["minecraft:red_dye"]})),
            file("tags/item/loom_dyes", json!({"values": ["#minecraft:dyes"]})),
            file(
                "tags/item/loom_patterns",
                json!({"values": ["minecraft:flower_banner_pattern"]}),
            ),
        ]))
    }

    fn banner(layers: usize) -> ItemStack {
        let layer = json!({"pattern": "minecraft:cross", "color": "black"});
        ItemStack {
            components: (layers > 0)
                .then(|| json!({ BANNER_PATTERNS: vec![layer; layers] })),
            ..ItemStack::new("minecraft:white_banner", 2)
        }
    }

    #[test]
    fn a_selected_pattern_weaves_and_the_take_uses_a_banner_and_a_dye() {
        let mut inventory = Inventory::default();
        inventory.recipes = book();
        let mut random = LegacyRandom::new(1);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        let mut menu = LoomMenu::new(Vec::new());
        menu.set_at_block(true);
        assert!(!menu.may_place(&cx, DYE, &ItemStack::new("minecraft:white_banner", 1)));
        set(&mut menu, &mut cx, BANNER, Some(banner(1)));
        set(&mut menu, &mut cx, DYE, Some(ItemStack::new("minecraft:red_dye", 1)));
        assert_eq!(menu.patterns(&cx), ["minecraft:stripe_top", "minecraft:cross"]);
        assert!(!menu.click_button(&mut cx, 2));
        assert!(menu.click_button(&mut cx, 1));
        let layers = &menu.own().get(RESULT).unwrap().components.as_ref().unwrap()[BANNER_PATTERNS];
        assert_eq!(layers[1], json!({"pattern": "minecraft:cross", "color": "red"}));
        // A pattern item providing one pattern selects it.
        set(&mut menu, &mut cx, PATTERN, Some(ItemStack::new("minecraft:flower_banner_pattern", 1)));
        assert_eq!(menu.selected(), 0);
        let take = MenuInput::Click {
            slot: RESULT as i32,
            button: 0,
            kind: ContainerInput::QuickMove,
        };
        handle(&mut menu, &mut cx, &take);
        assert_eq!(layer_count(cx.inventory.slots[8].as_ref().unwrap()), 2);
        assert_eq!(menu.own().get(BANNER).map(|b| b.count), Some(1));
        assert!(menu.own().get(DYE).is_none(), "the dye ran out");
        assert!(menu.own().get(PATTERN).is_some(), "the pattern item stays");
        assert_eq!(menu.selected(), -1);
        assert_eq!(cx.sounds.len(), 1);
        // A full banner selects nothing and weaves nothing.
        set(&mut menu, &mut cx, BANNER, Some(banner(MAX_LAYERS)));
        set(&mut menu, &mut cx, DYE, Some(ItemStack::new("minecraft:red_dye", 1)));
        assert_eq!((menu.selected(), menu.own().get(RESULT)), (-1, None));
    }
}
