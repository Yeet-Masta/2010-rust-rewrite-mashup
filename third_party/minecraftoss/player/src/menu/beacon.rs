//! `BeaconMenu` (26.3): the payment slot, which takes one of
//! `#beacon_payment_items`, and the beacon's three data values (its
//! pyramid's levels and its two powers, each `MOB_EFFECT` id + 1). The
//! screen's choice (`ServerboundSetBeaconPacket`, `updateEffects`) needs a
//! payment and powers the levels allow; it uses the payment and asks the
//! beacon to take the powers ([`BlockRequest::SetBeacon`]).
//!
//! A server's menu ([`BeaconMenu::set_at_block`]) drops the payment as it
//! closes.
use super::{
    BlockRequest, Menu, MenuContext, OwnSlots, SlotDef, item, move_item_stack_to, put_back,
    remove_from, return_carried, standard_inventory_slots,
};
use crate::inventory::ItemStack;

/// The payment's menu slot.
pub const PAYMENT: usize = 0;
/// The main inventory is 1-27, the hotbar 28-36.
const INVENTORY: usize = 1;
const HOTBAR: usize = 28;
const END: usize = 37;
/// The data values: `DATA_LEVELS`, `DATA_PRIMARY`, `DATA_SECONDARY`.
pub const LEVELS: usize = 0;
pub const PRIMARY: usize = 1;
pub const SECONDARY: usize = 2;

/// `BeaconBlockEntity.BEACON_EFFECTS`: the powers of each tier.
pub const BEACON_EFFECTS: [&[&str]; 4] = [
    &["minecraft:speed", "minecraft:haste"],
    &["minecraft:resistance", "minecraft:jump_boost"],
    &["minecraft:strength"],
    &["minecraft:regeneration"],
];

/// The beacon's powers in the `MOB_EFFECT` registry, by their ids there
/// (`MobEffects`' order).
const REGISTRY_IDS: [(&str, i32); 6] = [
    ("minecraft:speed", 0),
    ("minecraft:haste", 2),
    ("minecraft:strength", 4),
    ("minecraft:jump_boost", 7),
    ("minecraft:regeneration", 9),
    ("minecraft:resistance", 10),
];

/// `BeaconMenu.encodeEffect`: the registry id + 1, or 0 for none.
pub fn encode_effect(effect: Option<&str>) -> i32 {
    effect
        .and_then(|effect| REGISTRY_IDS.iter().find(|(name, _)| *name == effect))
        .map_or(0, |(_, id)| id + 1)
}

/// `BeaconMenu.decodeEffect`, for the beacon's powers.
pub fn decode_effect(value: i32) -> Option<&'static str> {
    REGISTRY_IDS
        .iter()
        .find(|(_, id)| id + 1 == value)
        .map(|(name, _)| *name)
}

/// `getRequiredLevelsFor`: the tier a power is in, from 1; 0 for none.
fn required_levels(effect: Option<&str>) -> i32 {
    let Some(effect) = effect else {
        return 0;
    };
    BEACON_EFFECTS
        .iter()
        .position(|tier| tier.contains(&effect))
        .map_or(i32::MAX, |tier| tier as i32 + 1)
}

/// `BeaconBlockEntity.validateEffects`: a secondary power needs four
/// levels; each power's tier must be built; the primary is never the
/// fourth tier's; the secondary is regeneration or the primary again.
pub fn validate_effects(primary: Option<&str>, secondary: Option<&str>, levels: i32) -> bool {
    if secondary.is_some() && levels < 4 {
        return false;
    }
    let (first, second) = (required_levels(primary), required_levels(secondary));
    if first > levels || second > levels || first >= 4 {
        return false;
    }
    second == 0 || second >= 4 || primary == secondary
}

/// `BeaconMenu` (`beacon`).
#[derive(Clone, Debug)]
pub struct BeaconMenu {
    slots: Vec<SlotDef>,
    /// The payment.
    items: OwnSlots,
    /// The menu is a server's, at its block.
    at_block: bool,
    /// The beacon's data values, as its block entity has them.
    data: [i32; 3],
}

impl BeaconMenu {
    pub fn new(mut items: Vec<Option<ItemStack>>) -> Self {
        let mut slots = vec![SlotDef::own(PAYMENT, 136, 110)];
        slots.extend(standard_inventory_slots(36, 137));
        items.resize(1, None);
        Self {
            slots,
            items: OwnSlots::new(items),
            at_block: false,
            data: [0; 3],
        }
    }

    /// The menu is a server's, at its block (`ContainerLevelAccess.create`).
    pub fn set_at_block(&mut self, at_block: bool) {
        self.at_block = at_block;
    }

    /// `hasPayment`.
    pub fn has_payment(&self) -> bool {
        self.items.get(PAYMENT).is_some()
    }

    /// `PaymentSlot.mayPlace`: `#beacon_payment_items`.
    fn is_payment(cx: &MenuContext, stack: &ItemStack) -> bool {
        cx.inventory
            .recipes
            .item_in_tag("minecraft:beacon_payment_items", &stack.id)
    }
}

