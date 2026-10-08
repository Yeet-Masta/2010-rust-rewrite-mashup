//! The creative screen (`CreativeModeInventoryScreen` and its
//! `ItemPickerMenu`): its tabs, scrolling, the search box, creative mode's
//! own click rules, the survival inventory tab with its bin, and the saved
//! hotbars (`HotbarManager`).
use std::path::PathBuf;
use std::time::Instant;

use minecraftoss_player::inventory::ItemStack;
use serde_json::{Value, json};

use super::{Game, Input, Key, take_notches};
use crate::creative::{self, Kind};
use crate::gui::{CreativeView, Gui, Slot};

/// `EditBox.setMaxLength(50)`.
const SEARCH_LENGTH: usize = 50;

pub(super) struct CreativeScreen {
    /// `selectedTab`, kept from one opening to the next.
    tab: usize,
    /// `ItemPickerMenu.items`; the hotbar tab has gaps.
    items: Vec<Option<ItemStack>>,
    /// `scrollOffs`, and whether the scroller is held.
    scroll: f32,
    scrolling: bool,
    search: String,
    /// When the search box took focus, for its cursor's blink.
    focused: Instant,
    /// A key did something else, so the character it types is not typed.
    ignore_text: bool,
    /// The nine saved hotbars, nine stacks each; none saved yet is empty.
    hotbars: Vec<Vec<Option<ItemStack>>>,
    /// `hotbar.json` in the game's folder.
    hotbar_file: Option<PathBuf>,
}

impl CreativeScreen {
    pub(super) fn new(save_dir: Option<&std::path::Path>) -> Self {
        // `<data>/saves/<world>` keeps them in `<data>`, as `.minecraft`
        // keeps `hotbar.nbt`.
        let hotbar_file = save_dir
            .and_then(|dir| dir.parent()?.parent())
            .map(|data| data.join("hotbar.json"));
        let mut hotbars = vec![Vec::new(); 9];
        if let Some(saved) = hotbar_file
            .as_ref()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            && let Some(rows) = saved.as_array()
        {
            for (row, cells) in hotbars.iter_mut().zip(rows) {
                *row = cells
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .take(9)
                    .map(|cell| {
                        let id = cell["id"].as_str()?;
                        let mut stack =
                            ItemStack::new(id, cell["count"].as_u64()?.clamp(1, 99) as u8);
                        stack.max = cell["max"].as_u64().unwrap_or(64).clamp(1, 99) as u8;
                        stack.components = cell.get("components").cloned();
                        Some(stack)
                    })
                    .collect();
            }
        }
        Self {
            tab: creative::data().default_tab(),
            items: Vec::new(),
            scroll: 0.0,
            scrolling: false,
            search: String::new(),
            focused: Instant::now(),
            ignore_text: false,
            hotbars,
            hotbar_file,
        }
    }

    fn kind(&self) -> Kind {
        creative::data().tabs[self.tab].kind
    }

    /// `ItemPickerMenu.calculateRowCount`: rows past the first five.
    fn rows(&self) -> i64 {
        self.items.len().div_ceil(9) as i64 - 5
    }

    /// `getRowIndexForScroll`.
    fn row(&self) -> usize {
        ((self.scroll * self.rows() as f32 + 0.5) as i64).max(0) as usize
    }

    /// `canScroll`: the tab has a scroller and more than a screen.
    fn can_scroll(&self) -> bool {
        creative::data().tabs[self.tab].scroll_bar && self.items.len() > 45
    }

    fn write_hotbars(&self) {
        let Some(file) = self.hotbar_file.as_ref() else {
            return;
        };
        let rows: Vec<Value> = self
            .hotbars
            .iter()
            .map(|row| {
                Value::Array(
                    row.iter()
                        .map(|cell| match cell {
                            Some(stack) => json!({
                                "id": stack.id,
                                "count": stack.count,
                                "max": stack.max,
                                "components": stack.components,
                            }),
                            None => Value::Null,
                        })
                        .collect(),
                )
            })
            .collect();
        if let Err(error) = std::fs::write(file, Value::Array(rows).to_string()) {
            log!("Could not save the hotbars: {error}");
        }
    }
}

/// `StringUtil.isAllowedChatCharacter`.
fn allowed(c: char) -> bool {
    c != '§' && c >= ' ' && c != '\u{7f}'
}

impl Game {
    /// A tab's stack, at one.
    fn creative_stack(&self, entry: &creative::Entry) -> ItemStack {
        let mut stack = self.entities.stack(&entry.id, 1);
        stack.components = entry.components.clone();
        stack
    }

