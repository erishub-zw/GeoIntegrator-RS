struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct CameraUniform {
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    params: vec4<f32>, // x=aspect, y=tan_half_fov, z=exposure, w=gamma
    params2: vec4<f32>, // x=debug_direction_view
    position: vec4<f32>,
};

struct GravityParams {
    params: vec4<f32>, // x=mass, y=spin, z=charge, w=horizon_radius
    params2: vec4<f32>, // x=is_wormhole, y=integrator_enabled, z=debug_steps_view, w=adaptive_step
    params3: vec4<f32>, // x=step_size, y=min_step_size, z=max_step_size, w=max_steps
    params4: vec4<f32>, // x=escape_radius, y=adaptive_radius_scale
};

struct RayState {
    pos: vec3<f32>,
    dir: vec3<f32>,
};

struct RayDeriv {
    dpos: vec3<f32>,
    ddir: vec3<f32>,
};

@group(0) @binding(0) var sky_tex: texture_2d<f32>;
@group(0) @binding(1) var sky_sampler: sampler;
@group(0) @binding(2) var<uniform> camera: CameraUniform;
@group(0) @binding(3) var<uniform> gravity: GravityParams;
@group(0) @binding(4) var sky_tex_b: texture_2d<f32>;

fn world_dir_to_sky_uv(ray_world: vec3<f32>) -> vec2<f32> {
    let pi = 3.1415926535;
    let phi = atan2(ray_world.z, ray_world.x);
    let theta = acos(clamp(ray_world.y, -1.0, 1.0));
    return vec2<f32>(phi / (2.0 * pi) + 0.5, theta / pi);
}

fn vec3_is_finite(v: vec3<f32>) -> bool {
    let sane_limit = vec3<f32>(1.0e19);
    return all(v == v) && all(abs(v) < sane_limit);
}

fn ray_state_is_finite(state: RayState) -> bool {
    return vec3_is_finite(state.pos) && vec3_is_finite(state.dir);
}

fn gravity_accel(pos: vec3<f32>, dir: vec3<f32>, mass: f32) -> vec3<f32> {
    let r = max(length(pos), 1.0e-5);
    let inv_r3 = 1.0 / (r * r * r);
    let radial = -2.0 * mass * pos * inv_r3;
    return radial - dot(radial, dir) * dir;
}

fn evaluate_derivative(state: RayState, mass: f32) -> RayDeriv {
    let safe_dir = normalize(state.dir);
    return RayDeriv(
        safe_dir,
        gravity_accel(state.pos, safe_dir, mass),
    );
}

fn add_scaled(state: RayState, deriv: RayDeriv, h: f32) -> RayState {
    return RayState(
        state.pos + deriv.dpos * h,
        normalize(state.dir + deriv.ddir * h),
    );
}