impl Menu for BeaconMenu {
    fn menu_type(&self) -> &'static str {
        "minecraft:beacon"
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
        slot != PAYMENT || Self::is_payment(cx, stack)
    }

    /// `PaymentSlot.getMaxStackSize`: one.
    fn max_stack(&self, _cx: &MenuContext, slot: usize, stack: &ItemStack) -> i32 {
        let max = if slot == PAYMENT { 1 } else { super::CONTAINER_MAX_STACK };
        max.min(i32::from(stack.max))
    }

    /// `BeaconMenu.quickMoveStack`: the payment into the inventory, last
    /// first; a payment item into the empty payment slot only when it is a
    /// stack of exactly one; otherwise between the main inventory and the
    /// hotbar.
    fn quick_move_stack(&mut self, cx: &mut MenuContext, slot: usize) -> Option<ItemStack> {
        let mut stack = item(self, cx, slot)?.clone();
        let clicked = stack.clone();
        let moved = if slot == PAYMENT {
            move_item_stack_to(self, cx, &mut stack, INVENTORY, END, true)
        } else if !self.has_payment() && Self::is_payment(cx, &stack) && stack.count == 1 {
            move_item_stack_to(self, cx, &mut stack, PAYMENT, INVENTORY, false)
        } else if (INVENTORY..HOTBAR).contains(&slot) {
            move_item_stack_to(self, cx, &mut stack, HOTBAR, END, false)
        } else if (HOTBAR..END).contains(&slot) {
            move_item_stack_to(self, cx, &mut stack, INVENTORY, HOTBAR, false)
        } else {
            move_item_stack_to(self, cx, &mut stack, INVENTORY, END, false)
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

    /// `updateEffects`: with a payment and powers the levels allow, the
    /// powers go to the beacon and the payment is used.
    fn set_beacon(&mut self, cx: &mut MenuContext, primary: Option<&str>, secondary: Option<&str>) -> bool {
        if !self.has_payment() || !validate_effects(primary, secondary, self.data[LEVELS]) {
            return false;
        }
        self.data[PRIMARY] = encode_effect(primary);
        self.data[SECONDARY] = encode_effect(secondary);
        remove_from(self, cx, PAYMENT, 1);
        cx.block_requests.push(BlockRequest::SetBeacon {
            primary: self.data[PRIMARY],
            secondary: self.data[SECONDARY],
        });
        true
    }

    /// `removed`: on a server the payment is dropped, not given back.
    fn removed(&mut self, cx: &mut MenuContext) {
        return_carried(cx);
        if !self.at_block {
            return;
        }
        if let Some(payment) = remove_from(self, cx, PAYMENT, 1) {
            cx.throw(payment, false);
        }
    }

    fn data(&self) -> Vec<i32> {
        self.data.to_vec()
    }

    fn set_data(&mut self, id: usize, value: i32) {
        if let Some(data) = self.data.get_mut(id) {
            *data = value;
        }
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
    use super::super::{ContainerInput, MenuInput, handle};
    use super::*;
    use crate::crafting::RecipeBook;
    use crate::inventory::Inventory;
    use crate::rng::LegacyRandom;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn the_powers_follow_the_levels_and_use_the_payment() {
        assert!(validate_effects(Some("minecraft:haste"), None, 1));
        assert!(!validate_effects(Some("minecraft:strength"), None, 2));
        assert!(!validate_effects(Some("minecraft:regeneration"), None, 4), "never primary");
        assert!(validate_effects(Some("minecraft:speed"), Some("minecraft:regeneration"), 4));
        assert!(validate_effects(Some("minecraft:speed"), Some("minecraft:speed"), 4), "level II");
        assert!(!validate_effects(Some("minecraft:speed"), Some("minecraft:haste"), 4));
        assert!(!validate_effects(Some("minecraft:speed"), Some("minecraft:speed"), 3));
        assert_eq!(encode_effect(Some("minecraft:jump_boost")), 8);
        assert_eq!(decode_effect(11), Some("minecraft:resistance"));

        let mut inventory = Inventory::default();
        inventory.recipes = Arc::new(RecipeBook::from_files(vec![(
            "data/minecraft/tags/item/beacon_payment_items.json".to_owned(),
            json!({"values": ["minecraft:emerald"]}),
        )]));
        inventory.slots[9] = Some(ItemStack::new("minecraft:emerald", 2));
        let mut random = LegacyRandom::new(1);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        let mut menu = BeaconMenu::new(Vec::new());
        menu.set_at_block(true);
        menu.set_data(LEVELS, 1);
        // Two emeralds will not shift-click in; a right click places one.
        let shift = MenuInput::Click { slot: 1, button: 0, kind: ContainerInput::QuickMove };
        handle(&mut menu, &mut cx, &shift);
        assert!(!menu.has_payment());
        assert_eq!(cx.inventory.slots[0].as_ref().map(|s| s.count), Some(2), "to the hotbar");
        cx.inventory.cursor = cx.inventory.slots[0].take();
        handle(&mut menu, &mut cx, &MenuInput::Click { slot: 0, button: 0, kind: ContainerInput::Pickup });
        assert_eq!(menu.own().get(PAYMENT).map(|s| s.count), Some(1), "one at most");
        let choose = |primary: &str| MenuInput::SetBeacon { primary: Some(primary.to_owned()), secondary: None };
        assert!(!handle(&mut menu, &mut cx, &choose("minecraft:strength")));
        assert!(handle(&mut menu, &mut cx, &choose("minecraft:speed")));
        assert_eq!((menu.data(), menu.has_payment()), (vec![1, 1, 0], false));
        assert_eq!(cx.block_requests, [BlockRequest::SetBeacon { primary: 1, secondary: 0 }]);
    }
}
