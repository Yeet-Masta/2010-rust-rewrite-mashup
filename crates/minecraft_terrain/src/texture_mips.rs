//! Mipmaps as 26.3's `MipmapGenerator.generateMipLevels` makes them for a
//! sprite: its `mipmap_strategy` (`auto` meaning `cutout` for a texture
//! with transparent texels, else `mean`), transparent texels first filled
//! with their nearest colour (`TextureUtil.solidify`) or a darkened one
//! (`fillEmptyAreasWithDarkColor`), each level the linear-light mean of
//! four texels (`ARGB.meanLinear`, or `darkenedAlphaBlend`), and cutout
//! levels' alpha scaled to keep the alpha-tested coverage of the first
//! (`scaleAlphaToCoverage`), so cutout plants and leaves neither thin out
//! nor grow dark fringes in the distance.
use image::{Rgba, RgbaImage};
use std::sync::OnceLock;

/// `MipmapStrategy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    Auto,
    Mean,
    Cutout,
    StrictCutout,
    DarkCutout,
}

impl Strategy {
    /// A texture's `.mcmeta` `texture.mipmap_strategy`.
    pub fn parse(name: Option<&str>) -> Self {
        match name {
            Some("mean") => Self::Mean,
            Some("cutout") => Self::Cutout,
            Some("strict_cutout") => Self::StrictCutout,
            Some("dark_cutout") => Self::DarkCutout,
            _ => Self::Auto,
        }
    }
}

/// `ARGB`'s lookup tables: sRGB bytes to linear in 1023ths, and back.
fn tables() -> &'static ([u16; 256], [u8; 1024]) {
    static TABLES: OnceLock<([u16; 256], [u8; 1024])> = OnceLock::new();
    TABLES.get_or_init(|| {
        let to_linear = |x: f32| {
            if x >= 0.04045 {
                ((x as f64 + 0.055) / 1.055).powf(2.4) as f32
            } else {
                x / 12.92
            }
        };
        let to_srgb = |x: f32| {
            if x >= 0.0031308 {
                (1.055 * (x as f64).powf(1.0 / 2.4) - 0.055) as f32
            } else {
                12.92 * x
            }
        };
        let mut linear = [0u16; 256];
        for (i, value) in linear.iter_mut().enumerate() {
            *value = (to_linear(i as f32 / 255.0) * 1023.0).round() as u16;
        }
        let mut srgb = [0u8; 1024];
        for (i, value) in srgb.iter_mut().enumerate() {
            *value = (to_srgb(i as f32 / 1023.0) * 255.0).round() as u8;
        }
        (linear, srgb)
    })
}

/// `ARGB.srgbToLinearChannel`.
fn srgb_to_linear(value: u8) -> f32 {
    tables().0[value as usize] as f32 / 1023.0
}

/// `ARGB.linearToSrgbChannel`.
fn linear_to_srgb(value: f32) -> u8 {
    tables().1[((value * 1023.0).floor() as usize).min(1023)]
}

/// `ARGB.meanLinear`: alpha averaged as stored, colour in linear light.
fn mean_linear(pixels: [Rgba<u8>; 4]) -> Rgba<u8> {
    let (linear, srgb) = tables();
    let channel = |c: usize| {
        let sum: u32 = pixels.iter().map(|p| linear[p[c] as usize] as u32).sum();
        srgb[(sum / 4) as usize]
    };
    let alpha = pixels.iter().map(|p| p[3] as u32).sum::<u32>() / 4;
    Rgba([channel(0), channel(1), channel(2), alpha as u8])
}

/// `MipmapGenerator.darkenedAlphaBlend`: the mean, in linear light, of the
/// texels that are not fully transparent, over all four.
fn darkened_alpha_blend(pixels: [Rgba<u8>; 4]) -> Rgba<u8> {
    let mut total = [0.0f32; 4];
    for pixel in pixels.iter().filter(|p| p[3] != 0) {
        for (i, channel) in [3usize, 0, 1, 2].into_iter().enumerate() {
            total[i] += srgb_to_linear(pixel[channel]);
        }
    }
    let [a, r, g, b] = total.map(|t| linear_to_srgb(t / 4.0));
    Rgba([r, g, b, a])
}

