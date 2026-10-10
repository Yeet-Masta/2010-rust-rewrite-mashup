//! The server menus' screens, drawn as `AbstractContainerScreen` draws
//! them: the world darkened, the kind's background, its title and the
//! inventory's label, then every slot in menu order between the hovered
//! slot's two highlight sprites, a drag's preview in the slots it covers,
//! the carried stack over everything, and the hovered stack's tooltip.
//! Each kind's window (`MenuScreens.register`'s screen) is a line in
//! [`layout`]; a kind with more to draw (progress, buttons) adds it in
//! [`Gui::menu_extras`], and what it shows over everything (its buttons'
//! tooltips) in [`Gui::menu_overlays`].
use minecraft_terrain::menus::MenuKind;
use minecraft_terrain::pack::PackStack;
use minecraftoss_player::inventory::{Inventory, ItemStack};
use minecraftoss_player::menu::{Menu, SlotDef};

use super::Gui;
use crate::font::rgb;
use crate::render::UiList;

#[path = "screens/anvil.rs"]
mod anvil;
#[path = "screens/brewing_stand.rs"]
mod brewing_stand;
#[path = "screens/crafter.rs"]
mod crafter;
#[path = "screens/enchanting.rs"]
mod enchanting;
#[path = "screens/furnace.rs"]
mod furnace;
#[path = "screens/merchant.rs"]
mod merchant;

pub use anvil::{NameBox, name_to_send as anvil_name_to_send, renames as anvil_renames};
pub use enchanting::{EnchantingBook, book_model, row_at as enchanting_row_at};
pub use merchant::TradeList;

/// The labels' colour (`-12566464`, without a shadow).
const LABEL: u32 = 0x404040;
/// `ChatFormatting.YELLOW`: a dragged slot's count when the slot is full.
const YELLOW: u32 = 0xFFFF55;
/// `extractSlot`'s wash over a slot a drag covers (`-2130706433`).
const DRAGGED: [f32; 4] = [1.0, 1.0, 1.0, 128.0 / 255.0];

/// Where a screen's title goes across.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TitleX {
    At(f32),
    /// `(imageWidth - font.width(title)) / 2` (`DispenserScreen.init`).
    Centred,
    /// Centred on this x: `x - font.width(title) / 2`
    /// (`MerchantScreen.extractLabels`).
    CentredOn(f32),
}

/// A kind's window: `imageWidth` and `imageHeight`, the background's
/// pieces, and its labels' places.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    /// The background sprite (a 256 by 256 texture, or 512 wide).
    background: &'static str,
    /// Its pieces: drawn at (x, y) in the window from its pixels at
    /// (u, v), w by h.
    blits: Vec<[f32; 6]>,
    title_x: TitleX,
    /// `inventoryLabelX` and `inventoryLabelY`.
    inventory_label: (f32, f32),
}

impl Layout {
    /// The whole background at once, the inventory's label at
    /// `imageHeight - 94`, as most screens are.
    fn plain(background: &'static str, height: f32, title_x: TitleX) -> Self {
        Self {
            width: 176.0,
            height,
            background,
            blits: vec![[0.0, 0.0, 0.0, 0.0, 176.0, height]],
            title_x,
            inventory_label: (8.0, height - 94.0),
        }
    }
}

