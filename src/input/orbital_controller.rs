use crate::camera::OrbitalCamera;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MousePosition {
    pub x: f32,
    pub y: f32,
}

impl MousePosition {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone)]
pub struct OrbitalCameraController {
    pub orbit_sensitivity: f32,
    pub zoom_sensitivity: f32,
    pub invert_y: bool,
    is_dragging: bool,
    last_mouse_pos: Option<MousePosition>,
}

impl Default for OrbitalCameraController {
    fn default() -> Self {
        Self {
            orbit_sensitivity: 0.01,
            zoom_sensitivity: 0.25,
            invert_y: false,
            is_dragging: false,
            last_mouse_pos: None,
        }
    }
}

impl OrbitalCameraController {
    pub fn begin_orbit_drag(&mut self, pos: MousePosition) {
        self.is_dragging = true;
        self.last_mouse_pos = Some(pos);
    }

    pub fn update_orbit_drag(&mut self, camera: &mut OrbitalCamera, pos: MousePosition) {
        if !self.is_dragging {
            self.last_mouse_pos = Some(pos);
            return;
        }

        let Some(last) = self.last_mouse_pos else {
            self.last_mouse_pos = Some(pos);
            return;
        };

        let delta_x = pos.x - last.x;
        let delta_y = pos.y - last.y;
        let dy_sign = if self.invert_y { 1.0 } else { -1.0 };

        camera.rotate(
            delta_x * self.orbit_sensitivity,
            delta_y * self.orbit_sensitivity * dy_sign,
        );
        self.last_mouse_pos = Some(pos);
    }

    pub fn end_orbit_drag(&mut self) {
        self.is_dragging = false;
        self.last_mouse_pos = None;
    }

    pub fn apply_scroll(&self, camera: &mut OrbitalCamera, scroll_delta: f32) {
        camera.zoom(-scroll_delta * self.zoom_sensitivity);
    }
}