/// `TextureUtil.solidify`: each transparent texel takes the colour of its
/// nearest visible texel (breadth first in x+, x-, y+, y- order), keeping
/// its zero alpha.
fn solidify(image: &mut RgbaImage) {
    let (width, height) = image.dimensions();
    let len = (width * height) as usize;
    let mut nearest = vec![Rgba([0u8; 4]); len];
    let mut distance = vec![u32::MAX; len];
    let mut queue = std::collections::VecDeque::new();
    for x in 0..width {
        for y in 0..height {
            let pixel = *image.get_pixel(x, y);
            if pixel[3] != 0 {
                let at = (x + y * width) as usize;
                distance[at] = 0;
                nearest[at] = pixel;
                queue.push_back((x, y));
            }
        }
    }
    while let Some((x, y)) = queue.pop_front() {
        let at = (x + y * width) as usize;
        for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                continue;
            }
            let next = (nx as u32 + ny as u32 * width) as usize;
            if distance[next] > distance[at] + 1 {
                distance[next] = distance[at] + 1;
                nearest[next] = nearest[at];
                queue.push_back((nx as u32, ny as u32));
            }
        }
    }
    for x in 0..width {
        for y in 0..height {
            let pixel = image.get_pixel_mut(x, y);
            if pixel[3] == 0 {
                let color = nearest[(x + y * width) as usize];
                *pixel = Rgba([color[0], color[1], color[2], 0]);
            }
        }
    }
}

/// `TextureUtil.fillEmptyAreasWithDarkColor`: transparent texels take
/// three quarters of the darkest visible texel's colour (the first darkest
/// scanning x before y).
fn fill_dark(image: &mut RgbaImage) {
    let mut darkest = Rgba([255u8; 4]);
    let mut least = u32::MAX;
    for x in 0..image.width() {
        for y in 0..image.height() {
            let pixel = *image.get_pixel(x, y);
            if pixel[3] != 0 {
                let brightness = pixel[0] as u32 + pixel[1] as u32 + pixel[2] as u32;
                if brightness < least {
                    least = brightness;
                    darkest = pixel;
                }
            }
        }
    }
    let dark = Rgba([
        (3 * darkest[0] as u32 / 4) as u8,
        (3 * darkest[1] as u32 / 4) as u8,
        (3 * darkest[2] as u32 / 4) as u8,
        0,
    ]);
    for pixel in image.pixels_mut() {
        if pixel[3] == 0 {
            *pixel = dark;
        }
    }
}

/// `MipmapGenerator.alphaTestCoverage`: the share of the image, sampled
/// bilinearly at 4 by 4 points between each four texels, whose alpha
/// times `scale` passes `cutoff`.
fn coverage(image: &RgbaImage, cutoff: f32, scale: f32) -> f32 {
    let (width, height) = image.dimensions();
    if width < 2 || height < 2 {
        return 0.0;
    }
    let alpha = |x: u32, y: u32| (image.get_pixel(x, y)[3] as f32 / 255.0 * scale).clamp(0.0, 1.0);
    let mut covered = 0.0f32;
    for y in 0..height - 1 {
        for x in 0..width - 1 {
            let (a00, a10, a01, a11) = (alpha(x, y), alpha(x + 1, y), alpha(x, y + 1), alpha(x + 1, y + 1));
            let mut texel = 0.0f32;
            for sy in 0..4 {
                let fy = (sy as f32 + 0.5) / 4.0;
                for sx in 0..4 {
                    let fx = (sx as f32 + 0.5) / 4.0;
                    let a = a00 * (1.0 - fx) * (1.0 - fy)
                        + a10 * fx * (1.0 - fy)
                        + a01 * (1.0 - fx) * fy
                        + a11 * fx * fy;
                    if a > cutoff {
                        texel += 1.0;
                    }
                }
            }
            covered += texel / 16.0;
        }
    }
    covered / ((width - 1) * (height - 1)) as f32
}

