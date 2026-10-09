//! The 2D layer, drawn as vanilla draws it from the pack's own sprites: the
//! hotbar, hearts, hunger, armor, air and experience; the inventory and
//! crafting table screens, and the server menus' screens (`menu_gui.rs`);
//! the pause, death and loading screens. Layout is in GUI pixels at
//! vanilla's automatic GUI scale.
use std::collections::HashMap;

use image::RgbaImage;
use minecraft_terrain::pack::{PackStack, ResourceId};
use minecraftoss_player::inventory::{Inventory, ItemStack};
use minecraftoss_player::survival::SurvivalStatus;

use crate::font::{Font, LINE, rgb};
use crate::render::{Renderer, TextureId, UiList};

#[path = "menu_gui.rs"]
mod menu_gui;
pub use menu_gui::{MenuSlotView, MenuView, SlotDrag, TradeList};

/// Icons in a row of the icon atlas, and rows: room for every creative
/// item and its variants.
const ICON_COLUMNS: u32 = 64;
const ICON_ROWS: u32 = 32;
/// The largest icon, so the atlas stays within a texture's size limit.
const ICON_MAX: u32 = 128;
/// Time a frame may spend making new item icons, so a screen full of new
/// items fills in over a few frames instead of stalling one.
const ICON_BUDGET: std::time::Duration = std::time::Duration::from_millis(8);
const WHITE: [f32; 4] = [1.0; 4];

#[derive(Clone, Copy)]
struct Sprite {
    texture: TextureId,
    size: (f32, f32),
    /// Nine-slice border in sprite pixels; 0 stretches the whole sprite.
    border: f32,
}

/// A slot on an open screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Inventory(usize),
    Crafting(usize),
    CraftingResult,
    Workbench(usize),
    WorkbenchResult,
    /// A cell of the creative item list, counted from the first shown.
    Creative(usize),
    /// The creative inventory tab's bin (`destroyItemSlot`).
    Destroy,
}

/// What the creative screen shows.
pub struct CreativeView<'a> {
    /// The selected tab, and those shown.
    pub tab: usize,
    pub tabs: &'a [usize],
    pub inventory: &'a Inventory,
    /// The tab's items (`ItemPickerMenu.items`), from row `row` on.
    pub items: &'a [Option<ItemStack>],
    pub row: usize,
    /// `scrollOffs`, and whether there is more than a screen to scroll.
    pub scroll: f32,
    pub can_scroll: bool,
    /// The search box's text, and whether its cursor shows.
    pub search: Option<(&'a str, bool)>,
}

/// What is on screen over the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Playing,
    Inventory,
    Crafting,
    Creative,
    /// A menu the server runs (a chest, a hopper...).
    Menu,
    Paused,
    Dead,
}

/// A button the player clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Resume,
    SaveAndQuit,
    Respawn,
}

/// What the HUD shows of the player.
pub struct Hud<'a> {
    pub inventory: &'a Inventory,
    pub selected: usize,
    /// Survival's bars; none in creative.
    pub survival: Option<&'a SurvivalStatus>,
    pub armor: u8,
    pub eyes_in_water: bool,
    /// The selected item's name and how opaque it still is.
    pub selected_name: Option<(String, f32)>,
    /// Hearts shake when health is low.
    pub shake: u64,
    pub debug: Option<Vec<String>>,
    /// The action bar's message (`setOverlayMessage`) and how opaque it
    /// still is.
    pub overlay: Option<(String, f32)>,
}

/// Item icons drawn at the GUI's pixel size, 16 pixels to a GUI scale
/// step (`GuiItemAtlas`), so blocks and items are as sharp as the screen.
struct Icons {
    /// An icon's side in pixels.
    size: u32,
    /// The image changed size: its texture is made anew.
    resized: bool,
    image: RgbaImage,
    texture: Option<TextureId>,
    cells: HashMap<String, Option<[f32; 4]>>,
    next: u32,
    /// Cells made since the last upload.
    fresh: Vec<(u32, u32)>,
    /// When this frame's first new icon was started.
    started: Option<std::time::Instant>,
}

pub struct Gui {
    pub font: Font,
    /// The texture the inventory's player is drawn into.
    model_texture: TextureId,
    /// When the GUI was made, for the glint's scroll.
    started: std::time::Instant,
    sprites: HashMap<String, Sprite>,
    icons: Icons,
    language: HashMap<String, String>,
    /// Window pixels per GUI pixel.
    pub scale: f32,
    pub width: f32,
    pub height: f32,
    /// The mouse in GUI pixels.
    pub mouse: (f32, f32),
}

