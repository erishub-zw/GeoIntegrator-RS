use crate::camera::OrbitalCamera;
use crate::input::{MousePosition, OrbitalCameraController};

pub mod winit_runner;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppEvent {
    MouseLeftDown,
    MouseLeftUp,
    CursorMoved { x: f32, y: f32 },
    MouseWheel { delta: f32 },
    Resized { width: u32, height: u32 },
}

#[derive(Debug)]
pub struct App {
    pub camera: OrbitalCamera,
    pub controller: OrbitalCameraController,
    pub tone_mapping: ToneMappingSettings,
    pub gravity: GravityParams,
    pub integrator: IntegratorSettings,
    cursor: MousePosition,
}

#[derive(Debug, Clone, Copy)]
pub struct ToneMappingSettings {
    pub exposure: f32,
    pub gamma: f32,
    pub debug_direction_view: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct GravityParams {
    pub mass: f32,
    pub spin: f32,
    pub charge: f32,
    pub is_wormhole: bool,
    pub horizon_radius: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct IntegratorSettings {
    pub enabled: bool,
    pub debug_steps_view: bool,
    pub adaptive_step: bool,
    pub step_size: f32,
    pub min_step_size: f32,
    pub max_step_size: f32,
    pub adaptive_radius_scale: f32,
    pub max_steps: u32,
    pub escape_radius: f32,
}

impl GravityParams {
    pub const MASS_MIN: f32 = 0.0;
    pub const MASS_MAX: f32 = 50.0;
    pub const SPIN_MIN: f32 = -1.0;
    pub const SPIN_MAX: f32 = 1.0;
    pub const CHARGE_MIN: f32 = -5.0;
    pub const CHARGE_MAX: f32 = 5.0;
    pub const HORIZON_RADIUS_MIN: f32 = 0.1;
    pub const HORIZON_RADIUS_MAX: f32 = 20.0;

    pub fn sanitize(&mut self) {
        self.mass = sanitize_range(self.mass, Self::MASS_MIN, Self::MASS_MAX, 5.0);
        self.spin = sanitize_range(self.spin, Self::SPIN_MIN, Self::SPIN_MAX, 0.0);
        self.charge = sanitize_range(self.charge, Self::CHARGE_MIN, Self::CHARGE_MAX, 0.0);
        self.horizon_radius = sanitize_range(
            self.horizon_radius,
            Self::HORIZON_RADIUS_MIN,
            Self::HORIZON_RADIUS_MAX,
            1.0,
        );
    }
}

impl IntegratorSettings {
    pub const STEP_SIZE_MIN: f32 = 0.0005;
    pub const STEP_SIZE_MAX: f32 = 0.2;
    pub const MAX_STEPS_MIN: u32 = 8;
    pub const MAX_STEPS_MAX: u32 = 1024;
    pub const ESCAPE_RADIUS_MIN: f32 = 2.0;
    pub const ESCAPE_RADIUS_MAX: f32 = 200.0;
    pub const ADAPTIVE_RADIUS_SCALE_MIN: f32 = 0.25;
    pub const ADAPTIVE_RADIUS_SCALE_MAX: f32 = 100.0;

    pub fn sanitize(&mut self) {
        self.step_size = sanitize_range(self.step_size, Self::STEP_SIZE_MIN, Self::STEP_SIZE_MAX, 0.02);
        self.min_step_size = sanitize_range(
            self.min_step_size,
            Self::STEP_SIZE_MIN,
            Self::STEP_SIZE_MAX,
            0.002,
        );
        self.max_step_size = sanitize_range(
            self.max_step_size,
            Self::STEP_SIZE_MIN,
            Self::STEP_SIZE_MAX,
            0.05,
        );
        if self.min_step_size > self.max_step_size {
            core::mem::swap(&mut self.min_step_size, &mut self.max_step_size);
        }
        self.adaptive_radius_scale = sanitize_range(
            self.adaptive_radius_scale,
            Self::ADAPTIVE_RADIUS_SCALE_MIN,
            Self::ADAPTIVE_RADIUS_SCALE_MAX,
            6.0,
        );
        self.max_steps = self.max_steps.clamp(Self::MAX_STEPS_MIN, Self::MAX_STEPS_MAX);
        self.escape_radius = sanitize_range(
            self.escape_radius,
            Self::ESCAPE_RADIUS_MIN,
            Self::ESCAPE_RADIUS_MAX,
            40.0,
        );
    }
}

impl Default for App {
    fn default() -> Self {
        Self {
            camera: OrbitalCamera::default(),
            controller: OrbitalCameraController::default(),
            tone_mapping: ToneMappingSettings {
                exposure: 1.0,
                gamma: 2.2,
                debug_direction_view: false,
            },
            gravity: GravityParams {
                mass: 5.0,
                spin: 0.0,
                charge: 0.0,
                is_wormhole: false,
                horizon_radius: 1.0,
            },
            integrator: IntegratorSettings {
                enabled: false,
                debug_steps_view: false,
                adaptive_step: false,
                step_size: 0.02,
                min_step_size: 0.002,
                max_step_size: 0.05,
                adaptive_radius_scale: 6.0,
                max_steps: 128,
                escape_radius: 40.0,
            },
            cursor: MousePosition::new(0.0, 0.0),
        }
    }
}

impl App {
    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::MouseLeftDown => {
                self.controller.begin_orbit_drag(self.cursor);
            }
            AppEvent::MouseLeftUp => {
                self.controller.end_orbit_drag();
            }
            AppEvent::CursorMoved { x, y } => {
                self.cursor = MousePosition::new(x, y);
                self.controller.update_orbit_drag(&mut self.camera, self.cursor);
            }
            AppEvent::MouseWheel { delta } => {
                self.controller.apply_scroll(&mut self.camera, delta);
            }
            AppEvent::Resized { width, height } => {
                if width > 0 && height > 0 {
                    self.camera.set_aspect(width as f32 / height as f32);
                }
            }
        }
        self.gravity.sanitize();
        self.integrator.sanitize();
    }
}

fn sanitize_range(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}
