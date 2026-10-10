//! The screens of the menus the server runs (`AbstractContainerScreen`):
//! the mouse and keys made into the menu's inputs as vanilla's screen makes
//! them into clicks (`mouseClicked`, `mouseDragged`, `mouseReleased`,
//! `keyPressed`), and the menu's state as the server sends it.
//!
//! One batch of inputs is with the server at a time, with a copy of the
//! inventory taken as it is sent, and the inventory changes only from the
//! answers: pickups wait while a batch is out (`pickups_wait`), uses and
//! drops while the menu is open (`inventory_busy`), so each copy is the
//! inventory as the server last left it. Meanwhile the screen shows what
//! the inputs do as the same menu engine works it out on copies, as
//! vanilla's client clicks its own menu before it sends a click
//! (`handleContainerInput`); each update from the server, and each pickup
//! between batches, starts the copies over.
use std::time::Instant;

use minecraft_terrain::menus::{
    ContainerInput, MenuExtra, MenuInput, MenuKind, MenuOpen, MenuUpdate, PlayerContext, anvil,
    crafter, merchant,
};
use minecraftoss_player::inventory::{Inventory, ItemStack};
use minecraftoss_player::menu::{self, Menu, MenuContext, OFFHAND, SLOT_CLICKED_OUTSIDE, SlotRef};
use minecraftoss_player::rng::LegacyRandom;
use serde_json::Value;

use super::{Game, Input, Key};
use crate::gui::{
    EnchantingBook, Gui, MenuSlotView, MenuView, NameBox, Screen, SlotDrag, TradeList,
    enchanting_row_at,
};
use crate::render::GuiModel;

/// `MouseHandler.DOUBLE_CLICK_THRESHOLD_MS`.
const DOUBLE_CLICK_MS: u128 = 250;

/// A mouse button as a screen tells them apart (vanilla's raw 1, 3 and 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mouse {
    Left,
    Right,
    Middle,
}

impl Mouse {
    /// `getContainerClickButton`: 0 for the primary button, 1 for the
    /// secondary, 2 for the middle.
    fn button(self) -> i32 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Middle => 2,
        }
    }
}

/// The screen's drag (`isQuickCrafting` and the fields with it).
#[derive(Clone, Debug, Default, PartialEq)]
struct QuickCraft {
    /// `quickCraftingButton`, while a drag is on.
    button: Option<Mouse>,
    /// `quickCraftingType`: 0 shares the stack out, 1 places one in each,
    /// 2 places full stacks (creative).
    kind: i32,
    /// `quickCraftSlots`, in the order the mouse covered them.
    slots: Vec<usize>,
    /// `quickCraftingRemainder`: what stays carried.
    remainder: i32,
}

/// What an update for the open menu leaves the screen to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Updated {
    /// Show the menu as it now is.
    Shown,
    /// Close: the server closed the container (`stillValid` failed).
    Closing,
    /// The menu is gone.
    Closed,
}

/// The menu open on the server, and its screen's state.
pub(super) struct ClientMenu {
    id: u8,
    kind: MenuKind,
    /// The title as a text component.
    title: Value,
    /// The engine's menu of the kind: its slots in menu order, and its own
    /// slots as the screen shows them.
    menu: Box<dyn Menu + Send>,
    /// The own slots as of the last update.
    slots: Vec<Option<ItemStack>>,
    /// The data values as of the last update.
    data: Vec<i32>,
    /// The kind's extra as of the last update that had one.
    extra: Option<MenuExtra>,
    /// The trading screen's list.
    trades: TradeList,
    /// The inventory and carried stack as the screen shows them: the last
    /// answer's, with the inputs since.
    shown: Inventory,
    /// What the copies are worked out for.
    selected: usize,
    creative: bool,
    /// The batch with the server: its `seq` and inputs.
    in_flight: Option<(u32, Vec<MenuInput>)>,
    /// Inputs for the next batch.
    queued: Vec<MenuInput>,
    /// The last batch's `seq`.
    seq: u32,
    /// The screen closed, with `Close` sent or queued; the menu goes with
    /// the server's answer.
    closed: bool,
    quick: QuickCraft,
    /// `skipNextRelease`: true at first, for the release of the click that
    /// opened the screen.
    skip_next_release: bool,
    /// `lastClickSlot`, and `doubleclick`.
    last_click_slot: Option<usize>,
    double_click: bool,
    /// `MouseHandler.lastClick`: the last press on this screen, and when.
    last_press: Option<(Mouse, Instant)>,
    /// `lastQuickMoved`: the stack the last shift-click moved.
    last_quick_moved: Option<ItemStack>,
    /// The mouse when last seen, so a drag takes a slot as the mouse moves
    /// over it (`mouseDragged`).
    mouse: (f32, f32),
    /// Sounds the screen played for the player (`Player.playSound`):
    /// event, volume, pitch.
    sounds: Vec<(&'static str, f32, f32)>,
    /// `Player.experienceLevel`, for the copies.
    xp_level: i32,
    /// The enchanting table screen's book.
    book: EnchantingBook,
    /// The anvil screen's name box.
    name: NameBox,
}

impl ClientMenu {
    fn new(update: &MenuUpdate, open: MenuOpen, game: &Game) -> Self {
        let mut menu = open.kind.menu(update.slots.clone());
        // The workstations' results come from the registries, as the
        // server's (without its block's access).
        let rules = minecraft_terrain::menus::station_rules(&game.world.registries);
        minecraft_terrain::menus::attach_rules(menu.as_mut(), rules, false);
        for (id, value) in update.data.iter().enumerate() {
            menu.set_data(id, *value);
        }
        Self {
            id: update.id,
            kind: open.kind,
            title: open.title,
            menu,
            slots: update.slots.clone(),
            data: update.data.clone(),
            extra: update.extra.clone(),
            trades: TradeList::default(),
            shown: game.entities.inventory.clone(),
            selected: game.entities.selected,
            creative: game.creative,
            in_flight: None,
            queued: Vec::new(),
            seq: update.ack,
            closed: false,
            quick: QuickCraft::default(),
            skip_next_release: true,
            last_click_slot: None,
            double_click: false,
            last_press: None,
            last_quick_moved: None,
            mouse: (f32::NAN, f32::NAN),
            sounds: Vec::new(),
            xp_level: game.xp_level(),
            book: EnchantingBook::new(game.ticks),
            name: NameBox::default(),
        }
    }

    /// Runs `f` with the menu and a context over the shown inventory.
    fn with<R>(&mut self, f: impl FnOnce(&mut (dyn Menu + Send), &mut MenuContext) -> R) -> R {
        let mut random = LegacyRandom::new(0);
        let mut cx = MenuContext::new(&mut self.shown, &mut random);
        cx.selected = self.selected;
        cx.creative = self.creative;
        cx.xp_level = self.xp_level;
        f(self.menu.as_mut(), &mut cx)
    }