    /// `init`: the screen reopens on its last tab.
    pub(super) fn open_creative(&mut self, gui: &Gui) {
        let data = creative::data();
        let mut tab = self.creative_screen.tab;
        if !data.shown(false).contains(&tab) {
            tab = data.default_tab();
        }
        self.creative_screen.tab = data.default_tab();
        self.select_tab(tab, gui);
    }

    /// `selectTab`.
    fn select_tab(&mut self, tab: usize, gui: &Gui) {
        let data = creative::data();
        let old = std::mem::replace(&mut self.creative_screen.tab, tab);
        self.drag = None;
        self.creative_screen.items = match data.tabs[tab].kind {
            Kind::Hotbar => {
                let mut items = Vec::new();
                for (index, row) in self.creative_screen.hotbars.iter().enumerate() {
                    if row.iter().all(Option::is_none) {
                        // A locked paper saying how to save the hotbar.
                        let language = gui.language();
                        let key = |name: &str| creative::translate(language, name, &[]);
                        let info = creative::translate(
                            language,
                            "inventory.hotbarInfo",
                            &[
                                key("key.keyboard.c"),
                                key(&format!("key.keyboard.{}", index + 1)),
                            ],
                        );
                        let mut paper = self.entities.stack("minecraft:paper", 1);
                        paper.components = Some(json!({
                            "minecraft:item_name": info,
                            "minecraft:creative_slot_lock": {},
                        }));
                        items.extend((0..9).map(|i| (i == index).then(|| paper.clone())));
                    } else {
                        items.extend((0..9).map(|i| row.get(i).cloned().flatten()));
                    }
                }
                items
            }
            Kind::Category => data.tabs[tab]
                .items
                .iter()
                .map(|entry| Some(self.creative_stack(entry)))
                .collect(),
            Kind::Search | Kind::Inventory => Vec::new(),
        };
        if data.tabs[tab].kind == Kind::Search {
            self.creative_screen.focused = Instant::now();
            if old != tab {
                self.creative_screen.search.clear();
            }
            self.refresh_search(gui);
        } else {
            self.creative_screen.search.clear();
        }
        self.creative_screen.scroll = 0.0;
    }

    /// `refreshSearchResults`: the search tab's stacks that the text finds.
    fn refresh_search(&mut self, gui: &Gui) {
        let text = self.creative_screen.search.to_lowercase();
        let entries = creative::data().search_entries(false);
        self.creative_screen.items = entries
            .into_iter()
            .map(|entry| self.creative_stack(entry))
            .filter(|stack| text.is_empty() || creative::matches(gui.language(), stack, &text))
            .map(Some)
            .collect();
        self.creative_screen.scroll = 0.0;
    }

    /// Whether the creative screen shows the survival inventory tab.
    pub(super) fn creative_inventory_tab(&self) -> bool {
        self.screen == crate::gui::Screen::Creative
            && self.creative_screen.kind() == Kind::Inventory
    }

    /// What `gui.creative_screen` draws.
    pub(super) fn creative_view(&self) -> (Vec<usize>, CreativeView<'_>) {
        let screen = &self.creative_screen;
        let blink = (screen.focused.elapsed().as_millis() / 300).is_multiple_of(2);
        let view = CreativeView {
            tab: screen.tab,
            tabs: &[],
            inventory: &self.entities.inventory,
            items: &screen.items,
            row: screen.row(),
            scroll: screen.scroll,
            can_scroll: screen.can_scroll(),
            search: (screen.kind() == Kind::Search).then_some((screen.search.as_str(), blink)),
        };
        (creative::data().shown(false), view)
    }

