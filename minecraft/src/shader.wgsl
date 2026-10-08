// MinecraftOSS's environment uniform, in blocks.
struct Environment {
    forward: vec4<f32>, right: vec4<f32>, up: vec4<f32>, camera_pos: vec4<f32>,
    sky: vec4<f32>, fog: vec4<f32>, light: vec4<f32>, sunset: vec4<f32>,
    sun_dir: vec4<f32>, moon_dir: vec4<f32>, cloud: vec4<f32>, params: vec4<f32>,
    extra: vec4<f32>,
    fog_distances: vec4<f32>,
    ambient: vec4<f32>,
    block_tint: vec4<f32>,
}
struct View {
    clip_from_rel: mat4x4<f32>,
    rel_from_clip: mat4x4<f32>,
    hand_clip: mat4x4<f32>,
    // The eye, in blocks.
    camera: vec4<f32>,
    environment: Environment,
}
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
@group(0) @binding(3) var celestials: texture_2d<f32>;
@group(0) @binding(4) var cracks: texture_2d<f32>;

fn rel_from_block(position: vec3<f32>) -> vec3<f32> {
    return position - view.camera.xyz;
}

fn light_brightness(level: f32) -> f32 {
    return level / (4.0 - 3.0 * level);
}

// Minecraft 26.3 lightmap.fsh as MinecraftOSS ports it: ambient, sky light
// scaled by the sky factor, and tinted block light, then the brightness
// option's notGamma blend.
fn lightmap(sky_level: f32, block_level: f32) -> vec3<f32> {
    let environment = view.environment;
    let sky = sky_level / 15.0;
    let block = block_level / 15.0;
    var color = environment.ambient.rgb;
    color += environment.light.rgb * (light_brightness(sky) * environment.light.w);
    let parabolic = (2.0 * block - 1.0) * (2.0 * block - 1.0);
    let block_color = mix(environment.block_tint.rgb, vec3<f32>(1.0), 0.9 * parabolic);
    color += block_color * (light_brightness(block) * environment.block_tint.w);
    color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    let greatest = max(color.r, max(color.g, color.b));
    let inverted = 1.0 - greatest;
    let gamma = color * ((1.0 - inverted * inverted * inverted * inverted) / max(greatest, 0.00001));
    return mix(color, gamma, environment.cloud.w);
}

// Minecraft 26.3 fog.glsl: the larger of spherical environmental fog and
// cylindrical render-distance fog, each linear between its start and end.
fn linear_fog_value(vertex_distance: f32, fog_start: f32, fog_end: f32) -> f32 {
    if vertex_distance <= fog_start { return 0.0; }
    if vertex_distance >= fog_end { return 1.0; }
    return (vertex_distance - fog_start) / (fog_end - fog_start);
}

fn fog_value(world_pos: vec3<f32>) -> f32 {
    let environment = view.environment;
    let pos = world_pos - environment.camera_pos.xyz;
    let spherical = length(pos);
    let cylindrical = max(length(pos.xz), abs(pos.y));
    return max(
        linear_fog_value(spherical, environment.fog_distances.x, environment.fog_distances.y),
        linear_fog_value(cylindrical, environment.fog_distances.z, environment.fog_distances.w),
    );
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) world_pos: vec3<f32>,
}

@vertex
fn vertex(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) light: vec2<f32>,
) -> Out {
    var out: Out;
    out.clip = view.clip_from_rel * vec4<f32>(rel_from_block(position), 1.0);
    out.uv = uv;
    // Unorm of level * 16 back to a level in 0..15.
    let level = light * (255.0 / 16.0);
    out.colour = vec4<f32>(colour.rgb * lightmap(level.x, level.y), colour.a);
    out.world_pos = position;
    return out;
}

// Minecraft's values are display values, as the target's are.
fn shade(in: Out, texel: vec4<f32>) -> vec4<f32> {
    let lit = texel.rgb * in.colour.rgb;
    return vec4<f32>(mix(lit, view.environment.fog.rgb, fog_value(in.world_pos)), texel.a * in.colour.a);
}

@fragment
fn opaque(in: Out) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, in.uv);
    if texel.a < 0.5 {
        discard;
    }
    return vec4<f32>(shade(in, texel).rgb, 1.0);
}

@fragment
fn translucent(in: Out) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, in.uv);
    return shade(in, texel);
}

// Minecraft 26.3 particle.fsh: texture times the lit vertex colour, fogged,
// dropped below a tenth of opacity.
@fragment
fn particle(in: Out) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, in.uv);
    let color = shade(in, texel);
    if color.a < 0.1 {
        discard;
    }
    return color;
}

// The sky, as MinecraftOSS's viewer draws it, behind everything.
struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn sky_vertex(@builtin(vertex_index) index: u32) -> SkyOut {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: SkyOut;
    out.ndc = uv * 2.0 - 1.0;
    out.clip = vec4<f32>(out.ndc, 0.0, 1.0);
    return out;
}