/// The sprites, by name: their pack path under `textures/` and nine-slice
/// border. New ones are appended.
const SPRITES: &[(&str, &str, f32)] = &[
    (
        "creative_scroller",
        "gui/sprites/container/creative_inventory/scroller",
        0.0,
    ),
    (
        "creative_scroller_disabled",
        "gui/sprites/container/creative_inventory/scroller_disabled",
        0.0,
    ),
    ("hotbar", "gui/sprites/hud/hotbar", 0.0),
    ("hotbar_selection", "gui/sprites/hud/hotbar_selection", 0.0),
    ("hotbar_offhand_left", "gui/sprites/hud/hotbar_offhand_left", 0.0),
    ("crosshair", "gui/sprites/hud/crosshair", 0.0),
    ("heart_container", "gui/sprites/hud/heart/container", 0.0),
    ("heart_full", "gui/sprites/hud/heart/full", 0.0),
    ("heart_half", "gui/sprites/hud/heart/half", 0.0),
    ("food_empty", "gui/sprites/hud/food_empty", 0.0),
    ("food_half", "gui/sprites/hud/food_half", 0.0),
    ("food_full", "gui/sprites/hud/food_full", 0.0),
    ("armor_empty", "gui/sprites/hud/armor_empty", 0.0),
    ("armor_half", "gui/sprites/hud/armor_half", 0.0),
    ("armor_full", "gui/sprites/hud/armor_full", 0.0),
    ("air", "gui/sprites/hud/air", 0.0),
    ("air_empty", "gui/sprites/hud/air_empty", 0.0),
    (
        "xp_background",
        "gui/sprites/hud/experience_bar_background",
        0.0,
    ),
    (
        "xp_progress",
        "gui/sprites/hud/experience_bar_progress",
        0.0,
    ),
    ("button", "gui/sprites/widget/button", 3.0),
    (
        "button_highlighted",
        "gui/sprites/widget/button_highlighted",
        3.0,
    ),
    ("button_disabled", "gui/sprites/widget/button_disabled", 3.0),
    ("tooltip_background", "gui/sprites/tooltip/background", 9.0),
    ("tooltip_frame", "gui/sprites/tooltip/frame", 9.0),
    (
        "slot_highlight_back",
        "gui/sprites/container/slot_highlight_back",
        0.0,
    ),
    (
        "slot_highlight_front",
        "gui/sprites/container/slot_highlight_front",
        0.0,
    ),
    ("inventory", "gui/container/inventory", 0.0),
    ("crafting_table", "gui/container/crafting_table", 0.0),
    ("title", "gui/title/minecraft", 0.0),
    ("edition", "gui/title/edition", 0.0),
    ("menu_background", "gui/menu_background", 0.0),
    ("underwater", "misc/underwater", 0.0),
    ("glint", "misc/enchanted_glint_item", 0.0),
    ("vignette", "misc/vignette", 0.0),
    ("helmet_slot", "gui/sprites/container/slot/helmet", 0.0),
    (
        "chestplate_slot",
        "gui/sprites/container/slot/chestplate",
        0.0,
    ),
    ("shield_slot", "gui/sprites/container/slot/shield", 0.0),
    // The server menus' backgrounds (`menu_gui::layout`).
    ("generic_54", "gui/container/generic_54", 0.0),
    ("shulker_box", "gui/container/shulker_box", 0.0),
    ("hopper", "gui/container/hopper", 0.0),
    ("dispenser", "gui/container/dispenser", 0.0),
    ("furnace", "gui/container/furnace", 0.0),
    ("blast_furnace", "gui/container/blast_furnace", 0.0),
    ("smoker", "gui/container/smoker", 0.0),
    // The furnaces' progress (`AbstractFurnaceScreen`).
    (
        "furnace_lit_progress",
        "gui/sprites/container/furnace/lit_progress",
        0.0,
    ),
    (
        "furnace_burn_progress",
        "gui/sprites/container/furnace/burn_progress",
        0.0,
    ),
    (
        "blast_furnace_lit_progress",
        "gui/sprites/container/blast_furnace/lit_progress",
        0.0,
    ),
    (
        "blast_furnace_burn_progress",
        "gui/sprites/container/blast_furnace/burn_progress",
        0.0,
    ),
    (
        "smoker_lit_progress",
        "gui/sprites/container/smoker/lit_progress",
        0.0,
    ),
    (
        "smoker_burn_progress",
        "gui/sprites/container/smoker/burn_progress",
        0.0,
    ),
    // The trading screen (`MerchantScreen`).
    ("villager", "gui/container/villager", 0.0),
    (
        "villager_out_of_stock",
        "gui/sprites/container/villager/out_of_stock",
        0.0,
    ),
    (
        "villager_experience_bar_background",
        "gui/sprites/container/villager/experience_bar_background",
        0.0,
    ),
    (
        "villager_experience_bar_current",
        "gui/sprites/container/villager/experience_bar_current",
        0.0,
    ),
    (
        "villager_experience_bar_result",
        "gui/sprites/container/villager/experience_bar_result",
        0.0,
    ),
    (
        "villager_scroller",
        "gui/sprites/container/villager/scroller",
        0.0,
    ),
    (
        "villager_scroller_disabled",
        "gui/sprites/container/villager/scroller_disabled",
        0.0,
    ),
    (
        "villager_trade_arrow",
        "gui/sprites/container/villager/trade_arrow",
        0.0,
    ),
    (
        "villager_trade_arrow_out_of_stock",
        "gui/sprites/container/villager/trade_arrow_out_of_stock",
        0.0,
    ),
    (
        "villager_discount_strikethrough",
        "gui/sprites/container/villager/discount_strikethrough",
        0.0,
    ),
];

/// Empty slots' icons that menus name (`Slot.getNoItemIcon`, as
/// `SlotDef.icon`), loaded under their own ids from `gui/sprites/`. New
/// ones are appended.
const SLOT_ICONS: &[&str] = &[];

impl Gui {
    pub fn load(packs: &PackStack, renderer: &mut Renderer) -> anyhow::Result<Self> {
        let font = Font::load(packs, renderer)?;
        let mut sprites = HashMap::new();
        // The creative screen's backgrounds and tabs.
        let mut creative = Vec::new();
        for background in ["items", "item_search", "inventory"] {
            creative.push((
                format!("creative_{background}"),
                format!("gui/container/creative_inventory/tab_{background}"),
            ));
        }
        for row in ["top", "bottom"] {
            for state in ["selected", "unselected"] {
                for column in 1..=7 {
                    creative.push((
                        format!("tab_{row}_{state}_{column}"),
                        format!(
                            "gui/sprites/container/creative_inventory/tab_{row}_{state}_{column}"
                        ),
                    ));
                }
            }
        }
        let named = SPRITES
            .iter()
            .map(|&(name, path, border)| (name.to_owned(), path.to_owned(), border))
            .chain(creative.into_iter().map(|(name, path)| (name, path, 0.0)))
            .chain(
                [
                    ("leggings_slot", "gui/sprites/container/slot/leggings"),
                    ("boots_slot", "gui/sprites/container/slot/boots"),
                ]
                .map(|(name, path)| (name.to_owned(), path.to_owned(), 0.0)),
            )
            .chain(
                SLOT_ICONS
                    .iter()
                    .map(|id| (id.to_string(), format!("gui/sprites/{id}"), 0.0)),
            );
        for (name, path, border) in named {
            let Ok(id) = ResourceId::parse(&format!("minecraft:{path}")) else {
                continue;
            };
            let Some(bytes) = packs.texture(&id)? else {
                log!("The pack has no {path}.png");
                continue;
            };
            let image = image::load_from_memory(&bytes)?.to_rgba8();
            let size = (image.width() as f32, image.height() as f32);
            sprites.insert(
                name,
                Sprite {
                    texture: renderer.add_texture(&image),
                    size,
                    border,
                },
            );
        }
        Ok(Self {
            font,
            model_texture: renderer.model_texture(),
            started: std::time::Instant::now(),
            sprites,
            icons: Icons {
                size: 32,
                resized: false,
                image: RgbaImage::new(32 * ICON_COLUMNS, 32 * ICON_ROWS),
                texture: None,
                cells: HashMap::new(),
                next: 0,
                fresh: Vec::new(),
                started: None,
            },
            language: minecraft_terrain::item_icons::language(packs)
                .unwrap_or_else(|_| HashMap::new()),
            scale: 2.0,
            width: 320.0,
            height: 240.0,
            mouse: (0.0, 0.0),
        })
    }