    /// Runs `f` with the menu, read only, and a context over a copy of the
    /// shown inventory.
    fn read<R>(&self, f: impl FnOnce(&(dyn Menu + Send), &MenuContext) -> R) -> R {
        let mut shown = self.shown.clone();
        let mut random = LegacyRandom::new(0);
        let mut cx = MenuContext::new(&mut shown, &mut random);
        cx.selected = self.selected;
        cx.creative = self.creative;
        f(self.menu.as_ref(), &cx)
    }

    /// `slotClicked`: an input for the server, worked out on the copies
    /// at once.
    fn send(&mut self, input: MenuInput) {
        self.with(|menu, cx| menu::handle(menu, cx, &input));
        self.queued.push(input);
    }

    /// `slotClicked` for a click; a crafter's screen toggles an empty grid
    /// slot first (`CrafterScreen.slotClicked`), with a click's sound.
    fn click(&mut self, slot: i32, button: i32, kind: ContainerInput) {
        if let Ok(index) = usize::try_from(slot)
            && let Some(enabled) =
                self.read(|menu, cx| crafter(menu)?.toggle_for_click(cx, index, button, kind))
        {
            self.send(MenuInput::SlotState {
                slot: index,
                enabled,
            });
            let pitch = if enabled { 1.0 } else { 0.75 };
            self.sounds.push(("minecraft:ui.button.click", 0.4, pitch));
        }
        self.send(MenuInput::Click { slot, button, kind });
    }

    /// The stack a slot shows (`Slot.getItem`).
    fn item(&self, index: usize) -> Option<&ItemStack> {
        match self.menu.slots().get(index)?.at {
            SlotRef::Own(own) => self.menu.own().get(own),
            SlotRef::Player(player) => self.shown.slots.get(player)?.as_ref(),
        }
    }

    /// `Slot.getMaxStackSize(stack)`.
    fn max_stack(&mut self, index: usize, stack: &ItemStack) -> i32 {
        self.with(|menu, cx| menu.max_stack(cx, index, stack))
    }

    /// An update for this menu: its own slots and data, and the end of the
    /// batch it answers. `inventory` is the player's, with the answer's
    /// part already written.
    fn update(&mut self, update: MenuUpdate, inventory: &Inventory) -> Updated {
        if update.cursor.is_some()
            && self
                .in_flight
                .as_ref()
                .is_some_and(|(seq, _)| *seq == update.ack)
        {
            self.in_flight = None;
        }
        if update.closed {
            return Updated::Closed;
        }
        self.slots = update.slots;
        self.data = update.data;
        if update.extra.is_some() {
            self.extra = update.extra;
        }
        self.predict(inventory);
        if update.closing {
            Updated::Closing
        } else {
            Updated::Shown
        }
    }

    /// Whether inputs are with the server, or queued for it: the inventory
    /// waits for their answer.
    fn waits(&self) -> bool {
        self.in_flight.is_some() || !self.queued.is_empty()
    }

    /// The copies again: the last update's, with every input the server
    /// has not answered yet.
    fn predict(&mut self, inventory: &Inventory) {
        self.menu.own_mut().load(self.slots.clone());
        for (id, value) in self.data.iter().enumerate() {
            self.menu.set_data(id, *value);
        }
        if let Some(extra) = &self.extra {
            extra.sync(self.menu.as_mut());
        }
        self.shown = inventory.clone();
        let pending: Vec<MenuInput> = self
            .in_flight
            .iter()
            .flat_map(|(_, inputs)| inputs.iter())
            .chain(&self.queued)
            .cloned()
            .collect();
        for input in &pending {
            self.with(|menu, cx| menu::handle(menu, cx, input));
        }
    }

    /// `shouldAddSlotToQuickCraft`.
    fn takes_drag(&mut self, index: usize) -> bool {
        let Some(carried) = self.shown.cursor.clone() else {
            return false;
        };
        let QuickCraft {
            kind, ref slots, ..
        } = self.quick;
        let size = slots.len();
        self.quick.button.is_some()
            && (usize::from(carried.count) > size || kind == 2)
            && menu::can_item_quick_replace(self.item(index), &carried, true)
            && self.with(|menu, cx| menu.may_place(cx, index, &carried))
            && self.menu.can_drag_to(index)
    }

    /// `recalculateQuickCraftRemaining`.
    fn recalculate_remainder(&mut self) {
        let Some(carried) = self.shown.cursor.clone() else {
            return;
        };
        if self.quick.button.is_none() {
            return;
        }
        if self.quick.kind == 2 {
            self.quick.remainder = i32::from(carried.max);
            return;
        }
        let size = self.quick.slots.len();
        let mut remainder = i32::from(carried.count);
        for index in self.quick.slots.clone() {
            let carry = self.item(index).map_or(0, |stack| i32::from(stack.count));
            let max = i32::from(carried.max).min(self.max_stack(index, &carried));
            let place = menu::quick_craft_place_count(size, self.quick.kind, &carried);
            remainder -= (place + carry).min(max) - carry;
        }
        self.quick.remainder = remainder;
    }

    /// `mouseDragged`: the slot the mouse moved onto joins the drag.
    fn drag_over(&mut self, mouse: (f32, f32), hovered: Option<usize>) {
        let moved = std::mem::replace(&mut self.mouse, mouse) != mouse;
        if let Some(slot) = hovered
            && moved
            && !self.quick.slots.contains(&slot)
            && self.takes_drag(slot)
        {
            self.quick.slots.push(slot);
            self.recalculate_remainder();
        }
    }

    /// `extractSlot`'s check as it draws the slots in menu order: a slot of
    /// the drag that can no longer take the carried stack leaves it, until
    /// one is left, which stays whatever it holds (and is not drawn).
    fn prune_drag(&mut self) {
        let Some(carried) = self.shown.cursor.clone() else {
            return;
        };
        if self.quick.button.is_none() {
            return;
        }
        for index in 0..self.menu.slots().len() {
            if self.quick.slots.len() <= 1 {
                return;
            }
            let Some(at) = self.quick.slots.iter().position(|&slot| slot == index) else {
                continue;
            };
            if !menu::can_item_quick_replace(self.item(index), &carried, true)
                || !self.menu.can_drag_to(index)
            {
                self.quick.slots.remove(at);
                self.recalculate_remainder();
            }
        }
    }

    /// `mouseClicked`.
    fn press(&mut self, button: Mouse, hovered: Option<usize>, outside: bool, shift: bool) {
        // The pick-block button, with infinite materials.
        let cloning = button == Mouse::Middle && self.creative;
        let double = self
            .last_press
            .is_some_and(|(last, at)| last == button && at.elapsed().as_millis() < DOUBLE_CLICK_MS);
        self.last_press = Some((button, Instant::now()));
        self.double_click = self.last_click_slot == hovered && double;
        self.skip_next_release = false;
        // Another button would go to the hotbar keys (`checkHotbarMouseClicked`),
        // which no mouse button is bound to by default.
        if button != Mouse::Middle || cloning {
            let slot = if outside {
                SLOT_CLICKED_OUTSIDE
            } else {
                hovered.map_or(-1, |index| index as i32)
            };
            if slot != -1 && self.quick.button.is_none() {
                if self.shown.cursor.is_none() {
                    let kind = if cloning {
                        ContainerInput::Clone
                    } else if slot != SLOT_CLICKED_OUTSIDE && shift {
                        self.last_quick_moved = hovered.and_then(|index| self.item(index)).cloned();
                        ContainerInput::QuickMove
                    } else if slot == SLOT_CLICKED_OUTSIDE {
                        ContainerInput::Throw
                    } else {
                        ContainerInput::Pickup
                    };
                    self.click(slot, button.button(), kind);
                    self.skip_next_release = true;
                } else {
                    // A press with a stack carried starts a drag; nothing
                    // is sent until the release.
                    self.quick = QuickCraft {
                        button: Some(button),
                        kind: button.button(),
                        slots: Vec::new(),
                        remainder: 0,
                    };
                }
            }
        }
        self.last_click_slot = hovered;
    }

