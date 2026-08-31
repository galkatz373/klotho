// UV from the sampled color size so half-res passes stay independent of the
// full-res composite uniform.

struct Post {
    screen: vec2<f32>,
    flags: u32,
    _pad: u32,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> post: Post;
@group(0) @binding(3) var depth_tex: texture_depth_2d;
@group(0) @binding(4) var depth_samp: sampler;
@group(0) @binding(5) var ssgi_tex: texture_2d<f32>;
@group(0) @binding(6) var bloom_tex: texture_2d<f32>;
@group(0) @binding(7) var history_tex: texture_2d<f32>;

@vertex
fn vs_fs(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    return vec4<f32>(pos[vid], 0.0, 1.0);
}

fn uv_of(p: vec4<f32>) -> vec2<f32> {
    let dim = vec2<f32>(textureDimensions(src));
    return p.xy / max(dim, vec2<f32>(1.0));
}

@fragment
fn fs_blit(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return textureSample(src, samp, uv_of(pos));
}

@fragment
fn fs_bloom(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureSample(src, samp, uv_of(pos)).rgb;
    let lum = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    let b = max(c - vec3<f32>(0.75), vec3<f32>(0.0)) * smoothstep(0.6, 1.2, lum);
    return vec4<f32>(b, 1.0);
}

@fragment
fn fs_ssgi(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    let depth = textureSample(depth_tex, depth_samp, uv);
    if depth >= 0.999 {
        return vec4<f32>(0.0);
    }
    var acc = vec3<f32>(0.0);
    let dirs = array<vec2<f32>, 4>(
        vec2<f32>(0.012, 0.0),
        vec2<f32>(-0.008, 0.010),
        vec2<f32>(-0.008, -0.010),
        vec2<f32>(0.0, 0.012)
    );
    for (var i = 0; i < 4; i++) {
        let suv = clamp(uv + dirs[i], vec2<f32>(0.0), vec2<f32>(1.0));
        let sd = textureSample(depth_tex, depth_samp, suv);
        if sd < depth - 0.001 {
            acc += textureSample(src, samp, suv).rgb;
        }
    }
    return vec4<f32>(acc * 0.12, 1.0);
}

@fragment
fn fs_composite(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    var color = textureSample(src, samp, uv).rgb;
    if (post.flags & 1u) != 0u {
        color += textureSample(ssgi_tex, samp, uv).rgb;
    }
    if (post.flags & 2u) != 0u {
        color += textureSample(bloom_tex, samp, uv).rgb * 0.65;
    }
    if (post.flags & 4u) != 0u {
        let px = vec2<f32>(1.0, 0.0) / max(post.screen, vec2<f32>(1.0));
        let py = vec2<f32>(0.0, 1.0) / max(post.screen, vec2<f32>(1.0));
        let h = textureSample(history_tex, samp, uv).rgb;
        let n = textureSample(history_tex, samp, uv + px).rgb
            + textureSample(history_tex, samp, uv - px).rgb
            + textureSample(history_tex, samp, uv + py).rgb
            + textureSample(history_tex, samp, uv - py).rgb;
        let neighborhood = (h + n) * 0.2;
        color = mix(color, neighborhood, 0.12);
    }
    return vec4<f32>(color, 1.0);
}