/// The screen of each kind, one arm per kind.
pub fn layout(kind: MenuKind) -> Layout {
    match kind {
        // `ContainerScreen`: 114 + 18 a row, its rows and the inventory
        // cut from the six-row image and joined.
        MenuKind::Generic { rows } => {
            let rows = f32::from(rows.clamp(1, 6));
            let height = 114.0 + rows * 18.0;
            let top = rows * 18.0 + 17.0;
            Layout {
                blits: vec![
                    [0.0, 0.0, 0.0, 0.0, 176.0, top],
                    [0.0, top, 0.0, 126.0, 176.0, 96.0],
                ],
                ..Layout::plain("generic_54", height, TitleX::At(8.0))
            }
        }
        MenuKind::Generic3x3 => Layout::plain("dispenser", 166.0, TitleX::Centred),
        MenuKind::Hopper => Layout::plain("hopper", 133.0, TitleX::At(8.0)),
        MenuKind::ShulkerBox => Layout::plain("shulker_box", 167.0, TitleX::At(8.0)),
        // `AbstractFurnaceScreen.init` centres the title.
        MenuKind::Furnace => Layout::plain("furnace", 166.0, TitleX::Centred),
        MenuKind::BlastFurnace => Layout::plain("blast_furnace", 166.0, TitleX::Centred),
        MenuKind::Smoker => Layout::plain("smoker", 166.0, TitleX::Centred),
        // `MerchantScreen`: 276 wide, from a 512 by 256 image; the title
        // centred over the part right of the offers (`49 + imageWidth / 2`).
        MenuKind::Merchant => Layout {
            width: 276.0,
            blits: vec![[0.0, 0.0, 0.0, 0.0, 276.0, 166.0]],
            inventory_label: (107.0, 72.0),
            ..Layout::plain("villager", 166.0, TitleX::CentredOn(49.0 + 138.0))
        },
        // `CrafterScreen.init` centres the title.
        MenuKind::Crafter => Layout::plain("crafter", 166.0, TitleX::Centred),
        // `BrewingStandScreen.init` centres the title.
        MenuKind::BrewingStand => Layout::plain("brewing_stand", 166.0, TitleX::Centred),
        MenuKind::Enchantment => Layout::plain("enchanting_table", 166.0, TitleX::At(8.0)),
        // `AnvilScreen`: the title at 60.
        MenuKind::Anvil => Layout::plain("anvil", 166.0, TitleX::At(60.0)),
    }
}

/// How a drag shows in a slot it covers.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotDrag {
    /// Not in a drag.
    None,
    /// The only slot of a drag so far: drawn not at all.
    Hidden,
    /// What the drag would leave there, washed white, and the count when
    /// the slot can't take its share (in yellow).
    Preview(Option<i32>),
}

/// A slot as the screen shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuSlotView {
    /// `Slot.x` and `Slot.y`, from the window's corner.
    pub x: i32,
    pub y: i32,
    pub stack: Option<ItemStack>,
    /// The empty slot's icon.
    pub icon: Option<&'static str>,
    pub drag: SlotDrag,
}

/// What a menu's screen shows.
pub struct MenuView<'a> {
    pub kind: MenuKind,
    pub title: String,
    /// Every slot, in menu order.
    pub slots: Vec<MenuSlotView>,
    pub hovered: Option<usize>,
    /// The carried stack as drawn: a drag's leftover, or none when it
    /// leaves nothing.
    pub carried: Option<ItemStack>,
    /// The hovered slot's stack, when nothing is carried.
    pub tooltip: Option<ItemStack>,
    /// For the stacks' wear.
    pub inventory: &'a Inventory,
    /// The menu's data values (progress, costs, a crafter's slot states),
    /// as the screen's inputs leave them, which the storage kinds have none
    /// of: the kinds that draw them read them in `menu_extras`.
    pub data: Vec<i32>,
    /// The menu as the screen shows it, for what a kind keeps beyond its
    /// slots (a merchant's offers).
    pub menu: &'a (dyn Menu + Send),
    /// The trade list's state (the merchant's screen).
    pub trades: &'a TradeList,
    /// `Player.experienceLevel` and `hasInfiniteMaterials`, for what the
    /// player can pay (the enchanting table's rows).
    pub xp_level: i32,
    pub creative: bool,
    /// The anvil's name box: its text, and whether its cursor shows.
    pub name: Option<(&'a str, bool)>,
}

impl Gui {
    /// `leftPos` and `topPos` of a window.
    fn menu_corner(&self, layout: &Layout) -> (f32, f32) {
        (
            ((self.width - layout.width) / 2.0).floor(),
            ((self.height - layout.height) / 2.0).floor(),
        )
    }

    /// The mouse from a kind's window's corner.
    pub fn menu_mouse(&self, kind: MenuKind) -> (f32, f32) {
        let (left, top) = self.menu_corner(&layout(kind));
        (self.mouse.0 - left, self.mouse.1 - top)
    }

    /// The slot under the mouse (`getHoveredSlot`: the first in menu order
    /// within a pixel of its 16 square), and whether the mouse is outside
    /// the window (`hasClickedOutside`).
    pub fn menu_slot_at(&self, kind: MenuKind, slots: &[SlotDef]) -> (Option<usize>, bool) {
        let layout = layout(kind);
        let (left, top) = self.menu_corner(&layout);
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        let hit = slots.iter().position(|slot| {
            let (x, y) = (slot.x as f32, slot.y as f32);
            mx >= x - 1.0 && mx < x + 17.0 && my >= y - 1.0 && my < y + 17.0
        });
        let outside = mx < 0.0 || my < 0.0 || mx >= layout.width || my >= layout.height;
        (hit, outside)
    }