fn rk4_step(state: RayState, h: f32, mass: f32) -> RayState {
    let k1 = evaluate_derivative(state, mass);
    let k2 = evaluate_derivative(add_scaled(state, k1, 0.5 * h), mass);
    let k3 = evaluate_derivative(add_scaled(state, k2, 0.5 * h), mass);
    let k4 = evaluate_derivative(add_scaled(state, k3, h), mass);
    let dpos = (k1.dpos + 2.0 * k2.dpos + 2.0 * k3.dpos + k4.dpos) * (h / 6.0);
    let ddir = (k1.ddir + 2.0 * k2.ddir + 2.0 * k3.ddir + k4.ddir) * (h / 6.0);
    return RayState(state.pos + dpos, normalize(state.dir + ddir));
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );

    let p = positions[idx];
    var out: VsOut;
    out.clip_pos = vec4<f32>(p, 0.0, 1.0);
    out.uv = p * 0.5 + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let ndc = uv * 2.0 - vec2<f32>(1.0, 1.0);
    let aspect = camera.params.x;
    let tan_half_fov = camera.params.y;
    let exposure = camera.params.z;
    let gamma = camera.params.w;
    let debug_direction_view = camera.params2.x;
    let cam_pos = camera.position.xyz;
    let mass = gravity.params.x;
    let spin = gravity.params.y;
    let charge = gravity.params.z;
    let horizon_radius = gravity.params.w;
    let is_wormhole = gravity.params2.x;
    let integrator_enabled = gravity.params2.y;
    let debug_steps_view = gravity.params2.z;
    let adaptive_step = gravity.params2.w;
    let base_step = gravity.params3.x;
    let min_step = gravity.params3.y;
    let max_step = gravity.params3.z;
    let max_steps_u = u32(max(1.0, gravity.params3.w));
    let escape_radius = gravity.params4.x;
    let adaptive_radius_scale = max(gravity.params4.y, 0.001);

    let ray_cam = normalize(vec3<f32>(
        ndc.x * aspect * tan_half_fov,
        -ndc.y * tan_half_fov,
        -1.0
    ));

    let right = camera.right.xyz;
    let up = camera.up.xyz;
    let forward = camera.forward.xyz;
    let ray_world = normalize(
        ray_cam.x * right +
        ray_cam.y * up +
        (-ray_cam.z) * forward
    );
    var sample_dir = ray_world;
    var steps_used: u32 = 0u;
    var absorbed = false;
    var invalid_state = false;
    var escaped = false;
    var side_b = false;
    var throat_cross_latch = false;

    if (integrator_enabled > 0.5) {
        var ray = RayState(cam_pos, ray_world);
        for (var i: u32 = 0u; i < max_steps_u; i = i + 1u) {
            if (!ray_state_is_finite(ray)) {
                invalid_state = true;
                break;
            }
            let r = length(ray.pos);
            if (r <= max(horizon_radius, 0.001)) {
                if (is_wormhole > 0.5) {
                    if (!throat_cross_latch) {
                        side_b = !side_b;
                        throat_cross_latch = true;
                        let n = normalize(select(vec3<f32>(0.0, 0.0, 1.0), ray.pos, r > 1.0e-6));
                        // Flip to the opposite side when crossing the throat.
                        ray.pos = -n * max(horizon_radius, 0.001) * 1.01;
                        ray.dir = normalize(ray.dir - 2.0 * dot(ray.dir, n) * n);
                    }
                } else {
                    absorbed = true;
                    break;
                }
            }
            if (r >= max(escape_radius, 1.0) && i > 2u) {
                escaped = true;
                break;
            }

            var h = max(base_step, 1.0e-5);
            if (adaptive_step > 0.5) {
                let adaptive = base_step * (r / adaptive_radius_scale);
                h = clamp(adaptive, min_step, max_step);
            }
            ray = rk4_step(ray, h, mass);
            steps_used = i + 1u;
            if (length(ray.pos) > max(horizon_radius, 0.001) * 1.2) {
                throat_cross_latch = false;
            }
        }
        sample_dir = normalize(ray.dir);
    }

    let sky_uv = world_dir_to_sky_uv(sample_dir);

    if (debug_direction_view > 0.5) {
        let base = 0.5 + 0.5 * ray_world;
        let gravity_debug = vec3<f32>(
            clamp(mass / 50.0, 0.0, 1.0),
            0.5 + 0.5 * clamp(spin, -1.0, 1.0),
            0.5 + 0.5 * clamp(charge / 5.0, -1.0, 1.0),
        );
        let grid_u = abs(fract(sky_uv.x * 24.0) - 0.5);
        let grid_v = abs(fract(sky_uv.y * 12.0) - 0.5);
        let grid = select(0.0, 1.0, min(grid_u, grid_v) < 0.02);
        let wormhole_boost = select(0.0, 0.2, is_wormhole > 0.5);
        let horizon_mix = clamp(horizon_radius / 20.0, 0.0, 1.0) * 0.25;
        let color = mix(base, gravity_debug, 0.35 + horizon_mix + wormhole_boost);
        let color_with_grid = mix(color, vec3<f32>(1.0), grid * 0.35);
        return vec4<f32>(color_with_grid, 1.0);
    }

    if (debug_steps_view > 0.5 && integrator_enabled > 0.5) {
        let t = clamp(f32(steps_used) / max(f32(max_steps_u), 1.0), 0.0, 1.0);
        var step_color = vec3<f32>(t, 0.25 + 0.75 * (1.0 - t), 1.0 - t);
        if (escaped) {
            step_color = mix(step_color, vec3<f32>(0.2, 1.0, 0.2), 0.35);
        }
        if (invalid_state) {
            step_color = vec3<f32>(1.0, 0.1, 1.0);
        }
        if (absorbed) {
            step_color = vec3<f32>(0.0, 0.0, 0.0);
        }
        if (is_wormhole > 0.5 && side_b) {
            step_color = mix(step_color, vec3<f32>(0.2, 0.45, 1.0), 0.35);
        }
        return vec4<f32>(step_color, 1.0);
    }

    if (absorbed) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    if (invalid_state) {
        return vec4<f32>(1.0, 0.0, 1.0, 1.0);
    }

    var hdr = textureSample(sky_tex, sky_sampler, sky_uv).rgb;
    if (is_wormhole > 0.5 && side_b) {
        hdr = textureSample(sky_tex_b, sky_sampler, sky_uv).rgb;
    }
    let exposed = hdr * max(exposure, 0.0001);
    let mapped = exposed / (vec3<f32>(1.0) + exposed);
    var corrected = pow(mapped, vec3<f32>(1.0 / max(gamma, 0.0001)));
    if (is_wormhole > 0.5 && side_b) {
        corrected = mix(corrected, corrected * vec3<f32>(0.88, 0.95, 1.08), 0.12);
    }
    return vec4<f32>(corrected, 1.0);
}
