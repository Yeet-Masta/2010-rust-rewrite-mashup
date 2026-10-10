//! Minecraft's bitmap fonts: the default font's `ascii.png` provider, and
//! the `alt` font's standard galactic alphabet (`ascii_sga.png`, the
//! enchanting table's glyphs), with each glyph's width measured from its
//! pixels as vanilla's `BitmapProvider` measures it, and a space four
//! pixels wide.
use std::collections::HashMap;

use minecraft_terrain::pack::{PackStack, ResourceId};

use crate::render::{Renderer, TextureId, UiList};

/// Glyph cell height, in font pixels.
pub const LINE: f32 = 9.0;

struct Glyph {
    /// The cell's corner in the texture, in pixels.
    cell: (u32, u32),
    advance: f32,
}

pub struct Font {
    texture: TextureId,
    size: (f32, f32),
    cell: (u32, u32),
    glyphs: HashMap<char, Glyph>,
}

impl Font {
    /// The default font.
    pub fn load(packs: &PackStack, renderer: &mut Renderer) -> anyhow::Result<Self> {
        Self::bitmap(packs, renderer, "minecraft:include/default", "font/ascii")
    }

    /// The `alt` font (`EnchantmentNames.ALT_FONT`).
    pub fn load_alt(packs: &PackStack, renderer: &mut Renderer) -> anyhow::Result<Self> {
        Self::bitmap(packs, renderer, "minecraft:alt", "font/ascii_sga")
    }

    /// The bitmap provider of the font `definition` drawing from the
    /// texture `file`.
    fn bitmap(
        packs: &PackStack,
        renderer: &mut Renderer,
        definition: &str,
        file: &str,
    ) -> anyhow::Result<Self> {
        let bytes = packs
            .texture(&ResourceId::parse(&format!("minecraft:{file}"))?)?
            .ok_or_else(|| anyhow::anyhow!("the pack has no {file}.png"))?;
        let image = image::load_from_memory(&bytes)?.to_rgba8();
        let definition = packs.font(&ResourceId::parse(definition)?)?;
        let provider = format!("minecraft:{file}.png");
        let rows: Vec<Vec<char>> = definition
            .as_ref()
            .and_then(|d| d["providers"].as_array())
            .and_then(|providers| {
                providers
                    .iter()
                    .find(|p| p["file"].as_str() == Some(provider.as_str()))
            })
            .and_then(|provider| provider["chars"].as_array())
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.as_str())
                    .map(|row| row.chars().collect())
                    .collect()
            })
            .unwrap_or_else(Vec::new);
        let columns = rows.first().map_or(16, Vec::len).max(1) as u32;
        let cell = (
            image.width() / columns,
            image.height() / (rows.len().max(1) as u32),
        );
        let mut glyphs = HashMap::new();
        for (row, chars) in rows.iter().enumerate() {
            for (column, &ch) in chars.iter().enumerate() {
                if ch == '\0' {
                    continue;
                }
                let origin = (column as u32 * cell.0, row as u32 * cell.1);
                // The rightmost column with any opaque pixel.
                let width = (0..cell.0)
                    .rev()
                    .find(|&x| {
                        (0..cell.1).any(|y| image.get_pixel(origin.0 + x, origin.1 + y)[3] > 0)
                    })
                    .map_or(0, |x| x + 1);
                let scale = 8.0 / cell.1 as f32;
                let advance = if ch == ' ' {
                    4.0
                } else {
                    width as f32 * scale + 1.0
                };
                glyphs.insert(
                    ch,
                    Glyph {
                        cell: origin,
                        advance,
                    },
                );
            }
        }
        glyphs.entry(' ').or_insert(Glyph {
            cell: (0, 0),
            advance: 4.0,
        });
        let size = (image.width() as f32, image.height() as f32);
        Ok(Self {
            texture: renderer.add_texture(&image),
            size,
            cell,
            glyphs,
        })
    }

    /// Width of a line in font pixels.
    pub fn width(&self, text: &str) -> f32 {
        text.chars().map(|c| self.advance(c)).sum()
    }

    fn advance(&self, c: char) -> f32 {
        self.glyphs.get(&c).map_or(6.0, |g| g.advance)
    }

    /// `StringSplitter.headByWidth`: the longest start of `text` no wider
    /// than `width`.
    pub fn head_by_width<'a>(&self, text: &'a str, width: f32) -> &'a str {
        let mut left = width;
        for (at, c) in text.char_indices() {
            left -= self.advance(c);
            if left < 0.0 {
                return &text[..at];
            }
        }
        text
    }

    /// Draws text at a GUI point (`scale` window pixels per font pixel),
    /// with vanilla's drop shadow when asked.
    pub fn draw(
        &self,
        ui: &mut UiList,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        colour: [f32; 4],
        shadow: bool,
    ) {
        if shadow {
            let dark = [
                colour[0] * 0.25,
                colour[1] * 0.25,
                colour[2] * 0.25,
                colour[3],
            ];
            self.run(ui, text, x + scale, y + scale, scale, dark);
        }
        self.run(ui, text, x, y, scale, colour);
    }

    fn run(&self, ui: &mut UiList, text: &str, mut x: f32, y: f32, scale: f32, colour: [f32; 4]) {
        let (cw, ch) = (self.cell.0 as f32, self.cell.1 as f32);
        let k = 8.0 / ch;
        for c in text.chars() {
            let Some(glyph) = self.glyphs.get(&c).or_else(|| self.glyphs.get(&'?')) else {
                x += 6.0 * scale;
                continue;
            };
            if c != ' ' {
                let (u, v) = (
                    glyph.cell.0 as f32 / self.size.0,
                    glyph.cell.1 as f32 / self.size.1,
                );
                ui.quad(
                    self.texture,
                    [x, y, cw * k * scale, ch * k * scale],
                    [u, v, u + cw / self.size.0, v + ch / self.size.1],
                    colour,
                );
            }
            x += glyph.advance * scale;
        }
    }
}

/// A colour from vanilla's `0xRRGGBB`.
pub const fn rgb(hex: u32) -> [f32; 4] {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        1.0,
    ]
}
