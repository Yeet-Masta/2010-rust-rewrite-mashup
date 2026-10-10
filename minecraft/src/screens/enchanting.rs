//! `EnchantmentScreen` over its background: the book (`extractBook`),
//! opening while a row offers and leafing when the item changes
//! (`tickBook`), drawn into the model texture as `GuiBookModelRenderer`
//! draws `BookModel`; the three rows with their level sprites, the glyphs
//! `EnchantmentNames` makes in the `alt` font, and the cost, green when
//! affordable; and over everything a hovered row's clue and price.
use glam::{Mat4, Vec3};
use minecraft_terrain::container_render::{BOOK_SHEET, append_book, book_state};
use minecraft_terrain::mesh::{Atlas, ChunkMesh};
use minecraft_terrain::pack::ResourceId;
use minecraftoss_player::inventory::ItemStack;
use minecraftoss_player::menu::enchanting::SEED;
use minecraftoss_player::rng::LegacyRandom;

use super::MenuView;
use crate::creative::{GRAY, RED, enchantment_by_id, enchantment_name, translate};
use crate::font::rgb;
use crate::gui::{Gui, WHITE};
use crate::render::{GuiModel, UiList};

/// `EnchantmentNames.words`.
const WORDS: [&str; 62] = [
    "the",
    "elder",
    "scrolls",
    "klaatu",
    "berata",
    "niktu",
    "xyzzy",
    "bless",
    "curse",
    "light",
    "darkness",
    "fire",
    "air",
    "earth",
    "water",
    "hot",
    "dry",
    "cold",
    "wet",
    "ignite",
    "snuff",
    "embiggen",
    "twist",
    "shorten",
    "stretch",
    "fiddle",
    "destroy",
    "imbue",
    "galvanize",
    "enchant",
    "free",
    "limited",
    "range",
    "of",
    "towards",
    "inside",
    "sphere",
    "cube",
    "self",
    "other",
    "ball",
    "mental",
    "physical",
    "grow",
    "shrink",
    "demon",
    "elemental",
    "spirit",
    "animal",
    "creature",
    "beast",
    "humanoid",
    "undead",
    "fresh",
    "stale",
    "phnglui",
    "mglwnafh",
    "cthulhu",
    "rlyeh",
    "wgahnagl",
    "fhtagn",
    "baguette",
];

/// A row's glyphs (`-9937334`), when hovered (`-128`), and when the
/// player cannot pay (the first halved).
const NAME: u32 = 0x685E4A;
const NAME_HOVERED: u32 = 0xFFFF80;
const NAME_DISABLED: u32 = (NAME & 0xFEFEFE) >> 1;
/// A row's cost (`-8323296`), and when the player cannot pay (`-12550384`).
const COST: u32 = 0x80FF20;
const COST_DISABLED: u32 = 0x407F10;

/// The book's box (`extractBook`): its corner from the window's, and its
/// size.
pub const BOOK_BOX: [f32; 4] = [14.0, 14.0, 38.0, 31.0];

/// The rows' rectangles: 108 by 19 from (60, 14), one under another.
const ROW_X: f32 = 60.0;
const ROW_Y: f32 = 14.0;
const ROW: (f32, f32) = (108.0, 19.0);

/// The row the window point (`x`, `y`) is in (`mouseClicked`).
pub fn row_at(x: f32, y: f32) -> Option<i32> {
    (0..3).find(|&row| {
        let (dx, dy) = (x - ROW_X, y - (ROW_Y + 19.0 * row as f32));
        dx >= 0.0 && dy >= 0.0 && dx < ROW.0 && dy < ROW.1
    })
}

/// `EnchantmentScreen`'s book: its leafing (`flip`, toward `flipT` at
/// `flipA`) and opening, with the last tick's, and the item it last saw.
#[derive(Clone, Debug)]
pub struct EnchantingBook {
    flip: f32,
    o_flip: f32,
    flip_t: f32,
    flip_a: f32,
    open: f32,
    o_open: f32,
    last: Option<ItemStack>,
    random: LegacyRandom,
}

