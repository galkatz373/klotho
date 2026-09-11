// Clustered forward+ PBR. Kitbash positions only; normals from screen derivatives.

struct Frame {
    view_proj: mat4x4<f32>,
    cascade0: mat4x4<f32>,
    cascade1: mat4x4<f32>,
    cascade2: mat4x4<f32>,
    sun_dir_int: vec4<f32>,
    eye: vec4<f32>,
    cascade_splits: vec4<f32>,
    tiles_xy_lights_cascades: vec4<u32>,
    screen: vec4<f32>,
    gi_origin_spacing: vec4<f32>,
    gi_dim_flags: vec4<u32>,
};

struct PointLight {
    pos_radius: vec4<f32>,
    color_int: vec4<f32>,
};

struct Lights {
    items: array<PointLight, 32>,
};

struct Tiles {
    masks: array<vec4<u32>, 144>,
};

struct Object {
    model: mat4x4<f32>,
    albedo: vec4<f32>,
    tag: u32,
    metalness: f32,
    roughness: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<uniform> lights: Lights;
@group(0) @binding(2) var<uniform> tiles: Tiles;
@group(0) @binding(3) var shadow_map: texture_depth_2d_array;
@group(0) @binding(4) var shadow_samp: sampler_comparison;
@group(0) @binding(5) var probe_tex: texture_3d<f32>;
@group(0) @binding(6) var probe_samp: sampler;
@group(1) @binding(0) var<uniform> object: Object;

struct Palette {
    bones: array<mat4x4<f32>, 256>,
};

@group(1) @binding(1) var<uniform> palette: Palette;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

const TAG_EMISSIVE: u32 = 4u;
const PI: f32 = 3.14159265;

@vertex
fn vs(@location(0) pos: vec3<f32>) -> VsOut {
    let world = object.model * vec4<f32>(pos, 1.0);
    var out: VsOut;
    out.clip = frame.view_proj * world;
    out.world = world.xyz;
    return out;
}

fn skin_pos(pos: vec3<f32>, joints: vec4<f32>, weights: vec4<f32>) -> vec4<f32> {
    let j0 = u32(joints.x);
    let j1 = u32(joints.y);
    let j2 = u32(joints.z);
    let j3 = u32(joints.w);
    var skin = weights.x * palette.bones[j0];
    skin += weights.y * palette.bones[j1];
    skin += weights.z * palette.bones[j2];
    skin += weights.w * palette.bones[j3];
    return object.model * skin * vec4<f32>(pos, 1.0);
}

@vertex
fn vs_skinned(
    @location(0) pos: vec3<f32>,
    @location(1) joints: vec4<f32>,
    @location(2) weights: vec4<f32>,
) -> VsOut {
    let world = skin_pos(pos, joints, weights);
    var out: VsOut;
    out.clip = frame.view_proj * world;
    out.world = world.xyz;
    return out;
}

@vertex
fn vs_shadow(@location(0) pos: vec3<f32>, @builtin(instance_index) cascade: u32) -> @builtin(position) vec4<f32> {
    let world = object.model * vec4<f32>(pos, 1.0);
    switch cascade {
        case 1u: { return frame.cascade1 * world; }
        case 2u: { return frame.cascade2 * world; }
        default: { return frame.cascade0 * world; }
    }
}

@vertex
fn vs_shadow_skinned(
    @location(0) pos: vec3<f32>,
    @location(1) joints: vec4<f32>,
    @location(2) weights: vec4<f32>,
    @builtin(instance_index) cascade: u32,
) -> @builtin(position) vec4<f32> {
    let world = skin_pos(pos, joints, weights);
    switch cascade {
        case 1u: { return frame.cascade1 * world; }
        case 2u: { return frame.cascade2 * world; }
        default: { return frame.cascade0 * world; }
    }
}

fn tile_mask(tx: u32, ty: u32) -> u32 {
    let idx = ty * frame.tiles_xy_lights_cascades.x + tx;
    let v = tiles.masks[idx / 4u];
    let c = idx % 4u;
    switch c {
        case 1u: { return v.y; }
        case 2u: { return v.z; }
        case 3u: { return v.w; }
        default: { return v.x; }
    }
}

fn fresnel(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - cos_theta, 5.0);
}

fn d_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d + 1e-5);
}

fn g_schlick(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let gv = n_dot_v / (n_dot_v * (1.0 - k) + k);
    let gl = n_dot_l / (n_dot_l * (1.0 - k) + k);
    return gv * gl;
}

fn brdf(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>, metalness: f32, roughness: f32, radiance: vec3<f32>) -> vec3<f32> {
    let h = normalize(v + l);
    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_v = max(dot(n, v), 0.0);
    let n_dot_h = max(dot(n, h), 0.0);
    let v_dot_h = max(dot(v, h), 0.0);
    let f0 = mix(vec3<f32>(0.04), albedo, metalness);
    let f = fresnel(v_dot_h, f0);
    let d = d_ggx(n_dot_h, max(roughness, 0.04));
    let g = g_schlick(n_dot_v, n_dot_l, roughness);
    let spec = d * g * f / max(4.0 * n_dot_v * n_dot_l, 1e-4);
    let kd = (vec3<f32>(1.0) - f) * (1.0 - metalness);
    return (kd * albedo / PI + spec) * radiance * n_dot_l;
}