fn celestial(ray: vec3<f32>, direction: vec3<f32>, half_size: f32, slot: f32, moon: bool) -> vec4<f32> {
    let facing = dot(ray, direction);
    if facing <= 0.0 { return vec4<f32>(0.0); }
    // SkyRenderer rotates the XZ quad by Y=-90 degrees and then by the
    // celestial X angle: local +X becomes world +Z, local +Z becomes
    // (-direction.y, direction.x, 0). Its quads sit 100 units from the eye.
    let tangent_u = vec3<f32>(0.0, 0.0, 1.0);
    let tangent_v = vec3<f32>(-direction.y, direction.x, 0.0);
    let projected = ray / facing;
    var uv = vec2<f32>(dot(projected, tangent_u), dot(projected, tangent_v)) / (2.0 * half_size) + 0.5;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) { return vec4<f32>(0.0); }
    // The moon phase quad reverses both texture axes in buildMoonPhases.
    if moon { uv = vec2<f32>(1.0) - uv; }
    let atlas_uv = vec2<f32>((slot + uv.x) / 9.0, uv.y);
    return textureSampleLevel(celestials, atlas_sampler, atlas_uv, 0.0);
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn stars(ray: vec3<f32>) -> f32 {
    let angle = view.environment.params.w;
    let ca = cos(angle);
    let sa = sin(angle);
    let turned = vec3<f32>(ray.x, ray.y * ca - ray.z * sa, ray.y * sa + ray.z * ca);
    let spherical = vec2<f32>(atan2(turned.z, turned.x) / 6.2831853 + 0.5, asin(clamp(turned.y, -1.0, 1.0)) / 3.14159265 + 0.5);
    let cell_uv = spherical * vec2<f32>(320.0, 160.0);
    let cell = floor(cell_uv);
    let seed = hash2(cell);
    if seed < 0.982 { return 0.0; }
    let center = vec2<f32>(hash2(cell + 9.0), hash2(cell + 31.0));
    let size = 0.045 + hash2(cell + 51.0) * 0.035;
    let offset = abs(fract(cell_uv) - center);
    return select(0.0, view.environment.params.z, all(offset < vec2<f32>(size)));
}

@fragment
fn sky_fragment(in: SkyOut) -> @location(0) vec4<f32> {
    let environment = view.environment;
    // The view direction through this pixel: from the near plane to a point
    // further along, so the camera's own movement (view bobbing) cancels.
    let near = view.rel_from_clip * vec4<f32>(in.ndc, 1.0, 1.0);
    let far = view.rel_from_clip * vec4<f32>(in.ndc, 0.01, 1.0);
    let ray = normalize(far.xyz / far.w - near.xyz / near.w);
    // SkyRenderer's 16-block-high fan has a 512-block radius. Its fog value
    // interpolates between the center and rim vertex distances.
    var color = environment.fog.rgb;
    if ray.y > 0.0 {
        let radius = 16.0 * length(ray.xz) / ray.y;
        if radius < 512.0 {
            let vertex_distance = mix(16.0, length(vec2<f32>(512.0, 16.0)), radius / 512.0);
            color = mix(environment.sky.rgb, environment.fog.rgb, clamp(vertex_distance / environment.fog.w, 0.0, 1.0));
        }
    }
    if ray.y > -0.01 {
        let star = stars(ray) * smoothstep(-0.01, 0.07, ray.y);
        color = mix(color, vec3<f32>(1.0), star);
    }
    let sun_horizontal = normalize(vec3<f32>(environment.sun_dir.x, 0.0, environment.sun_dir.z + 0.0001));
    let view_horizontal = normalize(vec3<f32>(ray.x, 0.0, ray.z + 0.0001));
    let sunset = environment.sunset.a * pow(max(dot(sun_horizontal, view_horizontal), 0.0), 8.0)
        * (1.0 - smoothstep(0.0, 0.35, abs(ray.y)));
    color = mix(color, environment.sunset.rgb, sunset);
    let sun = celestial(ray, environment.sun_dir.xyz, 0.3, 0.0, false);
    // Minecraft's celestial pipeline uses OVERLAY (source alpha, destination one).
    color = min(color + sun.rgb * sun.a * environment.sun_dir.w, vec3<f32>(1.0));
    let moon = celestial(ray, environment.moon_dir.xyz, 0.2, environment.extra.x + 1.0, true);
    color = min(color + moon.rgb * moon.a * environment.moon_dir.w, vec3<f32>(1.0));
    return vec4<f32>(color, 1.0);
}

struct CloudOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec4<f32>,
    @location(1) distance: f32,
}