impl EnchantingBook {
    pub fn new(seed: u64) -> Self {
        Self {
            flip: 0.0,
            o_flip: 0.0,
            flip_t: 0.0,
            flip_a: 0.0,
            open: 0.0,
            o_open: 0.0,
            last: None,
            random: LegacyRandom::new(seed),
        }
    }

    /// `tickBook`: another item in the slot leafs the book on by a few
    /// pages; it opens a fifth a tick while a row offers, and closes
    /// otherwise.
    pub fn tick(&mut self, item: Option<&ItemStack>, offers: bool) {
        if item != self.last.as_ref() {
            self.last = item.cloned();
            loop {
                self.flip_t += self.random.next_int(4) as f32 - self.random.next_int(4) as f32;
                if self.flip > self.flip_t + 1.0 || self.flip < self.flip_t - 1.0 {
                    break;
                }
            }
        }
        self.o_flip = self.flip;
        self.o_open = self.open;
        self.open = (self.open + if offers { 0.2 } else { -0.2 }).clamp(0.0, 1.0);
        let diff = ((self.flip_t - self.flip) * 0.4).clamp(-0.2, 0.2);
        self.flip_a += (diff - self.flip_a) * 0.9;
        self.flip += self.flip_a;
    }

    /// The opening and the leafing between the last tick and this one.
    pub fn at(&self, partial: f32) -> (f32, f32) {
        (
            self.o_open + (self.open - self.o_open) * partial,
            self.o_flip + (self.flip - self.o_flip) * partial,
        )
    }
}

/// `GuiBookModelRenderer.renderToTexture` under `PictureInPictureRenderer`:
/// the book, opened to `open` and leafed to `flip`, in a texture of its
/// box's size in window pixels, 40 GUI pixels to a block, 17 down from
/// the box's top; lit as `ENTITY_IN_UI` and at full brightness.
pub fn book_model(atlas: &Atlas, gui_scale: f32, open: f32, flip: f32) -> Option<GuiModel> {
    let sheet = ResourceId::parse(BOOK_SHEET).ok()?;
    if !atlas.contains(&sheet) {
        return None;
    }
    let width = (BOOK_BOX[2] * gui_scale).round().max(1.0);
    let tall = (BOOK_BOX[3] * gui_scale).round().max(1.0);
    let scale = gui_scale * 40.0;
    let shut = 1.0 - open;
    let pose = Mat4::from_translation(Vec3::new(width / 2.0, 17.0 * gui_scale, 0.0))
        * Mat4::from_scale(Vec3::new(scale, scale, -scale))
        * Mat4::from_rotation_y(180f32.to_radians())
        * Mat4::from_rotation_x(25f32.to_radians())
        * Mat4::from_translation(Vec3::new(shut * 0.2, shut * 0.1, shut * 0.25))
        * Mat4::from_rotation_y((-shut * 90.0 - 90.0).to_radians())
        * Mat4::from_rotation_x(180f32.to_radians());
    let mut mesh = ChunkMesh::default();
    let shade = &crate::inventory_player::shade;
    let state = book_state(0.0, flip, open);
    append_book(
        &mut mesh,
        pose,
        atlas.entity_region(&sheet),
        shade,
        [15.0, 15.0],
        state,
    );
    Some(GuiModel {
        vertices: mesh.vertices,
        indices: mesh.indices,
        clip_from_model: crate::inventory_player::clip_from_texture(width, tall),
        size: (width as u32, tall as u32),
    })
}

/// `getRandomName`'s words: three or four (`Util.getRandom`).
fn random_words(random: &mut LegacyRandom) -> String {
    let count = random.next_int(2) + 3;
    let words: Vec<&str> = (0..count)
        .map(|_| WORDS[random.next_int(WORDS.len() as u32) as usize])
        .collect();
    words.join(" ")
}

/// The generator `initSeed` makes of the menu's seed data value, as the
/// data packet's 16 bits leave it.
fn names_random(seed: i32) -> LegacyRandom {
    LegacyRandom::new(i64::from(seed as i16) as u64)
}

/// A data value, 0 for one not there.
fn value(view: &MenuView<'_>, id: usize) -> i32 {
    view.data.get(id).copied().unwrap_or(0)
}