    /// Vanilla's automatic GUI scale for a window, and the mouse.
    pub fn layout(&mut self, window: (u32, u32), mouse: (f32, f32)) {
        let (w, h) = (window.0.max(1) as f32, window.1.max(1) as f32);
        let mut scale = 1.0;
        while w / (scale + 1.0) >= 320.0 && h / (scale + 1.0) >= 240.0 {
            scale += 1.0;
        }
        self.scale = scale;
        // Icons follow the scale: a new one starts the icons over.
        let size = (16 * scale as u32).min(ICON_MAX);
        if size != self.icons.size {
            self.icons.size = size;
            self.icons.resized = true;
            self.icons.image = RgbaImage::new(size * ICON_COLUMNS, size * ICON_ROWS);
            self.icons.cells.clear();
            self.icons.next = 0;
            self.icons.fresh.clear();
        }
        self.width = w / scale;
        self.height = h / scale;
        self.mouse = (mouse.0 / scale, mouse.1 / scale);
    }

    /// A stack's display name.
    pub fn stack_name(&self, stack: &ItemStack) -> String {
        crate::creative::name(&self.language, stack)
    }

    /// The pack's English text.
    pub fn language(&self) -> &HashMap<String, String> {
        &self.language
    }

    /// Uploads item icons made this frame.
    pub fn flush(&mut self, renderer: &mut Renderer) {
        self.icons.started = None;
        let Some(texture) = self.icons.texture else {
            self.icons.texture = Some(renderer.add_texture(&self.icons.image));
            self.icons.fresh.clear();
            self.icons.resized = false;
            return;
        };
        if std::mem::take(&mut self.icons.resized) {
            renderer.replace_texture(texture, &self.icons.image);
            self.icons.fresh.clear();
            return;
        }
        let size = self.icons.size;
        for (x, y) in std::mem::take(&mut self.icons.fresh) {
            let cell = image::imageops::crop_imm(&self.icons.image, x, y, size, size).to_image();
            renderer.update_region(texture, x, y, &cell);
        }
    }

    /// A stack's icon in the atlas, made on first use; a potion's colour
    /// makes an icon of its own.
    fn icon(&mut self, packs: &PackStack, stack: &ItemStack) -> Option<[f32; 4]> {
        let tint =
            crate::creative::potion_color(stack).or_else(|| crate::creative::dyed_color(stack));
        // A banner's patterns draw it.
        let patterns = stack
            .components
            .as_ref()
            .and_then(|parts| parts.get("minecraft:banner_patterns"));
        let mut key = match tint {
            Some(color) => format!("{}#{color:06x}", stack.id),
            None => stack.id.clone(),
        };
        if let Some(patterns) = patterns {
            key.push_str(&patterns.to_string());
        }
        if let Some(rect) = self.icons.cells.get(&key) {
            return *rect;
        }
        if self
            .icons
            .started
            .get_or_insert_with(std::time::Instant::now)
            .elapsed()
            > ICON_BUDGET
        {
            return None;
        }
        let size = self.icons.size;
        let rect = if self.icons.next >= ICON_COLUMNS * ICON_ROWS {
            None
        } else {
            match minecraft_terrain::item_icons::item_icon_tinted(
                packs,
                &stack.id,
                size as usize,
                tint,
                stack.components.as_ref(),
            ) {
                Ok(Some(icon)) => {
                    let cell = self.icons.next;
                    self.icons.next += 1;
                    let (x, y) = ((cell % ICON_COLUMNS) * size, (cell / ICON_COLUMNS) * size);
                    let icon = image::imageops::resize(
                        &icon,
                        size,
                        size,
                        image::imageops::FilterType::Nearest,
                    );
                    image::imageops::replace(
                        &mut self.icons.image,
                        &icon,
                        i64::from(x),
                        i64::from(y),
                    );
                    self.icons.fresh.push((x, y));
                    let (width, height) = self.icons.image.dimensions();
                    Some([
                        x as f32 / width as f32,
                        y as f32 / height as f32,
                        (x + size) as f32 / width as f32,
                        (y + size) as f32 / height as f32,
                    ])
                }
                _ => None,
            }
        };
        self.icons.cells.insert(key, rect);
        rect
    }