fn sky_irradiance(n: vec3<f32>, sun_dir: vec3<f32>) -> vec3<f32> {
    let up = n.y * 0.5 + 0.5;
    let hemi = mix(vec3<f32>(0.03, 0.035, 0.05), vec3<f32>(0.32, 0.40, 0.55), up);
    let sun = pow(max(dot(n, sun_dir), 0.0), 4.0) * vec3<f32>(1.0, 0.92, 0.8) * 0.12;
    return hemi + sun;
}

fn shadow_at(world: vec3<f32>, n: vec3<f32>, sun_dir: vec3<f32>) -> f32 {
    let n_cascades = frame.tiles_xy_lights_cascades.w;
    if n_cascades == 0u {
        return 1.0;
    }
    let to_eye = frame.eye.xyz - world;
    let vz = length(to_eye);
    var layer = 0u;
    if n_cascades > 1u && vz > frame.cascade_splits.x {
        layer = 1u;
    }
    if n_cascades > 2u && vz > frame.cascade_splits.y {
        layer = 2u;
    }
    var lp: vec4<f32>;
    switch layer {
        case 1u: { lp = frame.cascade1 * vec4<f32>(world, 1.0); }
        case 2u: { lp = frame.cascade2 * vec4<f32>(world, 1.0); }
        default: { lp = frame.cascade0 * vec4<f32>(world, 1.0); }
    }
    let ndc = lp.xyz / lp.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 {
        return 1.0;
    }
    let bias = 0.002 + 0.008 * (1.0 - max(dot(n, sun_dir), 0.0));
    return textureSampleCompare(shadow_map, shadow_samp, uv, i32(layer), ndc.z - bias);
}

fn probe_irr(world: vec3<f32>) -> vec3<f32> {
    if (frame.gi_dim_flags.w & 1u) == 0u {
        return vec3<f32>(0.0);
    }
    let origin = frame.gi_origin_spacing.xyz;
    let spacing = frame.gi_origin_spacing.w;
    let dim = vec3<f32>(
        f32(frame.gi_dim_flags.x),
        f32(frame.gi_dim_flags.y),
        f32(frame.gi_dim_flags.z)
    );
    let uvw = (world * 1000.0 - origin) / (spacing * dim);
    let c = clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0));
    return textureSample(probe_tex, probe_samp, c).rgb;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(cross(dpdx(in.world), dpdy(in.world)));
    if object.tag == TAG_EMISSIVE {
        return vec4<f32>(object.albedo.rgb, 1.0);
    }
    let sun_dir = normalize(frame.sun_dir_int.xyz);
    let sun_i = frame.sun_dir_int.w;
    let v = normalize(frame.eye.xyz - in.world);
    let albedo = object.albedo.rgb;
    let metalness = object.metalness;
    let roughness = object.roughness;
    var color = vec3<f32>(0.0);

    let sh = shadow_at(in.world, n, sun_dir);
    let sun_rad = vec3<f32>(1.0, 0.96, 0.88) * sun_i * 3.0;
    color += brdf(n, v, sun_dir, albedo, metalness, roughness, sun_rad) * sh;

    let tiles_x = max(frame.tiles_xy_lights_cascades.x, 1u);
    let tiles_y = max(frame.tiles_xy_lights_cascades.y, 1u);
    let tx = min(u32(clamp(in.clip.x / frame.screen.z, 0.0, f32(tiles_x) - 0.001)), tiles_x - 1u);
    let ty = min(u32(clamp(in.clip.y / frame.screen.w, 0.0, f32(tiles_y) - 0.001)), tiles_y - 1u);
    let mask = tile_mask(tx, ty);
    let light_count = frame.tiles_xy_lights_cascades.z;
    for (var i = 0u; i < light_count; i++) {
        if (mask & (1u << i)) == 0u {
            continue;
        }
        let item = lights.items[i];
        let lvec = item.pos_radius.xyz - in.world;
        let dist = length(lvec);
        let radius = item.pos_radius.w;
        if dist > radius {
            continue;
        }
        let l = lvec / max(dist, 1e-4);
        let attn = saturate(1.0 - dist / radius);
        let rad = item.color_int.rgb * item.color_int.w * attn * attn;
        color += brdf(n, v, l, albedo, metalness, roughness, rad);
    }

    let irr = sky_irradiance(n, sun_dir) + probe_irr(in.world);
    let f0 = mix(vec3<f32>(0.04), albedo, metalness);
    color += irr * albedo * (1.0 - metalness);
    color += irr * f0 * (1.0 - roughness) * 0.25;
    return vec4<f32>(color, 1.0);
}