/// `getGoldCount`: the lapis in its slot.
fn lapis(view: &MenuView<'_>) -> i32 {
    view.menu
        .own()
        .get(1)
        .map_or(0, |stack| i32::from(stack.count))
}

impl Gui {
    /// `EnchantmentScreen.extractBackground` after the background: the
    /// book, then each row.
    pub(super) fn enchanting_extras(
        &mut self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        if ui.model.is_some() {
            let [x, y, w, h] = BOOK_BOX;
            let rect = self.rect(left + x, top + y, w, h);
            ui.quad(self.model_texture, rect, [0.0, 0.0, 1.0, 1.0], WHITE);
        }
        // `EnchantmentNames.initSeed` with the seed as the data packet
        // leaves it, its low 16 bits; the offered rows draw in turn.
        let mut names = names_random(value(view, SEED));
        let lapis = lapis(view);
        let (mx, my) = (self.mouse.0.floor() - left, self.mouse.1.floor() - top);
        for row in 0..3 {
            let (x, y) = (left + ROW_X, top + ROW_Y + 19.0 * row as f32);
            let cost = value(view, row);
            if cost == 0 {
                self.sprite(ui, "enchantment_slot_disabled", x, y, ROW.0, ROW.1);
                continue;
            }
            let text = cost.to_string();
            let width = 86.0 - self.font.width(&text);
            let name = self.random_name(&mut names, width);
            let level = row + 1;
            let affordable = (lapis >= level as i32 && view.xp_level >= cost) || view.creative;
            let (name_colour, cost_colour) = if affordable {
                let hovered = row_at(mx, my) == Some(row as i32);
                let slot = if hovered {
                    "enchantment_slot_highlighted"
                } else {
                    "enchantment_slot"
                };
                self.sprite(ui, slot, x, y, ROW.0, ROW.1);
                let sprite = format!("enchanting_level_{level}");
                self.sprite(ui, &sprite, x + 1.0, y + 1.0, 16.0, 16.0);
                (if hovered { NAME_HOVERED } else { NAME }, COST)
            } else {
                self.sprite(ui, "enchantment_slot_disabled", x, y, ROW.0, ROW.1);
                let sprite = format!("enchanting_level_{level}_disabled");
                self.sprite(ui, &sprite, x + 1.0, y + 1.0, 16.0, 16.0);
                (NAME_DISABLED, COST_DISABLED)
            };
            if let Some(font) = &self.alt_font {
                let s = self.scale;
                font.draw(
                    ui,
                    &name,
                    (x + 20.0) * s,
                    (y + 2.0) * s,
                    s,
                    rgb(name_colour),
                    false,
                );
            }
            let cost_x = x + 20.0 + 86.0 - self.font.width(&text);
            self.text(ui, &text, cost_x, y + 9.0, rgb(cost_colour), true);
        }
    }

    /// `EnchantmentNames.getRandomName`: three or four of its words, cut
    /// to `width` in the `alt` font (`StringSplitter.headByWidth`).
    fn random_name(&self, random: &mut LegacyRandom, width: f32) -> String {
        let name = random_words(random);
        match &self.alt_font {
            Some(font) => font.head_by_width(&name, width).to_owned(),
            None => name,
        }
    }

