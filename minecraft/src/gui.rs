//! The 2D layer, drawn as vanilla draws it from the pack's own sprites: the
//! hotbar, hearts, hunger, armor, air and experience; the inventory and
//! crafting table screens; the pause, death and loading screens. Layout is
//! in GUI pixels at vanilla's automatic GUI scale.
use std::collections::HashMap;

use image::RgbaImage;
use minecraft_terrain::pack::{PackStack, ResourceId};
use minecraftoss_player::inventory::{Inventory, ItemStack};
use minecraftoss_player::survival::SurvivalStatus;

use crate::font::{Font, LINE, rgb};
use crate::render::{Renderer, TextureId, UiList};

const ICON: u32 = 32;
const ICON_ATLAS: u32 = ICON * 64;
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
}

/// What is on screen over the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Playing,
    Inventory,
    Crafting,
    Creative,
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
}

struct Icons {
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
    sprites: HashMap<&'static str, Sprite>,
    icons: Icons,
    language: HashMap<String, String>,
    /// Window pixels per GUI pixel.
    pub scale: f32,
    pub width: f32,
    pub height: f32,
    /// The mouse in GUI pixels.
    pub mouse: (f32, f32),
}

const SPRITES: [(&str, &str, f32); 36] = [
    (
        "creative_items",
        "gui/container/creative_inventory/tab_items",
        0.0,
    ),
    (
        "creative_scroller",
        "gui/sprites/container/creative_inventory/scroller",
        0.0,
    ),
    ("hotbar", "gui/sprites/hud/hotbar", 0.0),
    ("hotbar_selection", "gui/sprites/hud/hotbar_selection", 0.0),
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
    ("steve", "entity/player/wide/steve", 0.0),
    ("underwater", "misc/underwater", 0.0),
    ("vignette", "misc/vignette", 0.0),
    ("helmet_slot", "gui/sprites/container/slot/helmet", 0.0),
    (
        "chestplate_slot",
        "gui/sprites/container/slot/chestplate",
        0.0,
    ),
    ("shield_slot", "gui/sprites/container/slot/shield", 0.0),
];