@vertex
fn cloud_vertex(@location(0) position: vec3<f32>, @location(1) colour: vec4<f32>) -> CloudOut {
    // CloudRenderer moves the texture at 0.03 blocks per game tick, with a
    // fixed 3.96-block Z phase. Geometry is rebuilt only on cell boundaries.
    let world = position - vec3<f32>(view.environment.extra.y, 0.0, 3.96);
    var out: CloudOut;
    out.clip = view.clip_from_rel * vec4<f32>(rel_from_block(world), 1.0);
    out.colour = colour;
    out.distance = distance(world, view.environment.camera_pos.xyz);
    return out;
}

@fragment
fn cloud_fragment(in: CloudOut) -> @location(0) vec4<f32> {
    let alpha = 0.8 * (1.0 - clamp(in.distance / 1024.0, 0.0, 1.0));
    return vec4<f32>(view.environment.cloud.rgb * in.colour.rgb, alpha);
}

// Mobs and dropped items. The entity overlay rides in vertex alpha: negative
// for the hurt flash's red, else the white's alpha. Entity textures have no
// mipmaps.
struct EntityOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) world_pos: vec3<f32>,
    @location(3) light: vec3<f32>,
}

@vertex
fn entity_vertex(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) sky_light: f32,
    @location(4) block_light: f32,
) -> EntityOut {
    var out: EntityOut;
    out.clip = view.clip_from_rel * vec4<f32>(rel_from_block(position), 1.0);
    out.uv = uv;
    let light = lightmap(sky_light, block_light);
    out.colour = vec4<f32>(colour.rgb * light, colour.a);
    out.world_pos = position;
    out.light = light;
    return out;
}

@fragment
fn entity_fragment(in: EntityOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0);
    if texel.a < 0.1 {
        discard;
    }
    let keep = abs(in.colour.a);
    let overlay = select(vec3<f32>(1.0), vec3<f32>(1.0, 0.0, 0.0), in.colour.a < 0.0);
    let lit = texel.rgb * in.colour.rgb * keep + overlay * (1.0 - keep) * in.light;
    return vec4<f32>(mix(lit, view.environment.fog.rgb, fog_value(in.world_pos)), 1.0);
}

@fragment
fn entity_translucent_fragment(in: EntityOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0);
    if texel.a < 0.1 {
        discard;
    }
    let lit = texel.rgb * in.colour.rgb;
    return vec4<f32>(mix(lit, view.environment.fog.rgb, fog_value(in.world_pos)), texel.a * in.colour.a);
}

// An entity model in the UI (`ENTITY_IN_UI` lighting, baked into the
// colour): no lightmap, no fog.
@vertex
fn gui_entity_vertex(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) sky_light: f32,
    @location(4) block_light: f32,
) -> EntityOut {
    var out: EntityOut;
    out.clip = view.clip_from_rel * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.colour = colour;
    out.world_pos = position;
    out.light = vec3<f32>(1.0);
    return out;
}

@fragment
fn gui_entity_fragment(in: EntityOut) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0);
    if texel.a < 0.1 {
        discard;
    }
    return vec4<f32>(texel.rgb * in.colour.rgb, 1.0);
}

// Black, as dark as the shadow sprite and the vertex alpha.
@fragment
fn shadow_fragment(in: EntityOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, textureSample(atlas, atlas_sampler, in.uv).a * in.colour.a);
}

// The first-person hand: view-space vertices under the hand projection,
// lit by the lightmap, cut out as items are (below 0.1).
@vertex
fn hand_vertex(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) light: vec2<f32>,
) -> Out {
    var out: Out;
    out.clip = view.hand_clip * vec4<f32>(position, 1.0);
    out.uv = uv;
    let level = light * (255.0 / 16.0);
    out.colour = vec4<f32>(colour.rgb * lightmap(level.x, level.y), colour.a);
    out.world_pos = view.environment.camera_pos.xyz;
    return out;
}

@fragment
fn hand_fragment(in: Out) -> @location(0) vec4<f32> {
    let texel = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0);
    if texel.a < 0.1 {
        discard;
    }
    return vec4<f32>(texel.rgb * in.colour.rgb, 1.0);
}

// The destroy stage over a block being mined, blended as the crumbling
// render type: source times destination, twice.
struct CrackOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn crack_vertex(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>) -> CrackOut {
    var out: CrackOut;
    out.clip = view.clip_from_rel * vec4<f32>(rel_from_block(position), 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn crack_fragment(in: CrackOut) -> @location(0) vec4<f32> {
    let colour = textureSample(cracks, atlas_sampler, in.uv);
    if colour.a < 0.1 {
        discard;
    }
    return colour;
}

// The outline around the block the player looks at.
@vertex
fn outline_vertex(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    let rel = rel_from_block(position);
    var clip = view.clip_from_rel * vec4<f32>(rel, 1.0);
    // Pulled a hair towards the eye, so faces do not hide it.
    clip.z = clip.z * 1.002;
    return clip;
}

@fragment
fn outline_fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 0.4);
}