    /// A menu's screen.
    pub fn menu_screen(&mut self, ui: &mut UiList, packs: &PackStack, view: &MenuView<'_>) {
        let layout = layout(view.kind);
        let (left, top) = self.menu_corner(&layout);
        // `extractBackground`.
        self.dim(ui);
        for &[x, y, u, v, w, h] in &layout.blits {
            self.sprite_part(ui, layout.background, left + x, top + y, u, v, w, h);
        }
        self.menu_extras(ui, packs, view, left, top);
        // `extractLabels`.
        let label = rgb(LABEL);
        let title_x = match layout.title_x {
            TitleX::At(x) => x,
            TitleX::Centred => ((layout.width - self.font.width(&view.title)) / 2.0).trunc(),
            TitleX::CentredOn(x) => x - (self.font.width(&view.title) / 2.0).floor(),
        };
        self.text(ui, &view.title, left + title_x, top + 6.0, label, false);
        let inventory = crate::creative::translate(&self.language, "container.inventory", &[]);
        let (x, y) = layout.inventory_label;
        self.text(ui, &inventory, left + x, top + y, label, false);
        // The hovered slot's highlight behind the items, and in front, when
        // it has one (`isHighlightable`).
        let hovered = view
            .hovered
            .filter(|&i| view.menu.is_highlightable(i))
            .and_then(|i| view.slots.get(i));
        if let Some(slot) = hovered {
            let (x, y) = (left + slot.x as f32, top + slot.y as f32);
            self.sprite(ui, "slot_highlight_back", x - 4.0, y - 4.0, 24.0, 24.0);
        }
        for (index, slot) in view.slots.iter().enumerate() {
            if !self.menu_slot_extra(ui, view, index, left, top) {
                self.menu_slot(ui, packs, view.inventory, slot, left, top);
            }
        }
        if let Some(slot) = hovered {
            let (x, y) = (left + slot.x as f32, top + slot.y as f32);
            self.sprite(ui, "slot_highlight_front", x - 4.0, y - 4.0, 24.0, 24.0);
        }
        // `extractCarriedItem`, at the mouse's whole pixel.
        if let Some(stack) = view.carried.as_ref() {
            let (mx, my) = (self.mouse.0.floor(), self.mouse.1.floor());
            self.item(ui, packs, view.inventory, stack, mx - 8.0, my - 8.0);
        }
        if let Some(stack) = view.tooltip.as_ref() {
            self.stack_tooltip(ui, stack, false);
        }
        self.menu_overlays(ui, view, left, top);
    }

    /// `extractSlot`: the empty slot's icon, or its stack (a drag's
    /// preview over a white wash).
    fn menu_slot(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        inventory: &Inventory,
        slot: &MenuSlotView,
        left: f32,
        top: f32,
    ) {
        let (x, y) = (left + slot.x as f32, top + slot.y as f32);
        match (&slot.drag, slot.stack.as_ref()) {
            (SlotDrag::Hidden, _) => {}
            (_, None) => {
                if let Some(icon) = slot.icon {
                    self.sprite(ui, icon, x, y, 16.0, 16.0);
                }
            }
            (SlotDrag::None, Some(stack)) => self.item(ui, packs, inventory, stack, x, y),
            (SlotDrag::Preview(full), Some(stack)) => {
                self.fill(ui, x, y, 16.0, 16.0, DRAGGED);
                let text = full.map(|count| count.to_string());
                let count = text.as_deref().map(|text| (text, rgb(YELLOW)));
                self.item_counted(ui, packs, inventory, stack, x, y, count);
            }
        }
    }

    /// What a kind draws over its background: progress, buttons. One arm
    /// per kind that has any.
    fn menu_extras(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        match view.kind {
            MenuKind::Generic { .. }
            | MenuKind::Generic3x3
            | MenuKind::Hopper
            | MenuKind::ShulkerBox => {}
            MenuKind::Furnace | MenuKind::BlastFurnace | MenuKind::Smoker => {
                self.furnace_extras(ui, view, left, top)
            }
            MenuKind::Merchant => self.merchant_extras(ui, packs, view, left, top),
            MenuKind::Crafter => self.crafter_extras(ui, view, left, top),
            MenuKind::BrewingStand => self.brewing_stand_extras(ui, view, left, top),
            MenuKind::Enchantment => self.enchanting_extras(ui, view, left, top),
            MenuKind::Anvil => self.anvil_extras(ui, view, left, top),
        }
    }

