//! `StonecutterScreen` over its background: the recipes for the input as
//! 4 by 3 buttons (selected, under the mouse, or plain) with their results,
//! scrolled by the wheel or the scroller, which a press in its column
//! holds; a press on a button selects its recipe (`clickMenuButton`); the
//! hovered result's tooltip.
use minecraft_terrain::pack::PackStack;
use minecraftoss_player::inventory::ItemStack;

use super::MenuView;
use crate::gui::Gui;
use crate::render::UiList;

/// The buttons' columns and visible rows, and their size.
const COLUMNS: usize = 4;
const ROWS: usize = 3;
const BUTTON: (f32, f32) = (16.0, 18.0);
/// The grid's corner (`leftPos + 52`, `topPos + 14`).
const GRID: (f32, f32) = (52.0, 14.0);
/// The scroller's column and its travel (`41` pixels below `y = 15`).
const SCROLLER: (f32, f32) = (119.0, 15.0);
const TRAVEL: f32 = 41.0;

/// The screen's list: `scrollOffs`, `startIndex`, whether the scroller is
/// held, `displayRecipes`, and the input it last saw (`containerChanged`).
#[derive(Clone, Debug, Default)]
pub struct RecipeList {
    scroll: f32,
    start: usize,
    scrolling: bool,
    display: bool,
    input: Option<ItemStack>,
}

impl RecipeList {
    /// `containerChanged`: any change to the input shows its recipes, if it
    /// has any, from the top.
    pub fn watch(&mut self, input: Option<&ItemStack>, recipes: usize) {
        if self.input.as_ref() == input {
            return;
        }
        self.input = input.cloned();
        self.display = input.is_some() && recipes > 0;
        self.scroll = 0.0;
        self.start = 0;
    }

    /// `isScrollBarActive`.
    fn active(&self, recipes: usize) -> bool {
        self.display && recipes > COLUMNS * ROWS
    }

    /// `getOffscreenRows`.
    fn offscreen_rows(recipes: usize) -> usize {
        recipes.div_ceil(COLUMNS).saturating_sub(ROWS)
    }

    /// `startIndex` for the scroll.
    fn update_start(&mut self, recipes: usize) {
        let rows = Self::offscreen_rows(recipes) as f32;
        self.start = (self.scroll * rows + 0.5) as usize * COLUMNS;
    }

    /// `mouseScrolled`: each notch a row's share of the travel.
    pub fn wheel(&mut self, notches: i32, recipes: usize) {
        if notches == 0 || !self.active(recipes) {
            return;
        }
        let rows = Self::offscreen_rows(recipes) as f32;
        self.scroll = (self.scroll - notches as f32 / rows).clamp(0.0, 1.0);
        self.update_start(recipes);
    }

    /// `mouseClicked` at (x, y) from the window's corner: the recipe button
    /// under it (any of the 12 cells, recipe or not), or a press in the
    /// scroller's column, which holds it.
    pub fn press(&mut self, x: f32, y: f32) -> Option<i32> {
        if !self.display {
            return None;
        }
        for cell in 0..COLUMNS * ROWS {
            let (cx, cy) = cell_corner(cell);
            if x >= cx && y >= cy && x < cx + BUTTON.0 && y < cy + BUTTON.1 {
                return Some((self.start + cell) as i32);
            }
        }
        if (SCROLLER.0..SCROLLER.0 + 12.0).contains(&x) && (9.0..63.0).contains(&y) {
            self.scrolling = true;
        }
        None
    }

    /// `mouseDragged`: a held scroller follows the mouse's y.
    pub fn drag(&mut self, y: f32, recipes: usize) {
        if !self.scrolling || !self.active(recipes) {
            return;
        }
        let top = GRID.1;
        self.scroll = ((y - top - 7.5) / (54.0 - 15.0)).clamp(0.0, 1.0);
        self.update_start(recipes);
    }

    /// `mouseReleased`.
    pub fn release(&mut self) {
        self.scrolling = false;
    }
}

/// A cell's corner in the grid, `k` of the visible 12.
fn cell_corner(cell: usize) -> (f32, f32) {
    (
        GRID.0 + (cell % COLUMNS) as f32 * BUTTON.0,
        GRID.1 + (cell / COLUMNS) as f32 * BUTTON.1,
    )
}

/// The visible recipes: index, and where its result is drawn (`posY` is
/// two below the cell).
fn visible(list: &RecipeList, recipes: usize) -> impl Iterator<Item = (usize, f32, f32)> {
    (list.start..(list.start + COLUMNS * ROWS).min(recipes)).map(move |index| {
        let (x, y) = cell_corner(index - list.start);
        (index, x, y + 2.0)
    })
}

impl Gui {
    /// `StonecutterScreen.extractBackground`: the scroller, the buttons and
    /// their results.
    pub(super) fn stonecutter_extras(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let list = view.recipe_list;
        let count = view.recipes.len();
        let sprite = if list.active(count) {
            "stonecutter_scroller"
        } else {
            "stonecutter_scroller_disabled"
        };
        let offset = (TRAVEL * list.scroll).trunc();
        self.sprite(
            ui,
            sprite,
            left + SCROLLER.0,
            top + SCROLLER.1 + offset,
            12.0,
            15.0,
        );
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        let selected = view.data.first().copied().unwrap_or(-1);
        for (index, x, y) in visible(list, count) {
            let sprite = if index as i32 == selected {
                "stonecutter_recipe_selected"
            } else if mx >= x && my >= y && mx < x + BUTTON.0 && my < y + BUTTON.1 {
                "stonecutter_recipe_highlighted"
            } else {
                "stonecutter_recipe"
            };
            self.sprite(ui, sprite, left + x, top + y - 1.0, BUTTON.0, BUTTON.1);
        }
        for (index, x, y) in visible(list, count) {
            self.item(
                ui,
                packs,
                view.inventory,
                &view.recipes[index],
                left + x,
                top + y,
            );
        }
    }

    /// `extractTooltip`: the result of the recipe under the mouse.
    pub(super) fn stonecutter_tooltip(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let list = view.recipe_list;
        if !list.display {
            return;
        }
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        let hovered = visible(list, view.recipes.len())
            .find(|&(_, x, y)| mx >= x && my >= y && mx < x + BUTTON.0 && my < y + BUTTON.1);
        if let Some((index, _, _)) = hovered {
            self.stack_tooltip(ui, &view.recipes[index], false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_scrolls_by_rows_and_its_cells_take_presses() {
        let mut list = RecipeList::default();
        let stone = ItemStack::new("minecraft:stone", 1);
        list.watch(Some(&stone), 30);
        // 30 recipes: 8 rows, 5 of them off screen.
        list.wheel(-1, 30);
        assert_eq!((list.scroll, list.start), (0.2, 4));
        assert_eq!(list.press(52.0 + 16.0, 14.0 + 18.0), Some(4 + 5));
        // A press in the scroller's column holds it, and it follows the
        // mouse to the end.
        assert_eq!(list.press(120.0, 10.0), None);
        list.drag(14.0 + 7.5 + 39.0, 30);
        assert_eq!((list.scroll, list.start), (1.0, 20));
        list.release();
        list.drag(14.0 + 7.5, 30);
        assert_eq!(list.start, 20, "let go, it stays");
        // A change to the input starts over.
        list.watch(Some(&ItemStack::new("minecraft:stone", 2)), 30);
        assert_eq!(list.start, 0);
        list.watch(None, 0);
        assert_eq!(list.press(53.0, 15.0), None, "no recipes, no buttons");
    }
}