    /// `EnchantmentScreen.extractRenderState`'s tooltip for the first
    /// hovered row (`isHovering`, a pixel around its 108 by 17) that offers
    /// a clue: the enchantment and level, then unless creative what the
    /// row asks.
    pub(super) fn enchanting_tooltip(
        &self,
        ui: &mut UiList,
        view: &MenuView<'_>,
        left: f32,
        top: f32,
    ) {
        let (mx, my) = (self.mouse.0.floor() - left, self.mouse.1.floor() - top);
        let lapis = lapis(view);
        for row in 0..3 {
            let (cost, level) = (value(view, row), value(view, 7 + row));
            let Some(enchantment) = enchantment_by_id(value(view, 4 + row)) else {
                continue;
            };
            let y = ROW_Y + 19.0 * row as f32;
            let hovered = (ROW_X - 1.0..ROW_X + ROW.0 + 1.0).contains(&mx)
                && (y - 1.0..y + 18.0).contains(&my);
            if !hovered || cost <= 0 || level < 0 {
                continue;
            }
            let language = self.language();
            let (name, colour) = enchantment_name(language, enchantment, i64::from(level));
            let clue = translate(language, "container.enchant.clue", &["\0".to_owned()]);
            let (before, after) = clue.split_once('\0').unwrap_or((&clue, ""));
            let white = crate::creative::WHITE;
            let mut lines = vec![vec![
                (before.to_owned(), white),
                (name, colour),
                (after.to_owned(), white),
            ]];
            if !view.creative {
                lines.push(vec![(String::new(), white)]);
                let price = row as i32 + 1;
                if view.xp_level < cost {
                    let text = translate(
                        language,
                        "container.enchant.level.requirement",
                        &[cost.to_string()],
                    );
                    lines.push(vec![(text, RED)]);
                } else {
                    let (lapis_key, level_key) = if price == 1 {
                        ("container.enchant.lapis.one", "container.enchant.level.one")
                    } else {
                        (
                            "container.enchant.lapis.many",
                            "container.enchant.level.many",
                        )
                    };
                    let count = [price.to_string()];
                    let paid = if lapis >= price { GRAY } else { RED };
                    lines.push(vec![(translate(language, lapis_key, &count), paid)]);
                    lines.push(vec![(translate(language, level_key, &count), GRAY)]);
                }
            }
            self.tooltip_runs(ui, &lines);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_and_colours_are_vanillas() {
        assert_eq!(row_at(60.0, 14.0), Some(0));
        assert_eq!(row_at(167.9, 32.9), Some(0));
        assert_eq!(row_at(60.0, 33.0), Some(1));
        assert_eq!(row_at(100.0, 70.9), Some(2));
        assert_eq!(row_at(59.9, 20.0), None);
        assert_eq!(row_at(168.0, 20.0), None);
        assert_eq!(row_at(100.0, 71.0), None);
        assert_eq!(NAME_DISABLED, 0x342F25);
        // The signed ARGB ints vanilla writes them as.
        for (colour, int) in [
            (NAME, -9_937_334),
            (NAME_HOVERED, -128),
            (COST, -8_323_296),
            (COST_DISABLED, -12_550_384),
        ] {
            assert_eq!((0xFF00_0000 | colour) as i32, int);
        }
    }

    #[test]
    fn the_book_opens_with_offers_and_leafs_for_a_new_item() {
        let mut book = EnchantingBook::new(1);
        book.tick(None, false);
        assert_eq!(
            (book.flip_t, book.at(1.0)),
            (0.0, (0.0, 0.0)),
            "nothing new"
        );
        let sword = ItemStack::new("minecraft:diamond_sword", 1);
        book.tick(Some(&sword), true);
        assert!((book.flip_t - book.flip).abs() > 1.0, "{book:?}");
        assert!((book.at(1.0).0 - 0.2).abs() < 1e-6 && book.at(0.5).0 > 0.09);
        for _ in 0..10 {
            book.tick(Some(&sword), true);
        }
        assert_eq!(book.at(0.0).0, 1.0);
        let target = book.flip_t;
        for _ in 0..60 {
            book.tick(Some(&sword), false);
        }
        assert_eq!((book.flip_t, book.at(1.0).0), (target, 0.0));
        assert!((book.flip - target).abs() < 0.05, "it settles on its page");
    }

    #[test]
    fn the_glyphs_follow_the_seed() {
        // The three rows' words for a seed, worked in Java with
        // `java.util.Random((short) seed)`.
        let cases = [
            (
                0,
                [
                    "scrolls humanoid fire earth",
                    "elder niktu bless",
                    "cold spirit imbue curse",
                ],
            ),
            (
                12345,
                [
                    "mglwnafh shorten undead",
                    "snuff grow shrink fiddle",
                    "cold physical darkness",
                ],
            ),
            (
                -1_234_567,
                [
                    "air mglwnafh cold",
                    "phnglui xyzzy bless",
                    "scrolls animal grow",
                ],
            ),
        ];
        for (seed, rows) in cases {
            let mut random = names_random(seed);
            assert_eq!(rows.map(|_| random_words(&mut random)), rows, "seed {seed}");
        }
    }
}
