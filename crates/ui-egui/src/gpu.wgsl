// SliceCraft 3D view. Reversed depth: 1 at the near plane, 0 far away.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    sky_top: vec4<f32>,
    sky_bottom: vec4<f32>,
    // width, depth, grid on, round bed
    bed: vec4<f32>,
    plate: vec4<f32>,
    grid_minor: vec4<f32>,
    grid_major: vec4<f32>,
    // cut z, colour mode, ghost alpha, last layer
    params: vec4<f32>,
    // z low, z high, unused, unused
    ranges: vec4<f32>,
    // kind mask, unused...
    masks: vec4<u32>,
    kinds: array<vec4<f32>, 16>,
    tools: array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<uniform> G: Globals;

struct Lit {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

// Soft studio lighting: a key light over the viewer's shoulder, a sky/ground
// fill, a highlight and a rim, so curved beads and faces read as shapes.
fn shade(base: vec3<f32>, n_in: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let n = normalize(n_in);
    let l = normalize(G.light.xyz);
    let v = normalize(G.eye.xyz - world);
    let diffuse = max(dot(n, l), 0.0);
    // sky above, darker ground below: faces turned down read as underneath
    let fill = mix(0.16, 0.38, n.z * 0.5 + 0.5);
    let h = normalize(l + v);
    let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.18;
    let rim = pow(1.0 - max(dot(n, v), 0.0), 4.0) * 0.06;
    let lit = base * (fill + diffuse * 0.62) + vec3<f32>(spec + rim);
    // soft shoulder instead of a hard clip, so bright colours keep their hue
    return lit / (vec3<f32>(1.0) + max(lit - vec3<f32>(0.8), vec3<f32>(0.0)));
}

// ---------------------------------------------------------------- backdrop

struct Sky {
    @builtin(position) pos: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> Sky {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var out: Sky;
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.t = y * 0.5;
    return out;
}

@fragment
fn fs_sky(in: Sky) -> @location(0) vec4<f32> {
    return vec4<f32>(mix(G.sky_top.rgb, G.sky_bottom.rgb, clamp(in.t, 0.0, 1.0)), 1.0);
}

// ---------------------------------------------------------------- bed plate

struct Plate {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_plate(@builtin(vertex_index) i: u32) -> Plate {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let c = corners[i % 6u];
    let world = vec3<f32>(c.x * G.bed.x, c.y * G.bed.y, -0.02);
    var out: Plate;
    out.pos = G.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    return out;
}

fn grid_line(p: vec2<f32>, step: f32) -> f32 {
    let g = abs(fract(p / step - 0.5) - 0.5) * step / max(fwidth(p), vec2<f32>(1e-4));
    return 1.0 - min(min(g.x, g.y), 1.0);
}

@fragment
fn fs_plate(in: Plate) -> @location(0) vec4<f32> {
    let p = in.world.xy;
    if (G.bed.w > 0.5) {
        let r = G.bed.x * 0.5;
        if (length(p - vec2<f32>(r, r)) > r) {
            discard;
        }
    }
    var c = G.plate.rgb;
    if (G.bed.z > 0.5) {
        c = mix(c, G.grid_minor.rgb, grid_line(p, 10.0) * 0.9);
        c = mix(c, G.grid_major.rgb, grid_line(p, 50.0));
    }
    // a little light falls on the plate too
    let v = normalize(G.eye.xyz - in.world);
    c = c * (0.88 + 0.12 * v.z);
    return vec4<f32>(c, 1.0);
}

// ---------------------------------------------------------------- models

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_mesh(v: MeshIn) -> Lit {
    var out: Lit;
    out.pos = G.view_proj * vec4<f32>(v.pos, 1.0);
    out.world = v.pos;
    out.normal = v.normal;
    out.color = v.color;
    return out;
}

@fragment
fn fs_mesh(in: Lit, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (in.world.z > G.params.x) {
        discard;
    }
    var n = in.normal;
    var base = in.color.rgb;
    if (!front) {
        // inside faces show only through a section cut
        n = -n;
        base = base * 0.4;
    }
    return vec4<f32>(shade(base, n, in.world), 1.0);
}

@fragment
fn fs_ghost(in: Lit) -> @location(0) vec4<f32> {
    return vec4<f32>(shade(in.color.rgb, in.normal, in.world), G.params.z);
}

// ---------------------------------------------------------------- toolpaths

struct PathIn {
    // x: 0 or 1 along the move, -1 and 2 the rounded ends; y: corner 0..3
    @location(0) tmpl: vec2<f32>,
    @location(1) a: vec3<f32>,
    @location(2) b: vec3<f32>,
    // width and height, as fractions of 4 mm
    @location(3) wh: vec2<f32>,
    // speed and flow, 0..1 over the job's range
    @location(4) sf: vec2<f32>,
    // kind | tool << 4 | layer << 8
    @location(5) info: u32,
};

fn ramp(t_in: f32) -> vec3<f32> {
    let t = clamp(t_in, 0.0, 1.0);
    let c0 = vec3<f32>(0.0251, 0.1981, 0.7835);
    let c1 = vec3<f32>(0.0251, 0.5776, 0.4564);
    let c2 = vec3<f32>(0.8879, 0.5841, 0.0723);
    let c3 = vec3<f32>(0.7835, 0.0595, 0.0423);
    if (t < 0.33) {
        return mix(c0, c1, t / 0.33);
    }
    if (t < 0.66) {
        return mix(c1, c2, (t - 0.33) / 0.33);
    }
    return mix(c2, c3, (t - 0.66) / 0.34);
}

@vertex
fn vs_path(v: PathIn) -> Lit {
    var out: Lit;
    let kind = v.info & 15u;
    let tool = (v.info >> 4u) & 15u;
    let layer = f32(v.info >> 8u);
    if (((G.masks.x >> kind) & 1u) == 0u) {
        // hidden: behind the far plane
        out.pos = vec4<f32>(0.0, 0.0, -1.0, 1.0);
        return out;
    }
    let w = max(v.wh.x * 4.0, 0.05);
    let h = max(v.wh.y * 4.0, 0.02);
    var d = v.b - v.a;
    d.z = 0.0;
    let len = length(d);
    var dir = vec3<f32>(1.0, 0.0, 0.0);
    if (len > 1e-6) {
        dir = d / len;
    }
    let up = vec3<f32>(0.0, 0.0, 1.0);
    let side = normalize(cross(up, dir));
    let corner = u32(v.tmpl.y + 0.5);
    var off = vec3<f32>(0.0);
    var n = up;
    switch corner {
        case 0u: { off = side * (w * 0.5); n = side; }
        case 1u: { off = up * (h * 0.5); n = up; }
        case 2u: { off = -side * (w * 0.5); n = -side; }
        default: { off = -up * (h * 0.5); n = -up; }
    }
    var base = v.a;
    if (v.tmpl.x < -0.5) {
        base = v.a - dir * (w * 0.5);
        off = vec3<f32>(0.0);
        n = -dir;
    } else if (v.tmpl.x > 1.5) {
        base = v.b + dir * (w * 0.5);
        off = vec3<f32>(0.0);
        n = dir;
    } else if (v.tmpl.x > 0.5) {
        base = v.b;
    }
    let world = base - up * (h * 0.5) + off;
    var color = G.kinds[kind].rgb;
    let mode = u32(G.params.y + 0.5);
    if (mode == 1u) {
        color = ramp(v.sf.x);
    } else if (mode == 2u) {
        color = ramp((v.a.z - G.ranges.x) / max(G.ranges.y - G.ranges.x, 1e-4));
    } else if (mode == 3u) {
        color = G.tools[tool % 8u].rgb;
    } else if (mode == 4u) {
        color = ramp(v.sf.y);
    }
    // the layer being looked at is a touch brighter than those under it
    if (layer < G.params.w - 0.5) {
        color = color * 0.86;
    }
    out.pos = G.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.normal = n;
    out.color = vec4<f32>(color, 1.0);
    return out;
}

@fragment
fn fs_path(in: Lit) -> @location(0) vec4<f32> {
    return vec4<f32>(shade(in.color.rgb, in.normal, in.world), 1.0);
}

// ---------------------------------------------------------------- lines

struct LineIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var out: LineOut;
    out.pos = G.view_proj * vec4<f32>(v.pos, 1.0);
    out.color = v.color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}
