//! `LoomScreen` over its background: the patterns the inputs can weave as
//! 4 by 4 buttons (selected, under the mouse, or plain), each with a small
//! banner of its pattern, scrolled by the wheel or the scroller, which a
//! press in its column holds; a press on a button selects its pattern
//! (`clickMenuButton`); the result's flag as it will look, or the error
//! sprite when the banner is full; the hovered pattern's name.
use minecraft_terrain::pack::PackStack;
use minecraft_terrain::special_icon;
use minecraftoss_player::inventory::ItemStack;
use minecraftoss_player::menu::loom::{self, DYE, MAX_LAYERS, RESULT};

use super::MenuView;
use crate::gui::Gui;
use crate::render::UiList;

/// The buttons' columns and visible rows, and their size.
const COLUMNS: usize = 4;
const ROWS: usize = 4;
const BUTTON: f32 = 14.0;
/// The grid's corner (`leftPos + 60`, `topPos + 13`).
const GRID: (f32, f32) = (60.0, 13.0);
/// The scroller's column and its travel (`41` pixels below `y = 13`).
const SCROLLER: (f32, f32) = (119.0, 13.0);
const TRAVEL: f32 = 41.0;

/// The screen's list: `scrollOffs`, `startRow`, whether the scroller is
/// held, `displayPatterns`, `hasMaxPatterns`, and the inputs it last saw
/// (`containerChanged`).
#[derive(Clone, Debug, Default)]
pub struct PatternList {
    scroll: f32,
    start_row: usize,
    scrolling: bool,
    display: bool,
    has_max: bool,
    inputs: [Option<ItemStack>; 3],
}

impl PatternList {
    /// `containerChanged`: the banner full or not, the patterns shown when
    /// an input changed and there are some to weave, and the list from the
    /// top when it got shorter than its scroll.
    pub fn watch(&mut self, inputs: [Option<&ItemStack>; 3], patterns: usize) {
        let [banner, dye, _] = inputs;
        self.has_max = banner.is_some_and(|banner| loom::layer_count(banner) >= MAX_LAYERS);
        let changed = self.inputs.iter().zip(inputs).any(|(old, new)| old.as_ref() != new);
        if changed {
            self.display = banner.is_some() && dye.is_some() && !self.has_max && patterns > 0;
        }
        if self.start_row >= Self::total_rows(patterns) {
            self.start_row = 0;
            self.scroll = 0.0;
        }
        self.inputs = inputs.map(|input| input.cloned());
    }

    /// `totalRowCount`.
    fn total_rows(patterns: usize) -> usize {
        patterns.div_ceil(COLUMNS)
    }

    /// `isScrollBarActive`.
    fn active(&self, patterns: usize) -> bool {
        self.display && patterns > COLUMNS * ROWS
    }

    /// The rows past the visible four, while the patterns show.
    fn offscreen_rows(&self, patterns: usize) -> Option<f32> {
        let rows = Self::total_rows(patterns).saturating_sub(ROWS);
        (self.display && rows > 0).then_some(rows as f32)
    }

    /// `startRow` for the scroll.
    fn update_start(&mut self, rows: f32) {
        self.start_row = (self.scroll * rows + 0.5).max(0.0) as usize;
    }

    /// `mouseScrolled`: each notch a row's share of the travel.
    pub fn wheel(&mut self, notches: i32, patterns: usize) {
        let Some(rows) = self.offscreen_rows(patterns) else {
            return;
        };
        if notches == 0 {
            return;
        }
        self.scroll = (self.scroll - notches as f32 / rows).clamp(0.0, 1.0);
        self.update_start(rows);
    }

    /// `mouseClicked` at (x, y) from the window's corner: the pattern
    /// button under it that the menu takes (`clickMenuButton`: one of the
    /// patterns), or a press in the scroller's column, which holds it.
    pub fn press(&mut self, x: f32, y: f32, patterns: usize) -> Option<i32> {
        if !self.display {
            return None;
        }
        for cell in 0..COLUMNS * ROWS {
            let (cx, cy) = cell_corner(cell);
            let index = self.start_row * COLUMNS + cell;
            if x >= cx && y >= cy && x < cx + BUTTON && y < cy + BUTTON && index < patterns {
                return Some(index as i32);
            }
        }
        if (SCROLLER.0..SCROLLER.0 + 12.0).contains(&x) && (9.0..65.0).contains(&y) {
            self.scrolling = true;
        }
        None
    }

    /// `mouseDragged`: a held scroller follows the mouse's y.
    pub fn drag(&mut self, y: f32, patterns: usize) {
        let Some(rows) = self.offscreen_rows(patterns).filter(|_| self.scrolling) else {
            return;
        };
        self.scroll = ((y - SCROLLER.1 - 7.5) / (56.0 - 15.0)).clamp(0.0, 1.0);
        self.update_start(rows);
    }

    /// `mouseReleased`.
    pub fn release(&mut self) {
        self.scrolling = false;
    }
}