    /// `mouseReleased`.
    fn release(&mut self, button: Mouse, hovered: Option<usize>, outside: bool, shift: bool) {
        let slot = if outside {
            SLOT_CLICKED_OUTSIDE
        } else {
            hovered.map_or(-1, |index| index as i32)
        };
        let gathers = self.double_click
            && button == Mouse::Left
            && hovered.is_some_and(|index| {
                let empty = ItemStack::new("minecraft:air", 0);
                self.menu.can_take_for_pick_all(&empty, index)
            });
        if let Some(index) = hovered.filter(|_| gathers) {
            if shift {
                // Shift and a double click: a shift-click on every slot of
                // the clicked slot's container holding what the last one
                // moved, each worked out before the next is looked at.
                if let Some(moved) = self.last_quick_moved.clone() {
                    for target in 0..self.menu.slots().len() {
                        if menu::same_container(self.menu.as_ref(), target, index)
                            && self.item(target).is_some()
                            && self.with(|menu, cx| menu.may_pickup(cx, target))
                            && menu::can_item_quick_replace(self.item(target), &moved, true)
                        {
                            self.click(target as i32, button.button(), ContainerInput::QuickMove);
                        }
                    }
                }
            } else {
                self.click(slot, button.button(), ContainerInput::PickupAll);
            }
            self.double_click = false;
        } else {
            if self.quick.button.is_some_and(|started| started != button) {
                // Another button let go: the drag is off.
                self.quick = QuickCraft::default();
                self.skip_next_release = true;
                return;
            }
            if self.skip_next_release {
                self.skip_next_release = false;
                return;
            }
            if self.quick.button.is_some() && !self.quick.slots.is_empty() {
                // `quickCraftToSlots`: the start, a slot each, the end.
                let QuickCraft {
                    kind, ref slots, ..
                } = self.quick;
                let input = MenuInput::Drag {
                    button: kind,
                    slots: slots.clone(),
                };
                self.send(input);
            } else if self.shown.cursor.is_some() {
                if button == Mouse::Middle {
                    self.click(slot, button.button(), ContainerInput::Clone);
                } else {
                    let quick_key = slot != SLOT_CLICKED_OUTSIDE && shift;
                    if quick_key {
                        self.last_quick_moved = hovered.and_then(|index| self.item(index)).cloned();
                    }
                    let kind = if quick_key {
                        ContainerInput::QuickMove
                    } else {
                        ContainerInput::Pickup
                    };
                    self.click(slot, button.button(), kind);
                }
            }
        }
        self.quick = QuickCraft::default();
    }

    /// `keyPressed` over the screen (less E and Esc, which close it): the
    /// hotbar keys and F swap with the hovered slot (`checkHotbarKeyPressed`),
    /// Q drops from it. Whether the key was the screen's.
    fn key(&mut self, key: Key, hovered: Option<usize>, ctrl: bool) -> bool {
        let swap = match key {
            Key::Hotbar(slot) => Some(slot as i32),
            Key::SwapOffhand => Some(OFFHAND),
            Key::Drop => None,
            _ => return false,
        };
        let Some(index) = hovered else {
            return true;
        };
        match swap {
            Some(button) => {
                if self.shown.cursor.is_none() {
                    self.click(index as i32, button, ContainerInput::Swap);
                }
            }
            None => {
                if self.item(index).is_some() {
                    self.click(index as i32, i32::from(ctrl), ContainerInput::Throw);
                }
            }
        }
        true
    }

    /// What the screen draws: each slot as `extractSlot` shows it, and the
    /// carried stack as `extractCarriedItem` does.
    fn view<'a>(&'a self, title: String, hovered: Option<usize>) -> MenuView<'a> {
        let carried = self.shown.cursor.as_ref();
        let dragging = self.quick.button.is_some() && carried.is_some();
        let size = self.quick.slots.len();
        // What each dragged slot holds of the carried stack.
        let maxes: Vec<i32> = match carried {
            Some(carried) if dragging && size > 1 => self.read(|menu, cx| {
                self.quick
                    .slots
                    .iter()
                    .map(|&index| menu.max_stack(cx, index, carried))
                    .collect()
            }),
            _ => Vec::new(),
        };
        let mut slots = Vec::with_capacity(self.menu.slots().len());
        for (index, slot) in self.menu.slots().iter().enumerate() {
            let mut stack = self.item(index).cloned();
            let mut drag = SlotDrag::None;
            let dragged = self.quick.slots.iter().position(|&slot| slot == index);
            if let Some(carried) = carried.filter(|_| dragging)
                && let Some(at) = dragged
            {
                if size == 1 {
                    drag = SlotDrag::Hidden;
                } else {
                    let max = i32::from(carried.max).min(maxes[at]);
                    let carry = stack.as_ref().map_or(0, |stack| i32::from(stack.count));
                    let mut count =
                        menu::quick_craft_place_count(size, self.quick.kind, carried) + carry;
                    let mut full = None;
                    if count > max {
                        count = max;
                        full = Some(max);
                    }
                    stack = (count > 0).then(|| ItemStack {
                        count: count as u8,
                        ..carried.clone()
                    });
                    drag = SlotDrag::Preview(full);
                }
            }
            slots.push(MenuSlotView {
                x: slot.x,
                y: slot.y,
                stack,
                icon: slot.icon,
                drag,
            });
        }
        let carried_view = carried.and_then(|carried| {
            if dragging && size > 1 {
                (self.quick.remainder > 0).then(|| ItemStack {
                    count: self.quick.remainder.min(255) as u8,
                    ..carried.clone()
                })
            } else {
                Some(carried.clone())
            }
        });
        let tooltip = carried
            .is_none()
            .then(|| hovered.and_then(|index| self.item(index)).cloned())
            .flatten();
        MenuView {
            kind: self.kind,
            title,
            slots,
            hovered,
            carried: carried_view,
            tooltip,
            inventory: &self.shown,
            data: self.menu.data(),
            menu: self.menu.as_ref(),
            trades: &self.trades,
            xp_level: self.xp_level,
            creative: self.creative,
            name: (self.kind == MenuKind::Anvil)
                .then(|| (self.name.text.as_str(), self.name.cursor())),
        }
    }

    /// `AnvilScreen.slotChanged`: another stack in the input resets the
    /// name box to its name, which the box then answers.
    fn watch_anvil_input(&mut self, gui: &Gui) {
        if self.kind != MenuKind::Anvil {
            return;
        }
        let input = self.item(0).cloned();
        if self
            .name
            .input_changed(input.as_ref(), |stack| gui.stack_name(stack))
        {
            self.name_changed(gui);
        }
    }

    /// `AnvilScreen.onNameChanged`: the name, or none for the input's own,
    /// goes to the menu when it would take it (`setItemName`).
    fn name_changed(&mut self, gui: &Gui) {
        let Some(input) = self.item(0).cloned() else {
            return;
        };
        let name = crate::gui::anvil_name_to_send(&self.name.text, &input, &gui.stack_name(&input));
        let current =
            anvil(self.menu.as_ref()).and_then(|menu| menu.item_name().map(str::to_owned));
        if crate::gui::anvil_renames(current.as_deref(), &name) {
            self.send(MenuInput::Rename(name));
        }
    }

    /// `AnvilScreen.keyPressed`: while the input holds a stack, the name box
    /// takes every key (E types an e). Whether it took the key.
    fn anvil_key(&mut self, key: Key, gui: &Gui, ctrl: bool) -> bool {
        if self.kind != MenuKind::Anvil || self.item(0).is_none() {
            return false;
        }
        let typed = match key {
            Key::Char(c) => Some(c),
            _ => None,
        };
        if self.name.key(typed, key == Key::Backspace, ctrl) {
            self.name_changed(gui);
        }
        true
    }

    /// `EnchantmentScreen.mouseClicked`: a press in a row the menu would
    /// take (`clickMenuButton` on the copy) sends its button. Whether it
    /// did.
    fn press_enchanting_row(&mut self, x: f32, y: f32) -> bool {
        let Some(row) = enchanting_row_at(x, y).filter(|_| self.kind == MenuKind::Enchantment)
        else {
            return false;
        };
        if !self.with(|menu, cx| menu.click_button(cx, row)) {
            return false;
        }
        self.send(MenuInput::Button(row));
        true
    }
}