    /// The creative screen's keys; true when one was used.
    pub(super) fn creative_key(&mut self, key: Key, gui: &Gui, ctrl: bool) -> bool {
        let data = creative::data();
        let kind = self.creative_screen.kind();
        let inventory_tab = kind == Kind::Inventory;
        let hovered = gui.creative_slot_at(inventory_tab).0;
        let ignore = std::mem::take(&mut self.creative_screen.ignore_text);
        let hovered_stack = |game: &Self| match hovered {
            Some(Slot::Creative(cell)) => game
                .creative_screen
                .items
                .get(game.creative_screen.row() * 9 + cell)
                .cloned()
                .flatten(),
            Some(Slot::Inventory(index)) => game.entities.inventory.slots[index].clone(),
            _ => None,
        };
        match key {
            Key::Chat if kind != Kind::Search => {
                self.creative_screen.ignore_text = true;
                self.select_tab(data.search_tab(), gui);
            }
            Key::Hotbar(slot) => {
                // `checkHotbarKeyPressed`; in the search tab, only over a
                // stack or the player's own slots, else the digit is typed.
                let swap = kind != Kind::Search
                    || !matches!(hovered, Some(Slot::Creative(_)))
                    || hovered_stack(self).is_some();
                if swap && self.entities.inventory.cursor.is_none() && hovered.is_some() {
                    self.creative_screen.ignore_text = kind == Kind::Search;
                    match hovered {
                        Some(Slot::Creative(_)) => {
                            if let Some(stack) = hovered_stack(self) {
                                let full = ItemStack {
                                    count: stack.max,
                                    ..stack
                                };
                                let inventory = &mut self.entities.inventory;
                                inventory.creative_take(full, false, Some(slot));
                            }
                        }
                        Some(Slot::Inventory(index)) => {
                            self.entities.inventory.number_swap(index, slot)
                        }
                        _ => {}
                    }
                }
            }
            // The search box keeps letters for itself.
            Key::Inventory | Key::Drop if kind == Kind::Search => {}
            Key::Drop => {
                // `THROW` over a stack: a creative stack's copy, or the
                // slot's own items.
                match hovered {
                    Some(Slot::Creative(_)) => {
                        if let Some(stack) = hovered_stack(self) {
                            let count = if ctrl { stack.max } else { 1 };
                            self.throw(vec![ItemStack { count, ..stack }]);
                        }
                    }
                    Some(Slot::Inventory(index)) if self.entities.inventory.cursor.is_none() => {
                        let dropped = self.entities.inventory.drop_selected(index, ctrl);
                        self.throw(dropped.into_iter().collect());
                    }
                    _ => {}
                }
            }
            Key::Char(c) if kind == Kind::Search => {
                if !ignore
                    && allowed(c)
                    && self.creative_screen.search.chars().count() < SEARCH_LENGTH
                {
                    self.creative_screen.search.push(c);
                    self.refresh_search(gui);
                }
            }
            Key::Backspace if kind == Kind::Search => {
                let before = self.creative_screen.search.len();
                if ctrl {
                    // `deleteWords(-1)`: back over spaces, then the word.
                    let text = self.creative_screen.search.trim_end_matches(' ');
                    let keep = text.rfind(' ').map_or(0, |at| at + 1);
                    self.creative_screen.search.truncate(keep);
                } else {
                    self.creative_screen.search.pop();
                }
                if self.creative_screen.search.len() != before {
                    self.refresh_search(gui);
                }
            }
            _ => return false,
        }
        true
    }

    /// The creative screen's mouse: tabs, the scroller and the slots.
    pub(super) fn creative_input(&mut self, input: &mut Input, gui: &Gui) {
        let data = creative::data();
        let kind = self.creative_screen.kind();
        let inventory_tab = kind == Kind::Inventory;
        let notches = take_notches(&mut input.scroll);
        if notches != 0 && self.creative_screen.can_scroll() {
            // `subtractInputFromScroll`.
            let rows = self.creative_screen.rows() as f32;
            self.creative_screen.scroll =
                (self.creative_screen.scroll - notches as f32 / rows).clamp(0.0, 1.0);
        }
        if self.creative_screen.scrolling {
            self.creative_screen.scroll = gui.creative_scroll_at_mouse();
        }
        let shown = data.shown(false);
        let on_tab = gui.creative_tab_at(&shown);
        let (slot, outside) = gui.creative_slot_at(inventory_tab);
        if input.middle_click {
            self.creative_clone(slot);
        }
        // A drag over the player's slots, as on the survival screen.
        if let Some((_, slots)) = self.drag.as_mut()
            && let Some(slot @ Slot::Inventory(_)) = slot
            && !slots.contains(&slot)
            && let Some(carried) = self.entities.inventory.cursor.as_ref()
            && slots.len() < usize::from(carried.count)
            && super::drag_accepts(&self.entities.inventory, slot, carried)
        {
            slots.push(slot);
        }
        for (right, pressed) in input.clicks.drain(..).collect::<Vec<_>>() {
            if !right && pressed {
                if on_tab.is_some() {
                    continue;
                }
                if !inventory_tab && gui.creative_in_scrollbar() {
                    self.creative_screen.scrolling = self.creative_screen.can_scroll();
                    continue;
                }
            }
            if !right && !pressed {
                self.creative_screen.scrolling = false;
                if let Some(tab) = on_tab {
                    self.drag = None;
                    self.select_tab(tab, gui);
                    return;
                }
            }
            if pressed {
                self.creative_press(slot, outside && on_tab.is_none(), right, input.shift);
            } else if let Some((drag_right, slots)) = self.drag.take()
                && drag_right == right
            {
                self.release_drag(slots, right, false);
            }
        }
    }

