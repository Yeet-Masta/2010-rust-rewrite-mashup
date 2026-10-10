//! `CrafterScreen` over its background: a disabled grid slot drawn as its
//! sprite in the slot's place, the redstone that shows whether the crafter
//! is powered, and the tooltip over an enabled empty grid slot. The toggles
//! themselves are the screen's inputs (`menu_screen`); the result is the
//! menu's own slot, which is never highlighted. The pointing hand over the
//! grid is not shown: the game keeps the one cursor.
use minecraft_terrain::menus::crafter;
use minecraftoss_player::menu::crafter::{CrafterMenu, GRID};

use super::MenuView;
use crate::gui::Gui;
use crate::render::UiList;

/// `isSlotDisabled` for the menu shown.
fn disabled(view: &MenuView<'_>, index: usize) -> bool {
    crafter(view.menu).is_some_and(|menu| menu.is_slot_disabled(index))
}

impl Gui {
    /// `extractRedstone`: the redstone at (`width / 2 + 9`,
    /// `height / 2 - 48`), which is (97, 35) from the window's corner, lit
    /// while the crafter is triggered.
    pub(super) fn crafter_extras(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let sprite = if crafter(view.menu).is_some_and(CrafterMenu::is_powered) {
            "crafter_powered_redstone"
        } else {
            "crafter_unpowered_redstone"
        };
        self.sprite(ui, sprite, left + 97.0, top + 35.0, 16.0, 16.0);
    }

    /// `extractSlot` for a disabled grid slot (`extractDisabledSlot`): the
    /// 18 by 18 `disabled_slot` a pixel out from the slot, in its place.
    pub(super) fn crafter_slot(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        index: usize,
        left: f32,
        top: f32,
    ) -> bool {
        if !disabled(view, index) {
            return false;
        }
        let slot = &view.slots[index];
        let (x, y) = (left + slot.x as f32 - 1.0, top + slot.y as f32 - 1.0);
        self.sprite(ui, "crafter_disabled_slot", x, y, 18.0, 18.0);
        true
    }

    /// `gui.togglable_slot` over an enabled, empty grid slot while nothing
    /// is carried.
    pub(super) fn crafter_tooltip(&mut self, ui: &mut UiList, view: &MenuView<'_>) {
        let Some(index) = view.hovered.filter(|&index| index < GRID) else {
            return;
        };
        if disabled(view, index)
            || view.inventory.cursor.is_some()
            || view.slots[index].stack.is_some()
        {
            return;
        }
        let text = crate::creative::translate(&self.language, "gui.togglable_slot", &[]);
        self.tooltip(ui, &text);
    }
}