    /// `TextureTransform.setupGlintTexturing(8)` at the default glint
    /// speed of 0.5, over an item whose sprite spans a sixty-fourth of its
    /// sheet, as vanilla's items do.
    fn glint_uv(&self, [u0, v0, ..]: [f32; 4]) -> [[f32; 2]; 4] {
        let millis = (self.started.elapsed().as_millis() as f64 * 0.5 * 8.0) as u64;
        let offset0 = (millis % 110_000) as f32 / 110_000.0;
        let offset1 = (millis % 30_000) as f32 / 30_000.0;
        let (sin, cos) = (std::f32::consts::PI / 18.0).sin_cos();
        let span = 1.0 / 64.0;
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]].map(|[s, t]| {
            let (u, v) = ((u0 + s * span) * 8.0, (v0 + t * span) * 8.0);
            [u * cos - v * sin - offset0, u * sin + v * cos + offset1]
        })
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> [f32; 4] {
        [
            x * self.scale,
            y * self.scale,
            w * self.scale,
            h * self.scale,
        ]
    }

    fn fill(&self, ui: &mut UiList, x: f32, y: f32, w: f32, h: f32, colour: [f32; 4]) {
        ui.fill(self.rect(x, y, w, h), colour);
    }

    /// `Screen.extractTransparentBackground`: an in-game screen darkens the
    /// world, from `0xC0101010` at the top to `0xD0101010` at the bottom.
    fn dim(&self, ui: &mut UiList) {
        let grey = 16.0 / 255.0;
        ui.gradient(
            self.rect(0.0, 0.0, self.width, self.height),
            [grey, grey, grey, 192.0 / 255.0],
            [grey, grey, grey, 208.0 / 255.0],
        );
    }

    /// A whole sprite, stretched or nine-sliced to the rectangle.
    fn sprite(&self, ui: &mut UiList, name: &str, x: f32, y: f32, w: f32, h: f32) {
        self.sprite_tinted(ui, name, x, y, w, h, WHITE);
    }

    #[allow(clippy::too_many_arguments)]
    fn sprite_tinted(
        &self,
        ui: &mut UiList,
        name: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        colour: [f32; 4],
    ) {
        let Some(sprite) = self.sprites.get(name) else {
            return;
        };
        let b = sprite.border;
        if b <= 0.0 {
            ui.quad(
                sprite.texture,
                self.rect(x, y, w, h),
                [0.0, 0.0, 1.0, 1.0],
                colour,
            );
            return;
        }
        let (sw, sh) = sprite.size;
        let xs = [
            (x, b, 0.0, b),
            (x + b, w - 2.0 * b, b, sw - 2.0 * b),
            (x + w - b, b, sw - b, b),
        ];
        let ys = [
            (y, b, 0.0, b),
            (y + b, h - 2.0 * b, b, sh - 2.0 * b),
            (y + h - b, b, sh - b, b),
        ];
        for (px, pw, u, uw) in xs {
            for (py, ph, v, vh) in ys {
                if pw <= 0.0 || ph <= 0.0 {
                    continue;
                }
                ui.quad(
                    sprite.texture,
                    self.rect(px, py, pw, ph),
                    [u / sw, v / sh, (u + uw) / sw, (v + vh) / sh],
                    colour,
                );
            }
        }
    }

    /// Part of a sprite: its pixels from (u, v), `w` by `h`.
    #[allow(clippy::too_many_arguments)]
    fn sprite_part(
        &self,
        ui: &mut UiList,
        name: &str,
        x: f32,
        y: f32,
        u: f32,
        v: f32,
        w: f32,
        h: f32,
    ) {
        let Some(sprite) = self.sprites.get(name) else {
            return;
        };
        let (sw, sh) = sprite.size;
        ui.quad(
            sprite.texture,
            self.rect(x, y, w, h),
            [u / sw, v / sh, (u + w) / sw, (v + h) / sh],
            WHITE,
        );
    }

    pub fn text(
        &self,
        ui: &mut UiList,
        text: &str,
        x: f32,
        y: f32,
        colour: [f32; 4],
        shadow: bool,
    ) {
        self.font.draw(
            ui,
            text,
            x * self.scale,
            y * self.scale,
            self.scale,
            colour,
            shadow,
        );
    }

    pub fn centered(&self, ui: &mut UiList, text: &str, y: f32, colour: [f32; 4]) {
        let x = (self.width - self.font.width(text)) / 2.0;
        self.text(ui, text, x, y, colour, true);
    }

    /// A 16-pixel item at (x, y) with its count and wear.
    fn item(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        inventory: &Inventory,
        stack: &ItemStack,
        x: f32,
        y: f32,
    ) {
        self.item_counted(ui, packs, inventory, stack, x, y, None);
    }

    /// An item with its wear, and its count or `count`'s text in its
    /// colour instead (`itemDecorations` with a count text: a drag's
    /// capped stacks).
    #[allow(clippy::too_many_arguments)]
    fn item_counted(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        inventory: &Inventory,
        stack: &ItemStack,
        x: f32,
        y: f32,
        count: Option<(&str, [f32; 4])>,
    ) {
        if let Some(rect) = self.icon(packs, stack)
            && let Some(texture) = self.icons.texture
        {
            ui.quad(texture, self.rect(x, y, 16.0, 16.0), rect, WHITE);
            if crate::creative::foil(stack)
                && let Some(glint) = self.sprites.get("glint")
            {
                ui.glint(
                    texture,
                    glint.texture,
                    self.rect(x, y, 16.0, 16.0),
                    rect,
                    self.glint_uv(rect),
                );
            }
        }
        if let Some((damage, max)) = inventory
            .recipes
            .durability(stack)
            .filter(|&(damage, max)| damage > 0 && max > 0)
        {
            let left = (1.0 - damage as f32 / max as f32).clamp(0.0, 1.0);
            let width = (13.0 * left).round();
            let hue = left / 3.0;
            let colour = hsv(hue, 1.0, 1.0);
            self.fill(ui, x + 2.0, y + 13.0, 13.0, 2.0, [0.0, 0.0, 0.0, 1.0]);
            self.fill(ui, x + 2.0, y + 13.0, width, 1.0, colour);
        }
        let own = stack.count.to_string();
        let count = count.or_else(|| (stack.count != 1).then_some((own.as_str(), WHITE)));
        if let Some((count, colour)) = count {
            self.item_count(ui, count, x, y, colour);
        }
    }

    /// `itemDecorations`' count for an item at (x, y), right-aligned on its
    /// corner.
    fn item_count(&self, ui: &mut UiList, count: &str, x: f32, y: f32, colour: [f32; 4]) {
        let width = self.font.width(count);
        self.text(
            ui,
            count,
            x + 19.0 - 2.0 - width,
            y + 6.0 + 3.0,
            colour,
            true,
        );
    }

    /// The in-game HUD.
    pub fn hud(&mut self, ui: &mut UiList, packs: &PackStack, hud: &Hud<'_>) {
        let (w, h) = (self.width, self.height);
        let center = (w / 2.0).floor();
        if hud.eyes_in_water {
            self.sprite_tinted(ui, "underwater", 0.0, 0.0, w, h, [1.0, 1.0, 1.0, 0.1]);
        }
        if let Some(sprite) = self.sprites.get("crosshair") {
            let (cw, ch) = sprite.size;
            ui.inverted(
                sprite.texture,
                self.rect(((w - cw) / 2.0).floor(), ((h - ch) / 2.0).floor(), cw, ch),
                [0.0, 0.0, 1.0, 1.0],
            );
        }
        self.sprite(ui, "hotbar", center - 91.0, h - 22.0, 182.0, 22.0);
        self.sprite(
            ui,
            "hotbar_selection",
            center - 91.0 - 1.0 + hud.selected as f32 * 20.0,
            h - 22.0 - 1.0,
            24.0,
            23.0,
        );
        // The offhand's slot left of the hotbar (`Hud`'s
        // `HOTBAR_OFFHAND_LEFT_SPRITE`; the main arm is the right).
        let offhand = hud.inventory.slots[40].as_ref();
        if offhand.is_some() {
            self.sprite(ui, "hotbar_offhand_left", center - 91.0 - 29.0, h - 23.0, 29.0, 24.0);
        }
        for slot in 0..9 {
            if let Some(stack) = hud.inventory.slots[slot].as_ref() {
                self.item(
                    ui,
                    packs,
                    hud.inventory,
                    stack,
                    center - 90.0 + slot as f32 * 20.0 + 2.0,
                    h - 16.0 - 3.0,
                );
            }
        }
        if let Some(stack) = offhand {
            self.item(ui, packs, hud.inventory, stack, center - 91.0 - 26.0, h - 16.0 - 3.0);
        }
        if let Some(status) = hud.survival {
            // Experience.
            self.sprite(
                ui,
                "xp_background",
                center - 91.0,
                h - 32.0 + 3.0,
                182.0,
                5.0,
            );
            let progress = (status.experience_progress * 183.0) as i32;
            if progress > 0 {
                self.sprite_part(
                    ui,
                    "xp_progress",
                    center - 91.0,
                    h - 32.0 + 3.0,
                    0.0,
                    0.0,
                    progress.min(182) as f32,
                    5.0,
                );
            }
            if status.experience_level > 0 {
                let text = status.experience_level.to_string();
                let x = center - self.font.width(&text) / 2.0;
                let y = h - 31.0 - 4.0;
                for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    self.text(ui, &text, x + dx, y + dy, [0.0, 0.0, 0.0, 1.0], false);
                }
                self.text(ui, &text, x, y, rgb(0x80FF20), false);
            }
            // Health, with a shake when low.
            let left = center - 91.0;
            let top = h - 39.0;
            let hearts = (status.max_health / 2.0).ceil() as i32;
            let health = status.health.ceil() as i32;
            for i in (0..hearts).rev() {
                let x = left + (i % 10) as f32 * 8.0;
                let mut y = top - (i / 10) as f32 * 10.0;
                if health <= 4 {
                    y +=
                        ((hud.shake.wrapping_mul(31).wrapping_add(i as u64 * 17)) % 3) as f32 - 1.0;
                }
                self.sprite(ui, "heart_container", x, y, 9.0, 9.0);
                if i * 2 + 1 < health {
                    self.sprite(ui, "heart_full", x, y, 9.0, 9.0);
                } else if i * 2 + 1 == health {
                    self.sprite(ui, "heart_half", x, y, 9.0, 9.0);
                }
            }
            let rows = ((hearts - 1) / 10 + 1) as f32;
            let above = top - (rows - 1.0) * 10.0 - 10.0;
            if hud.armor > 0 {
                let armor = i32::from(hud.armor);
                for i in 0..10 {
                    let x = left + i as f32 * 8.0;
                    let name = if i * 2 + 1 < armor {
                        "armor_full"
                    } else if i * 2 + 1 == armor {
                        "armor_half"
                    } else {
                        "armor_empty"
                    };
                    self.sprite(ui, name, x, above, 9.0, 9.0);
                }
            }
            // Hunger.
            let right = center + 91.0;
            let food = i32::from(status.food.level);
            for i in 0..10 {
                let x = right - i as f32 * 8.0 - 9.0;
                self.sprite(ui, "food_empty", x, top, 9.0, 9.0);
                if i * 2 + 1 < food {
                    self.sprite(ui, "food_full", x, top, 9.0, 9.0);
                } else if i * 2 + 1 == food {
                    self.sprite(ui, "food_half", x, top, 9.0, 9.0);
                }
            }
            // Air, while under water or getting it back.
            if hud.eyes_in_water || status.air < status.max_air {
                let air = status.air.clamp(0, status.max_air);
                let full = ((air - 2) as f32 * 10.0 / status.max_air as f32).ceil() as i32;
                let partial = (air as f32 * 10.0 / status.max_air as f32).ceil() as i32 - full;
                for i in 0..(full + partial) {
                    let x = right - i as f32 * 8.0 - 9.0;
                    self.sprite(
                        ui,
                        if i < full { "air" } else { "air_empty" },
                        x,
                        top - 10.0,
                        9.0,
                        9.0,
                    );
                }
            }
        }
        if let Some((name, alpha)) = hud.selected_name.as_ref()
            && *alpha > 0.0
        {
            let y = h - if hud.survival.is_some() { 59.0 } else { 45.0 };
            let x = (w - self.font.width(name)) / 2.0;
            self.text(ui, name, x, y, [1.0, 1.0, 1.0, *alpha], true);
        }
        // `extractOverlayMessage`: centred 68 pixels up, without a backdrop
        // (the text background is for chat only by default).
        if let Some((text, alpha)) = hud.overlay.as_ref()
            && *alpha > 0.0
        {
            let x = center - (self.font.width(text) / 2.0).trunc();
            self.text(ui, text, x, h - 68.0 - 4.0, [1.0, 1.0, 1.0, *alpha], true);
        }
        if let Some(lines) = hud.debug.as_ref() {
            for (i, line) in lines.iter().enumerate() {
                let y = 2.0 + i as f32 * LINE;
                self.fill(
                    ui,
                    1.0,
                    y - 1.0,
                    self.font.width(line) + 2.0,
                    LINE,
                    [0.31, 0.31, 0.31, 0.56],
                );
                self.text(ui, line, 2.0, y, rgb(0xE0E0E0), false);
            }
        }
    }

    /// Where the inventory or crafting window sits, and its slots.
    fn container(&self, workbench: bool) -> ((f32, f32), Vec<(Slot, f32, f32)>) {
        let left = ((self.width - 176.0) / 2.0).floor();
        let top = ((self.height - 166.0) / 2.0).floor();
        let mut slots = Vec::new();
        for column in 0..9 {
            slots.push((Slot::Inventory(column), 8.0 + column as f32 * 18.0, 142.0));
        }
        for row in 0..3 {
            for column in 0..9 {
                slots.push((
                    Slot::Inventory(9 + row * 9 + column),
                    8.0 + column as f32 * 18.0,
                    84.0 + row as f32 * 18.0,
                ));
            }
        }
        if workbench {
            for i in 0..9 {
                slots.push((
                    Slot::Workbench(i),
                    30.0 + (i % 3) as f32 * 18.0,
                    17.0 + (i / 3) as f32 * 18.0,
                ));
            }
            slots.push((Slot::WorkbenchResult, 124.0, 35.0));
        } else {
            for (row, slot) in [39usize, 38, 37, 36].into_iter().enumerate() {
                slots.push((Slot::Inventory(slot), 8.0, 8.0 + row as f32 * 18.0));
            }
            slots.push((Slot::Inventory(40), 77.0, 62.0));
            for i in 0..4 {
                slots.push((
                    Slot::Crafting(i),
                    98.0 + (i % 2) as f32 * 18.0,
                    18.0 + (i / 2) as f32 * 18.0,
                ));
            }
            slots.push((Slot::CraftingResult, 154.0, 28.0));
        }
        (
            (left, top),
            slots
                .into_iter()
                .map(|(slot, x, y)| (slot, left + x, top + y))
                .collect(),
        )
    }

    /// The slot under the mouse, and whether the mouse is outside the window.
    pub fn slot_at(&self, workbench: bool) -> (Option<Slot>, bool) {
        let ((left, top), slots) = self.container(workbench);
        let (mx, my) = self.mouse;
        let hit = slots
            .into_iter()
            .find(|&(_, x, y)| mx >= x - 1.0 && mx < x + 17.0 && my >= y - 1.0 && my < y + 17.0)
            .map(|(slot, ..)| slot);
        let outside = mx < left || my < top || mx >= left + 176.0 || my >= top + 166.0;
        (hit, outside)
    }

    /// The inventory or crafting table screen.
    pub fn container_screen(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        inventory: &Inventory,
        workbench: bool,
    ) {
        self.dim(ui);
        let ((left, top), slots) = self.container(workbench);
        let background = if workbench {
            "crafting_table"
        } else {
            "inventory"
        };
        self.sprite_part(ui, background, left, top, 0.0, 0.0, 176.0, 166.0);
        let grey = rgb(0x404040);
        if workbench {
            self.text(ui, "Crafting", left + 29.0, top + 6.0, grey, false);
            self.text(ui, "Inventory", left + 8.0, top + 72.0, grey, false);
        } else {
            self.text(ui, "Crafting", left + 97.0, top + 8.0, grey, false);
            self.player_model(ui, Screen::Inventory, false);
        }
        let hovered = self.slot_at(workbench).0;
        for (slot, x, y) in slots {
            let stack = match slot {
                Slot::Inventory(index) => inventory.slots.get(index).cloned().flatten(),
                Slot::Crafting(index) => inventory.crafting[index].clone(),
                Slot::CraftingResult => inventory.crafting_output(),
                Slot::Workbench(index) => inventory.workbench[index].clone(),
                Slot::WorkbenchResult => inventory.workbench_output(),
                Slot::Creative(_) | Slot::Destroy => None,
            };
            if stack.is_none() {
                let empty = match slot {
                    Slot::Inventory(39) => Some("helmet_slot"),
                    Slot::Inventory(38) => Some("chestplate_slot"),
                    Slot::Inventory(37) => Some("leggings_slot"),
                    Slot::Inventory(36) => Some("boots_slot"),
                    Slot::Inventory(40) => Some("shield_slot"),
                    _ => None,
                };
                if let Some(name) = empty {
                    self.sprite(ui, name, x, y, 16.0, 16.0);
                }
            }
            if hovered == Some(slot) {
                self.sprite(ui, "slot_highlight_back", x - 4.0, y - 4.0, 24.0, 24.0);
            }
            if let Some(stack) = stack.as_ref() {
                self.item(ui, packs, inventory, stack, x, y);
            }
            if hovered == Some(slot) {
                self.sprite(ui, "slot_highlight_front", x - 4.0, y - 4.0, 24.0, 24.0);
            }
        }
        if let Some(stack) = inventory.cursor.as_ref() {
            let (mx, my) = self.mouse;
            self.item(ui, packs, inventory, stack, mx - 8.0, my - 8.0);
        } else if let Some(slot) = hovered {
            let stack = match slot {
                Slot::Inventory(index) => inventory.slots.get(index).cloned().flatten(),
                Slot::Crafting(index) => inventory.crafting[index].clone(),
                Slot::CraftingResult => inventory.crafting_output(),
                Slot::Workbench(index) => inventory.workbench[index].clone(),
                Slot::WorkbenchResult => inventory.workbench_output(),
                Slot::Creative(_) | Slot::Destroy => None,
            };
            if let Some(stack) = stack {
                self.stack_tooltip(ui, &stack, false);
            }
        }
    }

    /// Where the screen shows the player (`extractEntityInInventoryFollowsMouse`'s
    /// box), and its pixels to a block.
    pub fn player_box(&self, screen: Screen, creative_inventory: bool) -> Option<([f32; 4], f32)> {
        match screen {
            Screen::Inventory => {
                let ((left, top), _) = self.container(false);
                Some(([left + 26.0, top + 8.0, left + 75.0, top + 78.0], 30.0))
            }
            Screen::Creative if creative_inventory => {
                let (left, top) = self.creative_corner();
                Some(([left + 73.0, top + 6.0, left + 105.0, top + 49.0], 20.0))
            }
            _ => None,
        }
    }

    /// The player's picture, drawn this frame into its texture.
    fn player_model(&self, ui: &mut UiList, screen: Screen, creative_inventory: bool) {
        if ui.model.is_none() {
            return;
        }
        if let Some(([x0, y0, x1, y1], _)) = self.player_box(screen, creative_inventory) {
            ui.quad(
                self.model_texture,
                self.rect(x0, y0, x1 - x0, y1 - y0),
                [0.0, 0.0, 1.0, 1.0],
                WHITE,
            );
        }
    }

    fn tooltip(&self, ui: &mut UiList, text: &str) {
        self.tooltip_lines(ui, &[(text.to_owned(), 0xFFFFFF)]);
    }

    /// A stack's tooltip, as `ItemStack.getTooltipLines` gives it, with
    /// the tabs holding it when asked.
    fn stack_tooltip(&self, ui: &mut UiList, stack: &ItemStack, tabs: bool) {
        let mut lines = crate::creative::tooltip(&self.language, stack);
        if tabs {
            let data = crate::creative::data();
            for (i, tab) in data.tabs_holding(stack).into_iter().enumerate() {
                let title = crate::creative::translate(&self.language, &tab.title, &[]);
                lines.insert(1 + i, (title, crate::creative::BLUE));
            }
        }
        self.tooltip_lines(ui, &lines);
    }

    /// `TooltipRenderUtil` and `DefaultTooltipPositioner`: lines in their
    /// colours, the first set 2 pixels apart from the rest.
    fn tooltip_lines(&self, ui: &mut UiList, lines: &[(String, u32)]) {
        if lines.is_empty() {
            return;
        }
        let (mx, my) = self.mouse;
        let width = lines
            .iter()
            .map(|(line, _)| self.font.width(line))
            .fold(0.0, f32::max);
        let height = if lines.len() == 1 {
            8.0
        } else {
            10.0 * lines.len() as f32
        };
        let (mut x, mut y) = (mx + 12.0, my - 12.0);
        if x + width > self.width {
            x = (x - 24.0 - width).max(4.0);
        }
        if y + height + 3.0 > self.height {
            y = self.height - height - 3.0;
        }
        for sprite in ["tooltip_background", "tooltip_frame"] {
            self.sprite(
                ui,
                sprite,
                x - 3.0 - 9.0,
                y - 3.0 - 9.0,
                width + 6.0 + 18.0,
                height + 6.0 + 18.0,
            );
        }
        let mut line_y = y;
        for (i, (line, color)) in lines.iter().enumerate() {
            self.text(ui, line, x, line_y, rgb(*color), true);
            line_y += if i == 0 { 12.0 } else { 10.0 };
        }
    }

    /// A screen's buttons: what each does (nothing for one shown greyed
    /// out), its label, and its corner and width.
    fn buttons(&self, screen: Screen) -> Vec<(Option<Button>, &'static str, f32, f32, f32)> {
        let x = self.width / 2.0 - 102.0;
        let y = self.height / 4.0;
        match screen {
            // Vanilla's game menu, with what this game has no screen for
            // greyed out, so Save and Quit is where it always is.
            Screen::Paused => vec![
                (Some(Button::Resume), "Back to Game", x, y + 8.0, 204.0),
                (None, "Advancements", x, y + 32.0, 98.0),
                (None, "Statistics", x + 106.0, y + 32.0, 98.0),
                (None, "Give Feedback", x, y + 56.0, 98.0),
                (None, "Report Bugs", x + 106.0, y + 56.0, 98.0),
                (None, "Options...", x, y + 80.0, 98.0),
                (None, "Open to LAN", x + 106.0, y + 80.0, 98.0),
                (
                    Some(Button::SaveAndQuit),
                    "Save and Quit",
                    x,
                    y + 104.0,
                    204.0,
                ),
            ],
            Screen::Dead => vec![
                (Some(Button::Respawn), "Respawn", x + 2.0, y + 72.0, 200.0),
                (
                    Some(Button::SaveAndQuit),
                    "Save and Quit",
                    x + 2.0,
                    y + 96.0,
                    200.0,
                ),
            ],
            _ => Vec::new(),
        }
    }

    /// The button under the mouse.
    pub fn button_at(&self, screen: Screen) -> Option<Button> {
        let (mx, my) = self.mouse;
        self.buttons(screen)
            .into_iter()
            .find(|&(_, _, x, y, w)| mx >= x && mx < x + w && my >= y && my < y + 20.0)
            .and_then(|(button, ..)| button)
    }

    pub fn pause_screen(&self, ui: &mut UiList) {
        let active = true;
        self.fill(
            ui,
            0.0,
            0.0,
            self.width,
            self.height,
            [0.06, 0.06, 0.06, 0.6],
        );
        self.centered(ui, "Game Menu", 40.0, WHITE);
        self.draw_buttons(ui, Screen::Paused, active);
    }

    /// The death screen; its buttons wake a second after death.
    pub fn death_screen(&self, ui: &mut UiList, score: u32, active: bool) {
        self.fill(ui, 0.0, 0.0, self.width, self.height, [0.5, 0.0, 0.0, 0.5]);
        let title = "You Died!";
        let big = self.scale * 2.0;
        let x = (self.width * self.scale - self.font.width(title) * big) / 2.0;
        self.font
            .draw(ui, title, x, 30.0 * self.scale, big, WHITE, true);
        let score = format!("Score: {score}");
        self.centered(ui, &score, 100.0, WHITE);
        self.draw_buttons(ui, Screen::Dead, active);
    }

    fn draw_buttons(&self, ui: &mut UiList, screen: Screen, active: bool) {
        let hovered = self.button_at(screen);
        for (button, label, x, y, width) in self.buttons(screen) {
            let enabled = active && button.is_some();
            let sprite = if !enabled {
                "button_disabled"
            } else if hovered == button {
                "button_highlighted"
            } else {
                "button"
            };
            self.sprite(ui, sprite, x, y, width, 20.0);
            let tx = x + (width - self.font.width(label)) / 2.0;
            let colour = if enabled {
                WHITE
            } else {
                crate::font::rgb(0xa0a0a0)
            };
            self.text(ui, label, tx, y + 6.0, colour, true);
        }
    }

    /// The creative screen's corner (`leftPos`, `topPos` for a 195 by 136
    /// window).
    fn creative_corner(&self) -> (f32, f32) {
        (
            ((self.width - 195.0) / 2.0).floor(),
            ((self.height - 136.0) / 2.0).floor(),
        )
    }

    /// The creative screen's slots: a category's 45 cells over the hotbar,
    /// or the survival inventory tab's (`selectTab`'s `SlotWrapper`s) and
    /// its bin.
    fn creative_slots(&self, inventory_tab: bool) -> Vec<(Slot, f32, f32)> {
        let (left, top) = self.creative_corner();
        let mut slots = Vec::new();
        if inventory_tab {
            for (i, slot) in [39usize, 38, 37, 36].into_iter().enumerate() {
                let (column, row) = (i / 2, i % 2);
                slots.push((
                    Slot::Inventory(slot),
                    54.0 + column as f32 * 54.0,
                    6.0 + row as f32 * 27.0,
                ));
            }
            slots.push((Slot::Inventory(40), 35.0, 20.0));
            for i in 9..36 {
                let (column, row) = ((i - 9) % 9, (i - 9) / 9);
                slots.push((
                    Slot::Inventory(i),
                    9.0 + column as f32 * 18.0,
                    54.0 + row as f32 * 18.0,
                ));
            }
            slots.push((Slot::Destroy, 173.0, 112.0));
        } else {
            for i in 0..45 {
                slots.push((
                    Slot::Creative(i),
                    9.0 + (i % 9) as f32 * 18.0,
                    18.0 + (i / 9) as f32 * 18.0,
                ));
            }
        }
        slots.extend((0..9).map(|i| (Slot::Inventory(i), 9.0 + i as f32 * 18.0, 112.0)));
        slots
            .into_iter()
            .map(|(slot, x, y)| (slot, left + x, top + y))
            .collect()
    }

    /// The slot under the mouse on the creative screen, and whether the
    /// mouse is outside its window.
    pub fn creative_slot_at(&self, inventory_tab: bool) -> (Option<Slot>, bool) {
        let (left, top) = self.creative_corner();
        let (mx, my) = self.mouse;
        let hit = self
            .creative_slots(inventory_tab)
            .into_iter()
            .find(|&(_, x, y)| mx >= x - 1.0 && mx < x + 17.0 && my >= y - 1.0 && my < y + 17.0)
            .map(|(slot, ..)| slot);
        (
            hit,
            mx < left || my < top || mx >= left + 195.0 || my >= top + 136.0,
        )
    }

    /// `getTabX` and `getTabY`: a tab's corner from the window's.
    fn tab_corner(tab: &crate::creative::Tab) -> (f32, f32) {
        let x = if tab.aligned_right {
            195.0 - 27.0 * (7.0 - tab.column as f32) + 1.0
        } else {
            27.0 * tab.column as f32
        };
        (x, if tab.top { -32.0 } else { 136.0 })
    }

    /// The tab under the mouse (`checkTabClicked`).
    pub fn creative_tab_at(&self, tabs: &[usize]) -> Option<usize> {
        let data = crate::creative::data();
        let (left, top) = self.creative_corner();
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        tabs.iter().copied().find(|&i| {
            let (x, y) = Self::tab_corner(&data.tabs[i]);
            mx >= x && mx <= x + 26.0 && my >= y && my <= y + 32.0
        })
    }

    /// `insideScrollbar`.
    pub fn creative_in_scrollbar(&self) -> bool {
        let (left, top) = self.creative_corner();
        let (mx, my) = self.mouse;
        mx >= left + 175.0 && my >= top + 18.0 && mx < left + 189.0 && my < top + 130.0
    }

    /// `mouseDragged` on the scroller: where the mouse holds it.
    pub fn creative_scroll_at_mouse(&self) -> f32 {
        let (_, top) = self.creative_corner();
        ((self.mouse.1 - (top + 18.0) - 7.5) / (112.0 - 15.0)).clamp(0.0, 1.0)
    }

    /// `extractTabButton`: the tab's sprite and its icon.
    fn creative_tab(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        inventory: &Inventory,
        index: usize,
        selected: bool,
    ) {
        let tab = &crate::creative::data().tabs[index];
        let (left, top) = self.creative_corner();
        let x = left + Self::tab_corner(tab).0;
        let y = if tab.top {
            top - 28.0
        } else {
            top + 136.0 - 4.0
        };
        let sprite = format!(
            "tab_{}_{}_{}",
            if tab.top { "top" } else { "bottom" },
            if selected { "selected" } else { "unselected" },
            tab.column.min(6) + 1
        );
        self.sprite(ui, &sprite, x, y, 26.0, 32.0);
        let icon = ItemStack::new(tab.icon.clone(), 1);
        let icon_y = y + 16.0 - 8.0 + if tab.top { 1.0 } else { -1.0 };
        self.item(ui, packs, inventory, &icon, x + 13.0 - 8.0, icon_y);
    }

    /// `CreativeModeInventoryScreen`: the shown tabs around the selected
    /// tab's window, its items from `row` on (or the survival inventory),
    /// its scroller, and the search box's text.
    pub fn creative_screen(&mut self, ui: &mut UiList, packs: &PackStack, view: &CreativeView<'_>) {
        let data = crate::creative::data();
        let tab = &data.tabs[view.tab];
        let inventory = view.inventory;
        let inventory_tab = tab.kind == crate::creative::Kind::Inventory;
        self.dim(ui);
        for &i in view.tabs {
            if i != view.tab {
                self.creative_tab(ui, packs, inventory, i, false);
            }
        }
        let (left, top) = self.creative_corner();
        self.sprite_part(
            ui,
            &format!("creative_{}", tab.background),
            left,
            top,
            0.0,
            0.0,
            195.0,
            136.0,
        );
        if let Some((text, cursor)) = view.search {
            // The borderless `EditBox` at (82, 6), 80 wide: as much of the
            // text's end as fits, and the blinking `_` after it.
            let mut shown = text;
            while self.font.width(shown) > 80.0 {
                let mut chars = shown.chars();
                chars.next();
                shown = chars.as_str();
            }
            self.text(ui, shown, left + 82.0, top + 6.0, WHITE, true);
            if cursor {
                let x = left + 82.0 + self.font.width(shown);
                self.text(ui, "_", x, top + 6.0, WHITE, true);
            }
        }
        if tab.scroll_bar {
            let sprite = if view.can_scroll {
                "creative_scroller"
            } else {
                "creative_scroller_disabled"
            };
            let y = top + 18.0 + ((112.0 - 17.0) * view.scroll).floor();
            self.sprite(ui, sprite, left + 175.0, y, 12.0, 15.0);
        }
        self.creative_tab(ui, packs, inventory, view.tab, true);
        self.player_model(ui, Screen::Creative, inventory_tab);
        let (hovered, _) = self.creative_slot_at(inventory_tab);
        let mut hovered_stack = None;
        for (slot, x, y) in self.creative_slots(inventory_tab) {
            let stack = match slot {
                Slot::Creative(i) => view.items.get(view.row * 9 + i).cloned().flatten(),
                Slot::Inventory(i) => inventory.slots[i].clone(),
                _ => None,
            };
            if stack.is_none() {
                let empty = match slot {
                    Slot::Inventory(39) => Some("helmet_slot"),
                    Slot::Inventory(38) => Some("chestplate_slot"),
                    Slot::Inventory(37) => Some("leggings_slot"),
                    Slot::Inventory(36) => Some("boots_slot"),
                    Slot::Inventory(40) => Some("shield_slot"),
                    _ => None,
                };
                if let Some(name) = empty {
                    self.sprite(ui, name, x, y, 16.0, 16.0);
                }
            }
            if hovered == Some(slot) {
                self.sprite(ui, "slot_highlight_back", x - 4.0, y - 4.0, 24.0, 24.0);
            }
            if let Some(stack) = stack.as_ref() {
                self.item(ui, packs, inventory, stack, x, y);
            }
            if hovered == Some(slot) {
                self.sprite(ui, "slot_highlight_front", x - 4.0, y - 4.0, 24.0, 24.0);
                hovered_stack = stack;
            }
        }
        if tab.show_title {
            let title = crate::creative::translate(&self.language, &tab.title, &[]);
            self.text(ui, &title, left + 8.0, top + 6.0, rgb(0x404040), false);
        }
        if let Some(stack) = inventory.cursor.as_ref() {
            let (mx, my) = self.mouse;
            self.item(ui, packs, inventory, stack, mx - 8.0, my - 8.0);
            return;
        }
        // `checkTabHovering`: a tab's title over its icon.
        let (mx, my) = (self.mouse.0 - left, self.mouse.1 - top);
        for &i in view.tabs {
            let (x, y) = Self::tab_corner(&data.tabs[i]);
            if mx >= x + 3.0 && mx < x + 24.0 && my >= y + 3.0 && my < y + 30.0 {
                let title = crate::creative::translate(&self.language, &data.tabs[i].title, &[]);
                self.tooltip(ui, &title);
                return;
            }
        }
        if hovered == Some(Slot::Destroy) {
            let text = crate::creative::translate(&self.language, "inventory.binSlot", &[]);
            self.tooltip(ui, &text);
        } else if let Some(stack) = hovered_stack {
            // `getTooltipFromContainerItem`: a category's own items show
            // no tab names.
            let own = matches!(hovered, Some(Slot::Creative(_)))
                && tab.kind == crate::creative::Kind::Category;
            self.stack_tooltip(ui, &stack, !own);
        }
    }

    /// The title over a dark backdrop, with a line of progress.
    pub fn loading_screen(&self, ui: &mut UiList, status: &str, progress: Option<f32>) {
        if let Some(sprite) = self.sprites.get("menu_background") {
            let tile = 32.0;
            ui.quad(
                sprite.texture,
                self.rect(0.0, 0.0, self.width, self.height),
                [0.0, 0.0, self.width / tile, self.height / tile],
                [0.25, 0.25, 0.25, 1.0],
            );
        } else {
            self.fill(ui, 0.0, 0.0, self.width, self.height, [0.1, 0.1, 0.1, 1.0]);
        }
        let logo_y = 30.0;
        self.sprite(ui, "title", self.width / 2.0 - 128.0, logo_y, 256.0, 64.0);
        self.sprite(
            ui,
            "edition",
            self.width / 2.0 - 64.0,
            logo_y + 49.0,
            128.0,
            16.0,
        );
        self.centered(ui, status, self.height / 2.0 + 10.0, WHITE);
        if let Some(progress) = progress {
            let (w, x, y) = (182.0, self.width / 2.0 - 91.0, self.height / 2.0 + 26.0);
            self.fill(ui, x - 1.0, y - 1.0, w + 2.0, 4.0, [0.5, 0.5, 0.5, 1.0]);
            self.fill(ui, x, y, w, 2.0, [0.0, 0.0, 0.0, 1.0]);
            self.fill(ui, x, y, w * progress.clamp(0.0, 1.0), 2.0, rgb(0x80FF20));
        }
    }
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 4] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b, 1.0]
}