    /// `CLONE`: a full stack of what is under the mouse, into an empty hand.
    fn creative_clone(&mut self, slot: Option<Slot>) {
        if self.entities.inventory.cursor.is_some() {
            return;
        }
        let stack = match slot {
            Some(Slot::Creative(cell)) => self
                .creative_screen
                .items
                .get(self.creative_screen.row() * 9 + cell)
                .cloned()
                .flatten()
                .filter(|stack| !locked(stack)),
            Some(Slot::Inventory(index)) => self.entities.inventory.slots[index].clone(),
            _ => None,
        };
        if let Some(stack) = stack {
            self.entities.inventory.cursor = Some(ItemStack {
                count: stack.max,
                ..stack
            });
        }
    }

    /// `slotClicked` on the creative screen.
    fn creative_press(&mut self, slot: Option<Slot>, outside: bool, right: bool, shift: bool) {
        let inventory_tab = self.creative_screen.kind() == Kind::Inventory;
        match slot {
            Some(Slot::Destroy) => {
                let inventory = &mut self.entities.inventory;
                if shift {
                    // Shift on the bin empties the whole inventory.
                    for slot in inventory.slots.iter_mut() {
                        *slot = None;
                    }
                    inventory.crafting = Default::default();
                } else {
                    inventory.cursor = None;
                }
            }
            Some(Slot::Creative(cell)) => {
                let clicked = self
                    .creative_screen
                    .items
                    .get(self.creative_screen.row() * 9 + cell)
                    .cloned()
                    .flatten();
                if clicked.as_ref().is_some_and(locked) {
                    return;
                }
                let inventory = &mut self.entities.inventory;
                match (inventory.cursor.as_mut(), clicked) {
                    (Some(carried), Some(clicked)) if carried.same_item(&clicked) => {
                        if !right {
                            if shift {
                                carried.count = carried.max;
                            } else if carried.count < carried.max {
                                carried.count += 1;
                            }
                        } else {
                            carried.count -= 1;
                            if carried.count == 0 {
                                inventory.cursor = None;
                            }
                        }
                    }
                    (None, Some(clicked)) => {
                        let count = if shift { clicked.max } else { 1 };
                        inventory.cursor = Some(ItemStack { count, ..clicked });
                    }
                    (Some(_), _) if !right => inventory.cursor = None,
                    (Some(carried), _) => {
                        carried.count -= 1;
                        if carried.count == 0 {
                            inventory.cursor = None;
                        }
                    }
                    (None, None) => {}
                }
            }
            // `ItemPickerMenu.quickMoveStack`: shift on the hotbar row
            // outside the inventory tab clears the slot.
            Some(Slot::Inventory(index)) if shift && !inventory_tab => {
                self.entities.inventory.slots[index] = None;
            }
            Some(slot @ Slot::Inventory(_)) => self.press_slot(Some(slot), false, right, shift),
            None if outside => {
                let inventory = &mut self.entities.inventory;
                let thrown = if inventory_tab {
                    inventory.click(None, right, false)
                } else if right {
                    // A category tab drops one of the carried stack.
                    inventory.cursor.as_mut().map(|carried| {
                        carried.count -= 1;
                        ItemStack {
                            count: 1,
                            ..carried.clone()
                        }
                    })
                } else {
                    inventory.cursor.take()
                };
                if inventory.cursor.as_ref().is_some_and(|c| c.count == 0) {
                    inventory.cursor = None;
                }
                self.throw(thrown.into_iter().collect());
            }
            _ => {}
        }
    }

    /// `handleHotbarLoadOrSave`: C with a number saves the hotbar, X with
    /// one loads it back.
    pub(super) fn hotbar_keys(&mut self, slot: usize, input: &Input) -> bool {
        if !self.creative || !(input.save_hotbar || input.load_hotbar) {
            return false;
        }
        let inventory = &mut self.entities.inventory;
        if input.load_hotbar {
            let row = self.creative_screen.hotbars[slot].clone();
            for i in 0..9 {
                inventory.slots[i] = row.get(i).cloned().flatten();
            }
        } else {
            self.creative_screen.hotbars[slot] = inventory.slots[..9].to_vec();
            self.creative_screen.write_hotbars();
        }
        true
    }
}

/// `DataComponents.CREATIVE_SLOT_LOCK`: the hotbar tab's placeholders.
fn locked(stack: &ItemStack) -> bool {
    stack
        .components
        .as_ref()
        .is_some_and(|parts| parts.get("minecraft:creative_slot_lock").is_some())
}