    /// A slot a kind draws in its own way (`extractSlot` overridden): true
    /// when it drew it. One arm per kind that has any.
    fn menu_slot_extra(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        index: usize,
        left: f32,
        top: f32,
    ) -> bool {
        match view.kind {
            MenuKind::Crafter => self.crafter_slot(ui, view, index, left, top),
            _ => false,
        }
    }

    /// What a kind draws over everything: its buttons' tooltips. One arm
    /// per kind that has any.
    fn menu_overlays(&mut self, ui: &mut UiList, view: &MenuView<'_>, left: f32, top: f32) {
        match view.kind {
            MenuKind::Merchant => self.merchant_tooltips(ui, view, left, top),
            MenuKind::Crafter => self.crafter_tooltip(ui, view),
            MenuKind::Enchantment => self.enchanting_tooltip(ui, view, left, top),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chest_screen_is_cut_to_its_rows() {
        // `ContainerScreen`: 176 by 114 + 18 a row, the inventory's label at
        // imageHeight - 94, the rows' part and the inventory's part.
        for (rows, height, label) in [(1u8, 132.0, 38.0), (3, 168.0, 74.0), (6, 222.0, 128.0)] {
            let layout = layout(MenuKind::Generic { rows });
            assert_eq!((layout.width, layout.height), (176.0, height));
            assert_eq!(layout.inventory_label, (8.0, label));
            let top = f32::from(rows) * 18.0 + 17.0;
            assert_eq!(
                layout.blits,
                [
                    [0.0, 0.0, 0.0, 0.0, 176.0, top],
                    [0.0, top, 0.0, 126.0, 176.0, 96.0]
                ]
            );
            // The two pieces end a pixel short of the window, as vanilla's.
            assert_eq!(top + 96.0, height - 1.0);
        }
    }

    #[test]
    fn the_other_storage_screens_have_their_own_sizes() {
        let sizes = [
            (
                MenuKind::ShulkerBox,
                "shulker_box",
                167.0,
                73.0,
                TitleX::At(8.0),
            ),
            (MenuKind::Hopper, "hopper", 133.0, 39.0, TitleX::At(8.0)),
            (
                MenuKind::Generic3x3,
                "dispenser",
                166.0,
                72.0,
                TitleX::Centred,
            ),
            (MenuKind::Furnace, "furnace", 166.0, 72.0, TitleX::Centred),
            (MenuKind::Smoker, "smoker", 166.0, 72.0, TitleX::Centred),
            (MenuKind::Crafter, "crafter", 166.0, 72.0, TitleX::Centred),
            (
                MenuKind::BrewingStand,
                "brewing_stand",
                166.0,
                72.0,
                TitleX::Centred,
            ),
        ];
        for (kind, background, height, label, title_x) in sizes {
            let layout = layout(kind);
            assert_eq!(layout.background, background);
            assert_eq!((layout.width, layout.height), (176.0, height));
            assert_eq!(layout.inventory_label, (8.0, label));
            assert_eq!(layout.title_x, title_x);
            assert_eq!(layout.blits, [[0.0, 0.0, 0.0, 0.0, 176.0, height]]);
        }
    }

    #[test]
    fn every_storage_menu_fits_its_window() {
        // The engine's slots, which the screen hit-tests and draws, all lie
        // inside the window the screen draws for them.
        for kind in [
            MenuKind::Generic { rows: 1 },
            MenuKind::Generic { rows: 3 },
            MenuKind::Generic { rows: 6 },
            MenuKind::ShulkerBox,
            MenuKind::Hopper,
            MenuKind::Generic3x3,
            MenuKind::Furnace,
            MenuKind::BlastFurnace,
            MenuKind::Smoker,
            MenuKind::Merchant,
            MenuKind::Crafter,
            MenuKind::BrewingStand,
            MenuKind::Enchantment,
            MenuKind::Anvil,
        ] {
            let layout = layout(kind);
            let menu = kind.menu(Vec::new());
            for slot in menu.slots() {
                assert!(
                    slot.x >= 7 && slot.x as f32 + 17.0 <= layout.width,
                    "{kind:?}"
                );
                assert!(
                    slot.y >= 16 && slot.y as f32 + 17.0 <= layout.height,
                    "{kind:?}"
                );
            }
        }
    }
}