/// A cell's corner in the grid, `k` of the visible 16.
fn cell_corner(cell: usize) -> (f32, f32) {
    (
        GRID.0 + (cell % COLUMNS) as f32 * BUTTON,
        GRID.1 + (cell / COLUMNS) as f32 * BUTTON,
    )
}

/// The visible patterns: index and their cell's corner.
fn visible(list: &PatternList, patterns: usize) -> impl Iterator<Item = (usize, f32, f32)> {
    let first = list.start_row * COLUMNS;
    (first..(first + COLUMNS * ROWS).min(patterns)).map(move |index| {
        let (x, y) = cell_corner(index - first);
        (index, x, y)
    })
}

impl Gui {
    /// `LoomScreen.extractBackground`: the scroller, the result's flag or
    /// the error, and the pattern buttons with their banners.
    pub(super) fn loom_extras(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let list = view.pattern_list;
        let count = view.patterns.len();
        let sprite = if list.active(count) {
            "loom_scroller"
        } else {
            "loom_scroller_disabled"
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
        let stack = |slot: usize| view.slots.get(slot).and_then(|slot| slot.stack.as_ref());
        if let Some(result) = stack(RESULT).filter(|_| !list.has_max)
            && let Some(base) = loom::banner_colour(result)
        {
            // `bannerPattern`: the flag at (141, 8) to (161, 48).
            let pixels = (self.scale as usize).max(1);
            let components = result.components.as_ref();
            let key = format!("{base} {components:?} {pixels}");
            let make = || special_icon::banner_result(packs, base, components, pixels).ok();
            if let Some(texture) = self.picture("loom_result", &key, make) {
                let rect = self.rect(left + 141.0, top + 8.0, 20.0, 40.0);
                ui.quad(texture, rect, [0.0, 0.0, 1.0, 1.0], [1.0; 4]);
            }
        } else if list.has_max {
            self.sprite(ui, "loom_error", left + 138.0, top + 52.0, 26.0, 26.0);
        }
        if !list.display {
            return;
        }
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        let selected = view.data.first().copied().unwrap_or(-1);
        for (index, x, y) in visible(list, count) {
            let sprite = if index as i32 == selected {
                "loom_pattern_selected"
            } else if mx >= x && my >= y && mx < x + BUTTON && my < y + BUTTON {
                "loom_pattern_highlighted"
            } else {
                "loom_pattern"
            };
            self.sprite(ui, sprite, left + x, top + y, BUTTON, BUTTON);
            // `extractBannerOnButton`: the pattern on a grey banner.
            let asset = &view.patterns[index].0;
            let make = || special_icon::loom_pattern(packs, asset).ok().flatten();
            if let Some(texture) = self.picture(&format!("loom_pattern {asset}"), asset, make) {
                let rect = self.rect(left + x + 4.0, top + y + 2.0, 5.0, 10.0);
                ui.quad(texture, rect, [0.0, 0.0, 1.0, 1.0], [1.0; 4]);
            }
        }
    }

    /// The tooltip `extractBackground` sets: the name of the pattern under
    /// the mouse, unless selected, in the dye's colour (white without one).
    pub(super) fn loom_tooltip(&mut self, ui: &mut UiList, view: &MenuView<'_>, left: f32, top: f32) {
        let list = view.pattern_list;
        if !list.display {
            return;
        }
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        let selected = view.data.first().copied().unwrap_or(-1);
        let hovered = visible(list, view.patterns.len())
            .find(|&(_, x, y)| mx >= x && my >= y && mx < x + BUTTON && my < y + BUTTON)
            .filter(|&(index, _, _)| index as i32 != selected);
        let Some((index, _, _)) = hovered else {
            return;
        };
        let dye = view.slots.get(DYE).and_then(|slot| slot.stack.as_ref());
        let colour = dye.and_then(loom::dye_colour).unwrap_or("white");
        let key = format!("{}.{colour}", view.patterns[index].1);
        let text = crate::creative::translate(self.language(), &key, &[]);
        self.tooltip(ui, &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_scrolls_by_rows_and_its_cells_take_presses() {
        let mut list = PatternList::default();
        let banner = ItemStack::new("minecraft:white_banner", 1);
        let dye = ItemStack::new("minecraft:red_dye", 1);
        list.watch([Some(&banner), Some(&dye), None], 32);
        // 32 patterns: 8 rows, 4 of them off screen.
        list.wheel(-1, 32);
        assert_eq!((list.scroll, list.start_row), (0.25, 1));
        assert_eq!(list.press(60.0 + 14.0, 13.0 + 14.0, 32), Some(4 + 5));
        assert_eq!(list.press(60.0, 13.0 + 42.0, 15), None, "past the patterns");
        // A press in the scroller's column holds it, and it follows the
        // mouse to the end.
        assert_eq!(list.press(120.0, 10.0, 32), None);
        list.drag(13.0 + 7.5 + 41.0, 32);
        assert_eq!((list.scroll, list.start_row), (1.0, 4));
        list.release();
        // A pattern item's one pattern: the list fits and starts over.
        list.watch([Some(&banner), Some(&dye), Some(&dye)], 1);
        assert_eq!(list.start_row, 0);
        assert!(!list.active(1));
    }
}
