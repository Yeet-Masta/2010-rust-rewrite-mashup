// Screen-space textured quads: the HUD, menus and text, in window pixels.
struct Screen {
    size: vec4<f32>,
}
@group(0) @binding(0) var<uniform> screen: Screen;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
}

@vertex
fn vertex(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) colour: vec4<f32>) -> Out {
    var out: Out;
    let ndc = position / screen.size.xy * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = uv;
    out.colour = colour;
    return out;
}

@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    let texel = textureSample(image, image_sampler, in.uv) * in.colour;
    if texel.a <= 0.0 {
        discard;
    }
    return texel;
}

// The crosshair inverts what is behind it, as vanilla's does.
@fragment
fn invert(in: Out) -> @location(0) vec4<f32> {
    let texel = textureSample(image, image_sampler, in.uv);
    if texel.a <= 0.0 {
        discard;
    }
    return vec4<f32>(vec3<f32>(texel.a), texel.a);
}