impl Gui {
    pub fn load(packs: &PackStack, renderer: &mut Renderer) -> anyhow::Result<Self> {
        let font = Font::load(packs, renderer)?;
        let mut sprites = HashMap::new();
        for (name, path, border) in SPRITES {
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
        for (name, path) in [
            ("leggings_slot", "gui/sprites/container/slot/leggings"),
            ("boots_slot", "gui/sprites/container/slot/boots"),
        ] {
            if let Ok(id) = ResourceId::parse(&format!("minecraft:{path}"))
                && let Some(bytes) = packs.texture(&id)?
            {
                let image = image::load_from_memory(&bytes)?.to_rgba8();
                let size = (image.width() as f32, image.height() as f32);
                sprites.insert(
                    name,
                    Sprite {
                        texture: renderer.add_texture(&image),
                        size,
                        border: 0.0,
                    },
                );
            }
        }
        Ok(Self {
            font,
            sprites,
            icons: Icons {
                image: RgbaImage::new(ICON_ATLAS, ICON_ATLAS),
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
        self.width = w / scale;
        self.height = h / scale;
        self.mouse = (mouse.0 / scale, mouse.1 / scale);
    }

    /// An item's display name.
    pub fn item_name(&self, id: &str) -> String {
        minecraft_terrain::item_icons::item_name(&self.language, id)
    }

    /// Uploads item icons made this frame.
    pub fn flush(&mut self, renderer: &mut Renderer) {
        self.icons.started = None;
        let Some(texture) = self.icons.texture else {
            self.icons.texture = Some(renderer.add_texture(&self.icons.image));
            self.icons.fresh.clear();
            return;
        };
        for (x, y) in std::mem::take(&mut self.icons.fresh) {
            let cell = image::imageops::crop_imm(&self.icons.image, x, y, ICON, ICON).to_image();
            renderer.update_region(texture, x, y, &cell);
        }
    }

    fn icon(&mut self, packs: &PackStack, id: &str) -> Option<[f32; 4]> {
        if let Some(rect) = self.icons.cells.get(id) {
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
        let per_row = ICON_ATLAS / ICON;
        let rect = if self.icons.next >= per_row * per_row {
            None
        } else {
            match minecraft_terrain::item_icons::item_icon(packs, id, ICON as usize) {
                Ok(Some(icon)) => {
                    let cell = self.icons.next;
                    self.icons.next += 1;
                    let (x, y) = ((cell % per_row) * ICON, (cell / per_row) * ICON);
                    let icon = image::imageops::resize(
                        &icon,
                        ICON,
                        ICON,
                        image::imageops::FilterType::Nearest,
                    );
                    image::imageops::replace(
                        &mut self.icons.image,
                        &icon,
                        i64::from(x),
                        i64::from(y),
                    );
                    self.icons.fresh.push((x, y));
                    let size = ICON_ATLAS as f32;
                    Some([
                        x as f32 / size,
                        y as f32 / size,
                        (x + ICON) as f32 / size,
                        (y + ICON) as f32 / size,
                    ])
                }
                _ => None,
            }
        };
        self.icons.cells.insert(id.to_owned(), rect);
        rect
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
    #[allow(clippy::too_many_arguments)]
    fn item(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        inventory: &Inventory,
        stack: &ItemStack,
        x: f32,
        y: f32,
    ) {
        if let Some(rect) = self.icon(packs, &stack.id)
            && let Some(texture) = self.icons.texture
        {
            ui.quad(texture, self.rect(x, y, 16.0, 16.0), rect, WHITE);
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
        if stack.count > 1 {
            let count = stack.count.to_string();
            let width = self.font.width(&count);
            self.text(
                ui,
                &count,
                x + 19.0 - 2.0 - width,
                y + 6.0 + 3.0,
                WHITE,
                true,
            );
        }
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
        self.fill(
            ui,
            0.0,
            0.0,
            self.width,
            self.height,
            [0.06, 0.06, 0.06, 0.75],
        );
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
            self.player_figure(ui, left + 26.0, top + 8.0);
        }
        let hovered = self.slot_at(workbench).0;
        for (slot, x, y) in slots {
            let stack = match slot {
                Slot::Inventory(index) => inventory.slots.get(index).cloned().flatten(),
                Slot::Crafting(index) => inventory.crafting[index].clone(),
                Slot::CraftingResult => inventory.crafting_output(),
                Slot::Workbench(index) => inventory.workbench[index].clone(),
                Slot::WorkbenchResult => inventory.workbench_output(),
                Slot::Creative(_) => None,
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
                Slot::Creative(_) => None,
            };
            if let Some(stack) = stack {
                let name = self.item_name(&stack.id);
                self.tooltip(ui, &name);
            }
        }
    }

    /// The player, front on, in the inventory's character box.
    fn player_figure(&self, ui: &mut UiList, x: f32, y: f32) {
        let Some(skin) = self.sprites.get("steve") else {
            return;
        };
        let k = 1.75;
        let (cx, top) = (x + 49.0 / 2.0, y + 7.0);
        // Skin rectangles (u, v, w, h) and their place on the figure.
        let parts: [([f32; 4], f32, f32); 6] = [
            ([8.0, 8.0, 8.0, 8.0], -4.0, 0.0),
            ([20.0, 20.0, 8.0, 12.0], -4.0, 8.0),
            ([44.0, 20.0, 4.0, 12.0], -8.0, 8.0),
            ([36.0, 52.0, 4.0, 12.0], 4.0, 8.0),
            ([4.0, 20.0, 4.0, 12.0], -4.0, 20.0),
            ([20.0, 52.0, 4.0, 12.0], 0.0, 20.0),
        ];
        let overlay: [([f32; 4], f32, f32); 1] = [([40.0, 8.0, 8.0, 8.0], -4.0, 0.0)];
        for ([u, v, w, h], dx, dy) in parts.into_iter().chain(overlay) {
            ui.quad(
                skin.texture,
                self.rect(cx + dx * k, top + dy * k, w * k, h * k),
                [
                    u / skin.size.0,
                    v / skin.size.1,
                    (u + w) / skin.size.0,
                    (v + h) / skin.size.1,
                ],
                WHITE,
            );
        }
    }

    fn tooltip(&self, ui: &mut UiList, text: &str) {
        let (mx, my) = self.mouse;
        let width = self.font.width(text);
        let (mut x, mut y) = (mx + 12.0, my - 12.0);
        if x + width + 4.0 > self.width {
            x = (mx - 16.0 - width).max(4.0);
        }
        y = y.max(4.0);
        self.sprite(
            ui,
            "tooltip_background",
            x - 3.0 - 9.0,
            y - 3.0 - 9.0,
            width + 6.0 + 18.0,
            8.0 + 6.0 + 18.0,
        );
        self.sprite(
            ui,
            "tooltip_frame",
            x - 3.0 - 9.0,
            y - 3.0 - 9.0,
            width + 6.0 + 18.0,
            8.0 + 6.0 + 18.0,
        );
        self.text(ui, text, x, y, WHITE, true);
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

    /// The creative screen's corner and its slots: the item list's cells,
    /// then the hotbar.
    fn creative(&self) -> ((f32, f32), Vec<(Slot, f32, f32)>) {
        let left = ((self.width - 195.0) / 2.0).floor();
        let top = ((self.height - 136.0) / 2.0).floor();
        let mut slots: Vec<(Slot, f32, f32)> = (0..45)
            .map(|i| {
                (
                    Slot::Creative(i),
                    9.0 + (i % 9) as f32 * 18.0,
                    18.0 + (i / 9) as f32 * 18.0,
                )
            })
            .collect();
        slots.extend((0..9).map(|i| (Slot::Inventory(i), 9.0 + i as f32 * 18.0, 112.0)));
        (
            (left, top),
            slots
                .into_iter()
                .map(|(slot, x, y)| (slot, left + x, top + y))
                .collect(),
        )
    }

    /// The slot under the mouse on the creative screen, and whether the
    /// mouse is outside its window.
    pub fn creative_slot_at(&self) -> (Option<Slot>, bool) {
        let ((left, top), slots) = self.creative();
        let (mx, my) = self.mouse;
        let hit = slots
            .into_iter()
            .find(|&(_, x, y)| mx >= x - 1.0 && mx < x + 17.0 && my >= y - 1.0 && my < y + 17.0)
            .map(|(slot, ..)| slot);
        (
            hit,
            mx < left || my < top || mx >= left + 195.0 || my >= top + 136.0,
        )
    }

    /// Every item, nine to a row from `first` on, over the hotbar.
    pub fn creative_screen(
        &mut self,
        ui: &mut UiList,
        packs: &PackStack,
        inventory: &Inventory,
        items: &[ItemStack],
        first: usize,
        scroll: f32,
    ) {
        self.fill(
            ui,
            0.0,
            0.0,
            self.width,
            self.height,
            [0.06, 0.06, 0.06, 0.75],
        );
        let ((left, top), slots) = self.creative();
        self.sprite_part(ui, "creative_items", left, top, 0.0, 0.0, 195.0, 136.0);
        self.text(
            ui,
            "Creative Items",
            left + 8.0,
            top + 6.0,
            rgb(0x404040),
            false,
        );
        self.sprite(
            ui,
            "creative_scroller",
            left + 175.0,
            top + 18.0 + (112.0 - 17.0) * scroll.clamp(0.0, 1.0),
            12.0,
            15.0,
        );
        let hovered = self.creative_slot_at().0;
        let mut tooltip = None;
        for (slot, x, y) in slots {
            let stack = match slot {
                Slot::Creative(i) => items.get(first + i).cloned(),
                Slot::Inventory(i) => inventory.slots[i].clone(),
                _ => None,
            };
            if hovered == Some(slot) {
                self.sprite(ui, "slot_highlight_back", x - 4.0, y - 4.0, 24.0, 24.0);
            }
            if let Some(stack) = stack.as_ref() {
                self.item(ui, packs, inventory, stack, x, y);
                if hovered == Some(slot) {
                    tooltip = Some(self.item_name(&stack.id));
                }
            }
            if hovered == Some(slot) {
                self.sprite(ui, "slot_highlight_front", x - 4.0, y - 4.0, 24.0, 24.0);
            }
        }
        if let Some(stack) = inventory.cursor.as_ref() {
            let (mx, my) = self.mouse;
            self.item(ui, packs, inventory, stack, mx - 8.0, my - 8.0);
        } else if let Some(name) = tooltip {
            self.tooltip(ui, &name);
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