impl Game {
    /// `Player.experienceLevel`.
    fn xp_level(&self) -> i32 {
        self.player.survival.experience_level.min(i32::MAX as u32) as i32
    }

    /// `containerTick`: the enchanting table's screen moves its book
    /// (`tickBook`).
    pub(super) fn tick_menu(&mut self) {
        let level = self.xp_level();
        let Some(menu) = self.menu.as_mut().filter(|menu| !menu.closed) else {
            return;
        };
        menu.xp_level = level;
        if menu.kind == MenuKind::Enchantment {
            let offers = menu.menu.data().iter().take(3).any(|&cost| cost != 0);
            let item = menu.item(0).cloned();
            menu.book.tick(item.as_ref(), offers);
        }
    }

    /// The enchanting table screen's book, drawn this frame
    /// (`extractBook`).
    pub(super) fn enchanting_book(&self, gui: &Gui, partial: f32) -> Option<GuiModel> {
        let menu = self
            .menu
            .as_ref()
            .filter(|menu| !menu.closed && menu.kind == MenuKind::Enchantment)?;
        let (open, flip) = menu.book.at(partial);
        crate::gui::book_model(&self.world.atlas, gui.scale, open, flip)
    }

    /// What the server reads of the player with a use or a menu batch.
    pub(super) fn player_context(&self) -> PlayerContext {
        PlayerContext {
            inventory: self.entities.inventory.clone(),
            selected: self.entities.selected,
            eye: self.player.eye().to_array(),
            feet: self.player.pos.to_array(),
            creative: self.creative,
            xp_level: self.xp_level(),
            enchantment_seed: self.enchantment_seed,
        }
    }

    /// Whether the inventory waits on the server: a menu is open there (its
    /// screen may have closed already), or a use of a menu block is.
    pub(super) fn inventory_busy(&self) -> bool {
        self.menu.is_some() || self.use_pending.is_some()
    }

    /// Whether a copy of the inventory is with the server, or about to go,
    /// whose answer is written over the inventory: a batch of the menu's,
    /// or a use of a menu block. Pickups wait for the answer; between
    /// batches they go into the inventory, and the open screen shows them,
    /// as vanilla's `ItemEntity.playerTouch` fills the inventory whatever
    /// screen is open.
    pub(super) fn pickups_wait(&self) -> bool {
        self.use_pending.is_some() || self.menu.as_ref().is_some_and(ClientMenu::waits)
    }

    /// The inventory changed outside the menu's answers (a pickup): the
    /// open screen shows it.
    pub(super) fn inventory_changed(&mut self) {
        if let Some(menu) = self.menu.as_mut() {
            menu.predict(&self.entities.inventory);
        }
    }

    /// A menu update from the server, in order.
    pub(super) fn menu_update(&mut self, update: MenuUpdate) {
        // The player's part, on the answer to a batch: what it changed in
        // the inventory, and the carried stack.
        if let Some(cursor) = update.cursor.clone() {
            let inventory = &mut self.entities.inventory;
            for (slot, stack) in &update.player {
                if let Some(held) = inventory.slots.get_mut(*slot) {
                    *held = stack.clone();
                }
            }
            inventory.cursor = cursor;
            for recipe in &update.unlocked {
                inventory.unlock_recipe(recipe);
            }
            // The client keeps no statistics yet: `update.stats` go unread.
            // The levels spent (`onEnchantmentPerformed`, and
            // `giveExperienceLevels` with fewer): none below 0, where the
            // progress and the total go too.
            let levels = u32::try_from(update.xp_levels).unwrap_or(0);
            let survival = &mut self.player.survival;
            if levels > survival.experience_level {
                survival.experience_level = 0;
                survival.experience_progress = 0.0;
                survival.total_experience = 0;
            } else {
                survival.experience_level -= levels;
            }
            if let Some(seed) = update.enchantment_seed {
                self.enchantment_seed = seed;
            }
        }
        self.throw(update.thrown.clone());
        if let Some(ender) = update.ender.clone() {
            self.ender_items = ender;
        }
        if let Some(open) = update.open.clone() {
            self.menu = Some(ClientMenu::new(&update, open, self));
            match self.screen {
                // `ClientboundOpenScreenPacket` opens the screen.
                Screen::Playing | Screen::Paused => self.screen = Screen::Menu,
                // Dead, or another screen took its place: closed at once.
                _ => self.close_menu(),
            }
        }
        let creative = self.creative;
        let Some(menu) = self.menu.as_mut().filter(|menu| menu.id == update.id) else {
            return;
        };
        menu.creative = creative;
        match menu.update(update, &self.entities.inventory) {
            Updated::Shown => {}
            // `stillValid` failed on the server: the screen closes, and
            // says so.
            Updated::Closing => self.close_menu(),
            Updated::Closed => {
                self.menu = None;
                if self.screen == Screen::Menu {
                    self.screen = Screen::Playing;
                }
            }
        }
    }

