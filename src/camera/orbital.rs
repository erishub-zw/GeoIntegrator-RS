use core::ops::{Add, AddAssign, Sub};

const DEFAULT_RADIUS_MIN: f32 = 0.5;
const DEFAULT_RADIUS_MAX: f32 = 10_000.0;
const DEFAULT_ELEVATION_MIN: f32 = -1.55;
const DEFAULT_ELEVATION_MAX: f32 = 1.55;

pub type Mat4 = [[f32; 4]; 4];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }

    pub fn cross(self, rhs: Self) -> Self {
        Self::new(
            self.y * rhs.z - self.z * rhs.y,
            self.z * rhs.x - self.x * rhs.z,
            self.x * rhs.y - self.y * rhs.x,
        )
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn normalize(self) -> Self {
        let len = self.length();
        if len <= f32::EPSILON {
            Self::ZERO
        } else {
            Self::new(self.x / len, self.y / len, self.z / len)
        }
    }
}

impl Add for Vec3 {
    type Output = Vec3;

    fn add(self, rhs: Self) -> Self::Output {
        Vec3::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl Sub for Vec3 {
    type Output = Vec3;

    fn sub(self, rhs: Self) -> Self::Output {
        Vec3::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

#[derive(Debug, Clone)]
pub struct OrbitalCamera {
    pub target: Vec3,
    pub radius: f32,
    pub azimuth: f32,
    pub elevation: f32,
    pub radius_min: f32,
    pub radius_max: f32,
    pub elevation_min: f32,
    pub elevation_max: f32,
    pub fov_y_radians: f32,
    pub aspect: f32,
    pub z_near: f32,
    pub z_far: f32,
}

impl Default for OrbitalCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            radius: 5.0,
            azimuth: 0.0,
            elevation: 0.35,
            radius_min: DEFAULT_RADIUS_MIN,
            radius_max: DEFAULT_RADIUS_MAX,
            elevation_min: DEFAULT_ELEVATION_MIN,
            elevation_max: DEFAULT_ELEVATION_MAX,
            fov_y_radians: 60.0_f32.to_radians(),
            aspect: 16.0 / 9.0,
            z_near: 0.01,
            z_far: 100_000.0,
        }
    }
}

impl OrbitalCamera {
    pub fn new(target: Vec3, radius: f32, azimuth: f32, elevation: f32) -> Self {
        let mut camera = Self {
            target,
            radius,
            azimuth,
            elevation,
            ..Self::default()
        };
        camera.clamp_all();
        camera
    }

    pub fn position(&self) -> Vec3 {
        let cos_el = self.elevation.cos();
        let x = self.radius * cos_el * self.azimuth.cos();
        let y = self.radius * self.elevation.sin();
        let z = self.radius * cos_el * self.azimuth.sin();
        self.target + Vec3::new(x, y, z)
    }

    pub fn view_matrix(&self) -> Mat4 {
        look_at_rh(self.position(), self.target, Vec3::Y)
    }

    pub fn projection_matrix(&self) -> Mat4 {
        perspective_rh(self.fov_y_radians, self.aspect, self.z_near, self.z_far)
    }

    pub fn view_projection_matrix(&self) -> Mat4 {
        mul_mat4(self.projection_matrix(), self.view_matrix())
    }

    pub fn forward(&self) -> Vec3 {
        (self.target - self.position()).normalize()
    }

    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize()
    }

    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward()).normalize()
    }

    pub fn rotate(&mut self, delta_azimuth: f32, delta_elevation: f32) {
        self.azimuth += delta_azimuth;
        self.elevation += delta_elevation;
        self.clamp_elevation();
    }

    pub fn zoom(&mut self, delta_radius: f32) {
        self.radius += delta_radius;
        self.clamp_radius();
    }

    pub fn set_aspect(&mut self, aspect: f32) {
        if aspect.is_finite() && aspect > 0.0 {
            self.aspect = aspect;
        }
    }

    fn clamp_all(&mut self) {
        self.clamp_radius();
        self.clamp_elevation();
    }

    fn clamp_radius(&mut self) {
        if self.radius_min > self.radius_max {
            core::mem::swap(&mut self.radius_min, &mut self.radius_max);
        }
        self.radius = self.radius.clamp(self.radius_min, self.radius_max);
    }

    fn clamp_elevation(&mut self) {
        if self.elevation_min > self.elevation_max {
            core::mem::swap(&mut self.elevation_min, &mut self.elevation_max);
        }
        self.elevation = self.elevation.clamp(self.elevation_min, self.elevation_max);
    }
}

fn look_at_rh(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
    let f = (center - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);

    [
        [s.x, u.x, -f.x, 0.0],
        [s.y, u.y, -f.y, 0.0],
        [s.z, u.z, -f.z, 0.0],
        [-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0],
    ]
}

fn perspective_rh(fov_y_radians: f32, aspect: f32, z_near: f32, z_far: f32) -> Mat4 {
    let f = 1.0 / (fov_y_radians * 0.5).tan();
    let nf = 1.0 / (z_near - z_far);
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, (z_far + z_near) * nf, -1.0],
        [0.0, 0.0, 2.0 * z_far * z_near * nf, 0.0],
    ]
}

fn mul_mat4(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    let mut r = 0;
    while r < 4 {
        let mut c = 0;
        while c < 4 {
            out[r][c] = a[r][0] * b[0][c]
                + a[r][1] * b[1][c]
                + a[r][2] * b[2][c]
                + a[r][3] * b[3][c];
            c += 1;
        }
        r += 1;
    }
    out
}
