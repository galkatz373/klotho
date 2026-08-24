// One shader family: clustered forward, unlit + lambert. No mesh shaders.

struct Frame {
    view_proj: mat4x4<f32>,
    light_dir: vec3<f32>,
    _pad: f32,
};

struct Object {
    model: mat4x4<f32>,
    albedo: vec4<f32>,
    tag: u32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> object: Object;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs(@location(0) pos: vec3<f32>) -> VsOut {
    let world = object.model * vec4<f32>(pos, 1.0);
    var out: VsOut;
    out.clip = frame.view_proj * world;
    out.world = world.xyz;
    return out;
}

const TAG_EMISSIVE: u32 = 4u;

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(cross(dpdx(in.world), dpdy(in.world)));
    let ndotl = max(dot(n, normalize(frame.light_dir)), 0.0);
    if object.tag == TAG_EMISSIVE {
        return vec4<f32>(object.albedo.rgb, 1.0);
    }
    // lambert with a constant ambient; emissive is the unlit permutation
    return vec4<f32>(object.albedo.rgb * (0.25 + 0.75 * ndotl), 1.0);
}