    /// `onClose`: the screen closes at once, and `Close` goes to the server,
    /// whose answer returns the carried stack and ends the menu.
    pub(super) fn close_menu(&mut self) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        if !menu.closed {
            menu.closed = true;
            menu.quick = QuickCraft::default();
            menu.queued.push(MenuInput::Close);
        }
        if self.screen == Screen::Menu {
            self.screen = Screen::Playing;
        }
    }

    /// Sends the inputs queued as one batch, unless one is with the server.
    pub(super) fn flush_menu(&mut self) {
        let player = match self.menu.as_ref() {
            Some(menu) if menu.in_flight.is_none() && !menu.queued.is_empty() => {
                self.player_context()
            }
            _ => return,
        };
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        menu.seq += 1;
        let inputs = std::mem::take(&mut menu.queued);
        menu.in_flight = Some((menu.seq, inputs.clone()));
        let (id, seq) = (menu.id, menu.seq);
        self.entities.menu(id, seq, inputs, player);
    }

    /// Before the world saves for good or the player dies: the menu closes
    /// on the server, and its answer is in (the carried stack back in the
    /// inventory, a barrel shut), a use waiting on the server is answered,
    /// and everything else is taken in ([`Game::catch_up`]).
    pub(super) fn settle_menu(&mut self) {
        for _ in 0..8 {
            if !self.inventory_busy() {
                break;
            }
            self.close_menu();
            self.flush_menu();
            let events = self.entities.wait();
            self.handle_events(events, false);
        }
        self.catch_up();
    }

    /// Everything sent to the server is answered and taken in: the menu's
    /// batches (the open menu stays open), a use, and the stacks the server
    /// let the player pick up, which go into the inventory now (what no
    /// longer fits drops at the feet). Before a save, so the player saves
    /// as of the world saved with it, and before the inventory drops at
    /// death, which holds what was picked up before it.
    pub(super) fn catch_up(&mut self) {
        for _ in 0..8 {
            let events = self.entities.wait();
            self.handle_events(events, false);
            if !self.pickups_wait() {
                break;
            }
        }
        if self.entities.take_pickups(self.player.pos) {
            self.inventory_changed();
        }
    }

    /// The menu screen's mouse: presses, the drag, releases.
    pub(super) fn menu_input(&mut self, input: &mut Input, gui: &Gui) {
        let clicks: Vec<(Mouse, bool)> = input
            .clicks
            .drain(..)
            .map(|(right, pressed)| (if right { Mouse::Right } else { Mouse::Left }, pressed))
            .chain(
                input
                    .middle_clicks
                    .drain(..)
                    .map(|pressed| (Mouse::Middle, pressed)),
            )
            .collect();
        let level = self.xp_level();
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        menu.xp_level = level;
        menu.watch_anvil_input(gui);
        let (hovered, outside) = gui.menu_slot_at(menu.kind, menu.menu.slots());
        // The trading screen's list: the wheel scrolls it, and a held
        // scroller follows the mouse (`mouseScrolled`, `mouseDragged`).
        let offers = merchant(menu.menu.as_ref()).map(|trader| trader.offers().offers.len());
        let (mx, my) = gui.menu_mouse(menu.kind);
        if let Some(offers) = offers {
            menu.trades
                .wheel(super::take_notches(&mut input.scroll), offers);
            menu.trades.drag(my, offers);
        }
        if menu.quick.button.is_some() {
            menu.drag_over(gui.mouse, hovered);
        } else {
            menu.mouse = gui.mouse;
        }
        for (button, pressed) in clicks {
            if pressed {
                // Its buttons take a press before the slots: one picks its
                // offer, and the payments are filled for it.
                let taken = offers
                    .and_then(|offers| menu.trades.press(mx, my, button == Mouse::Left, offers));
                match taken {
                    Some(Some(index)) => menu.send(MenuInput::SelectTrade(index)),
                    Some(None) => {}
                    None if menu.press_enchanting_row(mx, my) => {}
                    None => menu.press(button, hovered, outside, input.shift),
                }
            } else {
                menu.trades.release();
                menu.release(button, hovered, outside, input.shift);
            }
        }
        menu.prune_drag();
        self.play_menu_sounds();
    }

    /// The sounds the screen played, at the player.
    fn play_menu_sounds(&mut self) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let at = self.player.pos;
        for (event, volume, pitch) in std::mem::take(&mut menu.sounds) {
            self.play(event, Some(at), volume, pitch);
        }
    }

    /// The menu screen's keys; true when the screen took the key.
    pub(super) fn menu_key(&mut self, key: Key, gui: &Gui, ctrl: bool) -> bool {
        if let Some(menu) = self.menu.as_mut() {
            menu.watch_anvil_input(gui);
            if key != Key::Escape && menu.anvil_key(key, gui, ctrl) {
                return true;
            }
        }
        if matches!(key, Key::Escape | Key::Inventory) {
            self.close_menu();
            return true;
        }
        let Some(menu) = self.menu.as_mut() else {
            return false;
        };
        let (hovered, _) = gui.menu_slot_at(menu.kind, menu.menu.slots());
        let taken = menu.key(key, hovered, ctrl);
        self.play_menu_sounds();
        taken
    }

    /// What `gui.menu_screen` draws.
    pub(super) fn menu_view(&self, gui: &Gui) -> Option<MenuView<'_>> {
        let menu = self.menu.as_ref().filter(|menu| !menu.closed)?;
        let (hovered, _) = gui.menu_slot_at(menu.kind, menu.menu.slots());
        // A trader's name comes with its level.
        let title = match merchant(menu.menu.as_ref()) {
            Some(trader) => TradeList::title(&menu.title, trader.offers()),
            None => menu.title.clone(),
        };
        let title = crate::creative::text(gui.language(), &title);
        Some(menu.view(title, hovered))
    }

    /// The inventory as the screen shows it (the HUD's under it).
    pub(super) fn shown_inventory(&self) -> &Inventory {
        self.menu
            .as_ref()
            .filter(|menu| !menu.closed)
            .map_or(&self.entities.inventory, |menu| &menu.shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(id: &str, count: u8) -> ItemStack {
        ItemStack::new(id, count)
    }

    /// A chest's screen over an inventory, as an opening makes it.
    fn chest(own: Vec<Option<ItemStack>>, inventory: Inventory) -> ClientMenu {
        let mut own = own;
        own.resize(27, None);
        screen(MenuKind::Generic { rows: 3 }, own, inventory)
    }

    /// A screen of a kind over an inventory.
    fn screen(kind: MenuKind, own: Vec<Option<ItemStack>>, inventory: Inventory) -> ClientMenu {
        ClientMenu {
            id: 1,
            kind,
            title: Value::Null,
            menu: kind.menu(own.clone()),
            slots: own,
            data: Vec::new(),
            extra: None,
            trades: TradeList::default(),
            shown: inventory,
            selected: 0,
            creative: false,
            in_flight: None,
            queued: Vec::new(),
            seq: 0,
            closed: false,
            quick: QuickCraft::default(),
            skip_next_release: true,
            last_click_slot: None,
            double_click: false,
            last_press: None,
            last_quick_moved: None,
            mouse: (0.0, 0.0),
            sounds: Vec::new(),
            xp_level: 0,
            book: EnchantingBook::new(0),
            name: NameBox::default(),
        }
    }

    #[test]
    fn the_release_of_the_opening_click_is_skipped() {
        let mut inventory = Inventory::default();
        inventory.cursor = Some(stack("minecraft:stone", 3));
        let mut menu = chest(Vec::new(), inventory);
        // The right button that opened the chest comes up over the window.
        menu.release(Mouse::Right, Some(0), false, false);
        assert!(menu.queued.is_empty());
    }

    #[test]
    fn a_press_with_an_empty_hand_picks_up_and_its_release_does_nothing() {
        let mut menu = chest(
            vec![Some(stack("minecraft:stone", 10))],
            Inventory::default(),
        );
        menu.press(Mouse::Left, Some(0), false, false);
        menu.release(Mouse::Left, Some(0), false, false);
        assert_eq!(
            menu.queued,
            [MenuInput::Click {
                slot: 0,
                button: 0,
                kind: ContainerInput::Pickup
            }]
        );
        // The screen shows the engine's outcome before any answer.
        assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(10));
        assert!(menu.item(0).is_none());
    }

    #[test]
    fn a_right_press_takes_half_and_shift_moves() {
        let mut menu = chest(
            vec![Some(stack("minecraft:stone", 9))],
            Inventory::default(),
        );
        menu.press(Mouse::Right, Some(0), false, false);
        assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(5));
        menu.release(Mouse::Right, Some(0), false, false);
        // Put it back with a left click: press starts a drag, release places.
        menu.press(Mouse::Left, Some(0), false, false);
        menu.release(Mouse::Left, Some(0), false, false);
        assert_eq!(menu.item(0).map(|s| s.count), Some(9));
        assert!(menu.shown.cursor.is_none());
        // Shift into the inventory: the hotbar's last slot first.
        menu.press(Mouse::Left, Some(0), false, true);
        assert_eq!(menu.shown.slots[8].as_ref().map(|s| s.count), Some(9));
        assert_eq!(menu.queued.len(), 3);
        assert_eq!(
            menu.queued[2],
            MenuInput::Click {
                slot: 0,
                button: 0,
                kind: ContainerInput::QuickMove
            }
        );
    }

    #[test]
    fn a_drag_over_slots_is_one_input_and_previews_its_shares() {
        let mut inventory = Inventory::default();
        inventory.cursor = Some(stack("minecraft:stone", 10));
        let mut menu = chest(vec![None, Some(stack("minecraft:stone", 62))], inventory);
        menu.skip_next_release = false;
        menu.press(Mouse::Left, Some(0), false, false);
        menu.drag_over((1.0, 0.0), Some(0));
        // One slot so far: drawn not at all.
        let view = menu.view(String::new(), None);
        assert_eq!(view.slots[0].drag, SlotDrag::Hidden);
        menu.drag_over((2.0, 0.0), Some(1));
        menu.drag_over((3.0, 0.0), Some(2));
        // The mouse resting on a slot adds nothing more.
        menu.drag_over((3.0, 0.0), Some(3));
        assert_eq!(menu.quick.slots, [0, 1, 2]);
        // 10 / 3 each; the full stack takes only 2 more, in yellow.
        let view = menu.view(String::new(), None);
        assert_eq!(view.slots[0].drag, SlotDrag::Preview(None));
        assert_eq!(view.slots[0].stack.as_ref().map(|s| s.count), Some(3));
        assert_eq!(view.slots[1].drag, SlotDrag::Preview(Some(64)));
        assert_eq!(view.slots[1].stack.as_ref().map(|s| s.count), Some(64));
        assert_eq!(view.carried.as_ref().map(|s| s.count), Some(10 - 3 - 2 - 3));
        menu.release(Mouse::Left, Some(2), false, false);
        assert_eq!(
            menu.queued,
            [MenuInput::Drag {
                button: 0,
                slots: vec![0, 1, 2]
            }]
        );
        assert_eq!(menu.item(1).map(|s| s.count), Some(64));
        assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(2));
    }

    #[test]
    fn a_drag_whose_slots_all_fill_keeps_its_last_slot() {
        // Two hoppers fill both dragged slots with other items in one tick:
        // the drag loses them one at a time in menu order down to the last
        // (`extractSlot`), and the release still sends it, which places
        // nothing.
        let mut inventory = Inventory::default();
        inventory.cursor = Some(stack("minecraft:stone", 10));
        for button in [Mouse::Left, Mouse::Right] {
            let mut menu = chest(Vec::new(), inventory.clone());
            menu.skip_next_release = false;
            menu.press(button, Some(0), false, false);
            menu.drag_over((1.0, 0.0), Some(0));
            menu.drag_over((2.0, 0.0), Some(1));
            let mut slots = vec![
                Some(stack("minecraft:dirt", 1)),
                Some(stack("minecraft:sand", 1)),
            ];
            slots.resize(27, None);
            menu.slots = slots;
            menu.predict(&inventory);
            menu.prune_drag();
            assert_eq!(menu.quick.slots, [1]);
            let view = menu.view(String::new(), None);
            assert_eq!(view.slots[1].drag, SlotDrag::Hidden);
            assert_eq!(view.carried.as_ref().map(|s| s.count), Some(10));
            menu.release(button, Some(1), false, false);
            assert_eq!(
                menu.queued,
                [MenuInput::Drag {
                    button: button.button(),
                    slots: vec![1]
                }]
            );
            assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(10));
            assert_eq!(menu.item(0).map(|s| s.id.as_str()), Some("minecraft:dirt"));
            assert_eq!(menu.item(1).map(|s| s.id.as_str()), Some("minecraft:sand"));
        }
    }

    #[test]
    fn another_button_let_go_calls_the_drag_off() {
        let mut inventory = Inventory::default();
        inventory.cursor = Some(stack("minecraft:stone", 10));
        let mut menu = chest(Vec::new(), inventory);
        menu.skip_next_release = false;
        menu.press(Mouse::Left, Some(0), false, false);
        menu.drag_over((1.0, 0.0), Some(0));
        menu.drag_over((2.0, 0.0), Some(1));
        menu.release(Mouse::Right, Some(1), false, false);
        menu.release(Mouse::Left, Some(1), false, false);
        assert!(menu.queued.is_empty());
        assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(10));
    }

    #[test]
    fn a_click_outside_throws_on_release_and_an_empty_hand_sends_throw() {
        let mut inventory = Inventory::default();
        inventory.cursor = Some(stack("minecraft:stone", 10));
        let mut menu = chest(Vec::new(), inventory);
        menu.skip_next_release = false;
        menu.press(Mouse::Right, None, true, false);
        assert!(menu.queued.is_empty());
        menu.release(Mouse::Right, None, true, false);
        assert_eq!(
            menu.queued,
            [MenuInput::Click {
                slot: -999,
                button: 1,
                kind: ContainerInput::Pickup
            }]
        );
        assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(9));
        menu.queued.clear();
        menu.shown.cursor = None;
        menu.press(Mouse::Left, None, true, false);
        assert_eq!(
            menu.queued,
            [MenuInput::Click {
                slot: -999,
                button: 0,
                kind: ContainerInput::Throw
            }]
        );
    }

    #[test]
    fn a_double_click_gathers_and_a_shift_double_click_moves_all() {
        let mut inventory = Inventory::default();
        inventory.slots[9] = Some(stack("minecraft:dirt", 5));
        inventory.slots[10] = Some(stack("minecraft:dirt", 7));
        let own = vec![
            Some(stack("minecraft:dirt", 3)),
            None,
            Some(stack("minecraft:dirt", 4)),
        ];
        let mut menu = chest(own, inventory);
        // Pick up slot 0, then press and let go on it again at once.
        menu.press(Mouse::Left, Some(0), false, false);
        menu.release(Mouse::Left, Some(0), false, false);
        menu.press(Mouse::Left, Some(0), false, false);
        menu.release(Mouse::Left, Some(0), false, false);
        assert_eq!(
            menu.queued[1],
            MenuInput::Click {
                slot: 0,
                button: 0,
                kind: ContainerInput::PickupAll
            }
        );
        assert_eq!(
            menu.shown.cursor.as_ref().map(|s| s.count),
            Some(3 + 4 + 5 + 7)
        );
        // With a stack carried, a shift-click on dirt moves it (the release
        // sends it, and remembers dirt), and a shift double click moves
        // every other chest slot holding dirt.
        let mut inventory = Inventory::default();
        inventory.cursor = Some(stack("minecraft:stone", 1));
        let own = vec![
            Some(stack("minecraft:dirt", 3)),
            None,
            Some(stack("minecraft:dirt", 4)),
        ];
        let mut menu = chest(own, inventory);
        menu.skip_next_release = false;
        menu.press(Mouse::Left, Some(0), false, true);
        menu.release(Mouse::Left, Some(0), false, true);
        menu.press(Mouse::Left, Some(0), false, true);
        menu.release(Mouse::Left, Some(0), false, true);
        assert_eq!(
            menu.queued,
            [
                MenuInput::Click {
                    slot: 0,
                    button: 0,
                    kind: ContainerInput::QuickMove
                },
                MenuInput::Click {
                    slot: 2,
                    button: 0,
                    kind: ContainerInput::QuickMove
                },
            ]
        );
        assert!((0..27).all(|i| menu.item(i).is_none()));
        assert_eq!(menu.shown.slots[8].as_ref().map(|s| s.count), Some(7));
        // With an empty hand the second press finds the slot emptied, so
        // there is nothing to move all of (`lastQuickMoved` is empty).
        let own = vec![
            Some(stack("minecraft:dirt", 3)),
            None,
            Some(stack("minecraft:dirt", 4)),
        ];
        let mut menu = chest(own, Inventory::default());
        menu.press(Mouse::Left, Some(0), false, true);
        menu.release(Mouse::Left, Some(0), false, true);
        menu.press(Mouse::Left, Some(0), false, true);
        menu.release(Mouse::Left, Some(0), false, true);
        assert_eq!(menu.item(2).map(|s| s.count), Some(4));
    }

    #[test]
    fn keys_swap_and_throw_over_the_hovered_slot() {
        let mut inventory = Inventory::default();
        inventory.slots[2] = Some(stack("minecraft:dirt", 5));
        let mut menu = chest(vec![Some(stack("minecraft:stone", 4))], inventory);
        assert!(menu.key(Key::Hotbar(2), Some(0), false));
        assert_eq!(menu.item(0).map(|s| s.id.as_str()), Some("minecraft:dirt"));
        assert!(menu.key(Key::SwapOffhand, Some(0), false));
        assert_eq!(
            menu.shown.slots[40].as_ref().map(|s| s.id.as_str()),
            Some("minecraft:dirt")
        );
        assert!(menu.key(Key::Drop, Some(27 + 27 + 2), true));
        assert_eq!(
            menu.queued,
            [
                MenuInput::Click {
                    slot: 0,
                    button: 2,
                    kind: ContainerInput::Swap
                },
                MenuInput::Click {
                    slot: 0,
                    button: OFFHAND,
                    kind: ContainerInput::Swap
                },
                MenuInput::Click {
                    slot: 56,
                    button: 1,
                    kind: ContainerInput::Throw
                },
            ]
        );
        assert!(!menu.key(Key::Debug, Some(0), false));
    }

    #[test]
    fn creative_middle_clicks_clone_and_survival_ones_do_nothing() {
        let mut menu = chest(
            vec![Some(stack("minecraft:stone", 4))],
            Inventory::default(),
        );
        menu.press(Mouse::Middle, Some(0), false, false);
        assert!(menu.queued.is_empty());
        menu.creative = true;
        menu.press(Mouse::Middle, Some(0), false, false);
        assert_eq!(
            menu.queued,
            [MenuInput::Click {
                slot: 0,
                button: 2,
                kind: ContainerInput::Clone
            }]
        );
        assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(64));
    }

    #[test]
    fn an_answer_ends_the_batch_and_a_tick_keeps_it_shown() {
        let mut menu = chest(
            vec![Some(stack("minecraft:stone", 4))],
            Inventory::default(),
        );
        menu.press(Mouse::Left, Some(0), false, false);
        menu.in_flight = Some((1, std::mem::take(&mut menu.queued)));
        let own = |slots: Vec<Option<ItemStack>>| {
            let mut slots = slots;
            slots.resize(27, None);
            slots
        };
        // A tick's update before the batch was seen: no player part, so
        // the batch is still out and still shown worked out.
        let pushed = MenuUpdate {
            id: 1,
            ack: 0,
            slots: own(vec![Some(stack("minecraft:stone", 4))]),
            ..MenuUpdate::default()
        };
        assert_eq!(menu.update(pushed, &Inventory::default()), Updated::Shown);
        assert!(menu.in_flight.is_some());
        assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(4));
        // The answer: the inventory has its part written, the batch is done.
        let mut inventory = Inventory::default();
        inventory.cursor = Some(stack("minecraft:stone", 4));
        let answer = MenuUpdate {
            id: 1,
            ack: 1,
            slots: own(Vec::new()),
            cursor: Some(inventory.cursor.clone()),
            ..MenuUpdate::default()
        };
        assert_eq!(menu.update(answer, &inventory), Updated::Shown);
        assert!(menu.in_flight.is_none());
        assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(4));
        assert!(menu.item(0).is_none());
        // The server closed the container; then the menu is gone.
        let closing = MenuUpdate {
            id: 1,
            ack: 1,
            slots: own(Vec::new()),
            closing: true,
            ..MenuUpdate::default()
        };
        assert_eq!(menu.update(closing, &inventory), Updated::Closing);
        let closed = MenuUpdate {
            id: 1,
            ack: 2,
            closed: true,
            cursor: Some(None),
            ..MenuUpdate::default()
        };
        assert_eq!(menu.update(closed, &Inventory::default()), Updated::Closed);
    }

    #[test]
    fn between_batches_a_pickup_shows_at_once() {
        // `ItemEntity.playerTouch` fills the inventory whatever screen is
        // open: with no batch out the open chest's inventory rows show what
        // the player picked up.
        let mut menu = chest(
            vec![Some(stack("minecraft:stone", 4))],
            Inventory::default(),
        );
        assert!(!menu.waits());
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(stack("minecraft:bone", 3));
        menu.predict(&inventory);
        assert_eq!(menu.item(27 + 27).map(|s| s.id.as_str()), Some("minecraft:bone"));
        assert_eq!(menu.item(0).map(|s| s.count), Some(4));
        // A click makes the inventory wait for its answer, while queued and
        // while with the server.
        menu.press(Mouse::Left, Some(0), false, false);
        assert!(menu.waits());
        menu.in_flight = Some((1, std::mem::take(&mut menu.queued)));
        assert!(menu.waits());
    }

    #[test]
    fn the_copies_start_over_from_each_update() {
        let mut menu = chest(
            vec![Some(stack("minecraft:stone", 4))],
            Inventory::default(),
        );
        menu.press(Mouse::Left, Some(0), false, false);
        menu.in_flight = Some((1, std::mem::take(&mut menu.queued)));
        // A hopper fed the chest before the batch was seen: the screen
        // shows that, with the click still worked out on top.
        let mut slots = vec![
            Some(stack("minecraft:stone", 4)),
            Some(stack("minecraft:dirt", 1)),
        ];
        slots.resize(27, None);
        menu.slots = slots;
        menu.predict(&Inventory::default());
        assert!(menu.item(0).is_none());
        assert_eq!(menu.item(1).map(|s| s.id.as_str()), Some("minecraft:dirt"));
        assert_eq!(menu.shown.cursor.as_ref().map(|s| s.count), Some(4));
    }

    #[test]
    fn a_click_on_an_empty_crafter_slot_toggles_it_first() {
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(stack("minecraft:stick", 1));
        let mut menu = screen(MenuKind::Crafter, vec![None; 10], inventory);
        menu.data = vec![0; 10];
        menu.press(Mouse::Left, Some(4), false, false);
        let disable = MenuInput::SlotState {
            slot: 4,
            enabled: false,
        };
        assert_eq!(
            menu.queued,
            [
                disable,
                MenuInput::Click {
                    slot: 4,
                    button: 0,
                    kind: ContainerInput::Pickup
                }
            ]
        );
        assert_eq!(menu.sounds, [("minecraft:ui.button.click", 0.4, 0.75)]);
        assert_eq!(menu.menu.data()[4], 1, "the copy shows it at once");
        // The hotbar key over it enables it again, and the stick goes in.
        menu.key(Key::Hotbar(0), Some(4), false);
        assert_eq!(
            menu.queued[2],
            MenuInput::SlotState {
                slot: 4,
                enabled: true
            }
        );
        assert_eq!(menu.sounds[1].2, 1.0);
        assert_eq!(menu.item(4), Some(&stack("minecraft:stick", 1)));
        // An answer that has not seen the toggles keeps them on the copies.
        let inventory = menu.shown.clone();
        menu.predict(&inventory);
        assert_eq!(menu.menu.data()[4], 0);
    }

    #[test]
    fn a_picked_trade_fills_the_payments_on_the_copies() {
        use minecraftoss_entities::merchant::Offers;
        use minecraftoss_entities::trading::{ItemCost, MerchantOffer, TradeItem};
        let offer = MerchantOffer {
            buy: ItemCost {
                id: "minecraft:wheat".into(),
                count: 20,
                components: None,
            },
            buy_b: None,
            sell: TradeItem {
                id: "minecraft:emerald".into(),
                count: 1,
                components: Default::default(),
            },
            uses: 0,
            max_uses: 16,
            reward_exp: true,
            special_price: 0,
            demand: 0,
            price_multiplier: 0.05,
            xp: 2,
        };
        let offers = Offers {
            offers: vec![offer],
            level: 1,
            show_progress: true,
            can_restock: true,
            ..Offers::default()
        };
        let mut inventory = Inventory::default();
        inventory.slots[9] = Some(stack("minecraft:wheat", 25));
        let mut menu = screen(MenuKind::Merchant, vec![None; 3], inventory.clone());
        menu.extra = Some(MenuExtra::Merchant(offers.clone()));
        menu.predict(&inventory);
        // `postButtonClick`: the pick is worked out at once, and sent.
        menu.send(MenuInput::SelectTrade(0));
        assert_eq!(menu.queued, [MenuInput::SelectTrade(0)]);
        assert_eq!(menu.item(0).map(|s| s.count), Some(25));
        assert_eq!(
            menu.item(2).map(|s| s.id.as_str()),
            Some("minecraft:emerald")
        );
        assert!(menu.shown.slots[9].is_none());
        let trader = merchant(menu.menu.as_ref()).expect("a trading menu");
        assert_eq!(trader.future_xp(), 2);
        // The server's answer: its offers come with its slots.
        menu.in_flight = Some((1, std::mem::take(&mut menu.queued)));
        let mut answered = inventory.clone();
        answered.slots[9] = None;
        let update = MenuUpdate {
            id: 1,
            ack: 1,
            slots: vec![
                Some(stack("minecraft:wheat", 25)),
                None,
                Some(stack("minecraft:emerald", 1)),
            ],
            extra: Some(MenuExtra::Merchant(Offers { xp: 9, ..offers })),
            cursor: Some(None),
            ..MenuUpdate::default()
        };
        assert_eq!(menu.update(update, &answered), Updated::Shown);
        let trader = merchant(menu.menu.as_ref()).expect("a trading menu");
        assert_eq!((trader.offers().xp, trader.future_xp()), (9, 2));
        assert_eq!(menu.item(0).map(|s| s.count), Some(25));
    }

    #[test]
    fn an_enchanting_row_sends_its_button_when_the_copy_would_take_it() {
        let own = vec![
            Some(stack("minecraft:diamond_sword", 1)),
            Some(stack("minecraft:lapis_lazuli", 2)),
        ];
        let mut menu = screen(MenuKind::Enchantment, own, Inventory::default());
        for (id, cost) in [(0, 5), (1, 10), (2, 30)] {
            menu.menu.set_data(id, cost);
        }
        menu.xp_level = 12;
        // Rows of 108 by 19 from (60, 14); the third costs too much.
        assert!(menu.press_enchanting_row(60.0, 14.0));
        assert!(menu.press_enchanting_row(167.5, 51.5));
        assert!(!menu.press_enchanting_row(100.0, 60.0));
        assert!(!menu.press_enchanting_row(59.0, 20.0));
        assert_eq!(menu.queued, [MenuInput::Button(0), MenuInput::Button(1)]);
        // Creative pays nothing.
        menu.creative = true;
        assert!(menu.press_enchanting_row(100.0, 60.0));
        // Another kind's screen has no rows.
        let mut chest = chest(Vec::new(), Inventory::default());
        assert!(!chest.press_enchanting_row(100.0, 20.0));
    }
}