/// `MipmapGenerator.scaleAlphaToCoverage`: five steps of bisection for the
/// alpha scale that keeps `desired` coverage, then every texel's alpha
/// scaled by it, raised by the bias and a fortieth.
fn scale_alpha_to_coverage(image: &mut RgbaImage, desired: f32, cutoff: f32, bias: f32) {
    let (mut low, mut high, mut scale) = (0.0f32, 4.0f32, 1.0f32);
    let (mut best, mut best_error) = (1.0f32, f32::MAX);
    for _ in 0..5 {
        let current = coverage(image, cutoff, scale);
        let error = (current - desired).abs();
        if error < best_error {
            best_error = error;
            best = scale;
        }
        if current < desired {
            low = scale;
        } else if current > desired {
            high = scale;
        } else {
            break;
        }
        scale = (low + high) * 0.5;
    }
    for pixel in image.pixels_mut() {
        let alpha = (pixel[3] as f32 / 255.0 * best + bias + 0.025).clamp(0.0, 1.0);
        // `ARGB.color(float alpha, int rgb)` truncates alpha * 255.
        pixel[3] = (alpha * 255.0) as u8;
    }
}

/// Whether any texel is fully transparent (`Transparency.hasTransparent`).
pub fn has_transparent(image: &RgbaImage) -> bool {
    image.pixels().any(|p| p[3] == 0)
}

/// `MipmapGenerator.generateMipLevels` for one sprite image: level 0 (with
/// its transparent texels filled, unless it is an item texture) and
/// `levels` more, each half the last.
pub fn generate(mut base: RgbaImage, levels: usize, strategy: Strategy, bias: f32, item: bool) -> Vec<RgbaImage> {
    let strategy = match strategy {
        Strategy::Auto if has_transparent(&base) => Strategy::Cutout,
        Strategy::Auto => Strategy::Mean,
        other => other,
    };
    if !item {
        match strategy {
            Strategy::Cutout | Strategy::StrictCutout => solidify(&mut base),
            Strategy::DarkCutout => fill_dark(&mut base),
            _ => {}
        }
    }
    let cutout = matches!(strategy, Strategy::Cutout | Strategy::StrictCutout | Strategy::DarkCutout);
    let cutoff = if strategy == Strategy::StrictCutout { 0.3 } else { 0.5 };
    let original = if cutout { coverage(&base, cutoff, 1.0) } else { 0.0 };
    let mut result = vec![base];
    for _ in 0..levels {
        let previous = result.last().expect("level 0 is there");
        let (width, height) = ((previous.width() / 2).max(1), (previous.height() / 2).max(1));
        let mut next = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let at = |dx: u32, dy: u32| {
                    *previous.get_pixel((x * 2 + dx).min(previous.width() - 1), (y * 2 + dy).min(previous.height() - 1))
                };
                let pixels = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
                *next.get_pixel_mut(x, y) = if strategy == Strategy::DarkCutout {
                    darkened_alpha_blend(pixels)
                } else {
                    mean_linear(pixels)
                };
            }
        }
        if cutout {
            scale_alpha_to_coverage(&mut next, original, cutoff, bias);
        }
        result.push(next);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plant-like sprite: a thin opaque cross on transparent black.
    fn plant() -> RgbaImage {
        let mut image = RgbaImage::new(16, 16);
        for i in 0..16 {
            image.put_pixel(7, i, Rgba([40, 160, 40, 255]));
            image.put_pixel(i, 12, Rgba([40, 160, 40, 255]));
        }
        image
    }

    #[test]
    fn cutout_mips_keep_their_coverage_and_colour() {
        let levels = generate(plant(), 4, Strategy::Auto, 0.0, false);
        assert_eq!(levels.len(), 5);
        assert_eq!(levels[4].dimensions(), (1, 1));
        // Transparent texels took the plant's colour, not black.
        assert_eq!(levels[0].get_pixel(0, 0).0, [40, 160, 40, 0]);
        // A plain box filter leaves level 2's alpha under the cutoff almost
        // everywhere; the coverage-preserving scale keeps the plant visible.
        let visible = levels[2].pixels().filter(|p| p[3] as f32 / 255.0 >= 0.5).count();
        assert!(visible > 0, "the plant survives at a quarter size");
        for pixel in levels[2].pixels() {
            assert_eq!(&pixel.0[..3], &[40, 160, 40], "no dark fringes");
        }
    }

    #[test]
    fn opaque_textures_take_the_linear_mean() {
        let mut image = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 0, 255]));
        image.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
        let levels = generate(image, 1, Strategy::Auto, 0.0, false);
        // A quarter white in linear light is sRGB 137.
        assert_eq!(levels[1].get_pixel(0, 0).0, [137, 137, 137, 255]);
    }
}
