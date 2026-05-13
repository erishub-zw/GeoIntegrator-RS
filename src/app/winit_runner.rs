mod gpu;
mod hdr_loader;
mod ui;

use crate::app::{App, AppEvent};
use egui::{Align2, Color32, TexturesDelta, ViewportId};
use egui_winit::State as EguiWinitState;
use pollster::block_on;
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessesToUpdate, System, get_current_pid};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use self::gpu::GpuState;
use self::hdr_loader::{spawn_hdr_loader, HdrLoadMessage};
use self::ui::{draw_control_panel, HdrUiState};

struct WinitApp {
    app: App,
    egui_ctx: egui::Context,
    egui_state: Option<EguiWinitState>,
    window: Option<Arc<Window>>,
    window_id: Option<WindowId>,
    gpu: Option<GpuState>,
    hdr_load_rx_primary: Option<Receiver<HdrLoadMessage>>,
    hdr_load_rx_secondary: Option<Receiver<HdrLoadMessage>>,
    hdr_load_error_primary: Option<String>,
    hdr_load_error_secondary: Option<String>,
    hdr_load_progress_primary: f32,
    hdr_load_progress_secondary: f32,
    hdr_load_status_primary: String,
    hdr_load_status_secondary: String,
    is_occluded: bool,
    is_active: bool,
    last_frame_at: Instant,
    frame_interval: Duration,
    frame_time_ms_smoothed: f32,
    fps_smoothed: f32,
    cpu_usage_percent: f32,
    gpu_usage_estimated_percent: f32,
    stats_system: System,
    current_pid: Option<Pid>,
    last_stats_refresh: Instant,
}

impl Default for WinitApp {
    fn default() -> Self {
        Self {
            app: App::default(),
            egui_ctx: egui::Context::default(),
            egui_state: None,
            window: None,
            window_id: None,
            gpu: None,
            hdr_load_rx_primary: None,
            hdr_load_rx_secondary: None,
            hdr_load_error_primary: None,
            hdr_load_error_secondary: None,
            hdr_load_progress_primary: 0.0,
            hdr_load_progress_secondary: 0.0,
            hdr_load_status_primary: "Waiting for HDR A load".to_string(),
            hdr_load_status_secondary: "Waiting for HDR B load".to_string(),
            is_occluded: false,
            is_active: true,
            last_frame_at: Instant::now(),
            frame_interval: Duration::from_millis(16),
            frame_time_ms_smoothed: 16.0,
            fps_smoothed: 60.0,
            cpu_usage_percent: 0.0,
            gpu_usage_estimated_percent: 0.0,
            stats_system: System::new_all(),
            current_pid: get_current_pid().ok(),
            last_stats_refresh: Instant::now(),
        }
    }
}

impl ApplicationHandler for WinitApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.is_active = true;

        if self.window.is_some() {
            return;
        }

        let attrs: WindowAttributes = Window::default_attributes()
            .with_title("Egone - Orbital Camera (Step 3)")
            .with_inner_size(LogicalSize::new(1280.0, 720.0));

        match event_loop.create_window(attrs) {
            Ok(window) => {
                let window = Arc::new(window);
                let size = window.inner_size();
                self.app.handle_event(AppEvent::Resized {
                    width: size.width,
                    height: size.height,
                });
                match block_on(GpuState::new(window.clone())) {
                    Ok(gpu) => {
                        let max_dim = gpu.max_texture_dimension_2d;
                        self.gpu = Some(gpu);
                        self.hdr_load_error_primary = None;
                        self.hdr_load_error_secondary = None;
                        self.hdr_load_progress_primary = 0.0;
                        self.hdr_load_progress_secondary = 0.0;
                        self.hdr_load_status_primary = "Starting HDR A loader...".to_string();
                        self.hdr_load_status_secondary = "Starting HDR B loader...".to_string();
                        let hdr_path_primary =
                            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assets/sky/bk.hdr");
                        let hdr_path_secondary =
                            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assets/sky/out2.hdr");
                        self.hdr_load_rx_primary = Some(spawn_hdr_loader(hdr_path_primary, max_dim));
                        self.hdr_load_rx_secondary = Some(spawn_hdr_loader(hdr_path_secondary, max_dim));
                    }
                    Err(err) => {
                        eprintln!("failed to init wgpu: {err}");
                        event_loop.exit();
                        return;
                    }
                }
                self.window_id = Some(window.id());
                let mut egui_state = EguiWinitState::new(
                    self.egui_ctx.clone(),
                    ViewportId::ROOT,
                    window.as_ref(),
                    Some(window.scale_factor() as f32),
                    window.theme(),
                    None,
                );
                egui_state.egui_input_mut().focused = true;
                self.egui_state = Some(egui_state);
                self.window = Some(window);
            }
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
            }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.is_active = false;
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if Some(window_id) != self.window_id {
            return;
        }
        let egui_consumed = if let (Some(window), Some(egui_state)) =
            (self.window.as_ref(), self.egui_state.as_mut())
        {
            egui_state.on_window_event(window, &event).consumed
        } else {
            false
        };

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.app.handle_event(AppEvent::Resized {
                    width: size.width,
                    height: size.height,
                });
                if let Some(gpu) = self.gpu.as_mut() {
                    if size.width > 0 && size.height > 0 {
                        gpu.resize(size.width, size.height);
                    }
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.is_occluded = occluded;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if !egui_consumed {
                    self.app.handle_event(AppEvent::CursorMoved {
                        x: position.x as f32,
                        y: position.y as f32,
                    });
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if !egui_consumed {
                    if state == ElementState::Pressed {
                        self.app.handle_event(AppEvent::MouseLeftDown);
                    } else {
                        self.app.handle_event(AppEvent::MouseLeftUp);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if !egui_consumed {
                    self.app.handle_event(AppEvent::MouseWheel {
                        delta: normalize_wheel_delta(delta),
                    });
                }
            }
            WindowEvent::RedrawRequested => {
                self.poll_hdr_loading();
                if !self.can_render() {
                    return;
                }
                self.update_perf_stats();
                if let Some(window) = self.window.as_ref() {
                    let pos = self.app.camera.position();
                    window.set_title(&format!(
                        "Egone r={:.2} az={:.2} el={:.2} exp={:.2} gam={:.2} pos=({:.2},{:.2},{:.2})",
                        self.app.camera.radius,
                        self.app.camera.azimuth,
                        self.app.camera.elevation,
                        self.app.tone_mapping.exposure,
                        self.app.tone_mapping.gamma,
                        pos.x,
                        pos.y,
                        pos.z
                    ));
                }

                let (paint_jobs, textures_delta, pixels_per_point) = if let (Some(window), Some(egui_state)) =
                    (self.window.as_ref(), self.egui_state.as_mut())
                {
                    let raw_input = egui_state.take_egui_input(window);
                    let full_output = self.egui_ctx.run_ui(raw_input, |ui| {
                        egui::Area::new("control_panel_left_top".into())
                            .anchor(Align2::LEFT_TOP, egui::vec2(12.0, 12.0))
                            .show(ui.ctx(), |ui| {
                                egui::Frame::window(ui.style())
                                    .fill(Color32::from_rgba_unmultiplied(18, 22, 30, 150))
                                    .stroke(egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(180, 190, 210, 90)))
                                    .show(ui, |ui| {
                                    ui.set_width(300.0);
                                    ui.set_max_width(300.0);
                                    ui.spacing_mut().slider_width = 160.0;
                                    let is_loading = self.hdr_load_rx_primary.is_some() || self.hdr_load_rx_secondary.is_some();
                                    let hdr_ui_state = if is_loading {
                                        HdrUiState::Loading {
                                            status: format!(
                                            "HDR A: {} | HDR B: {}",
                                            self.hdr_load_status_primary, self.hdr_load_status_secondary
                                        ),
                                            progress: (self.hdr_load_progress_primary + self.hdr_load_progress_secondary) * 0.5,
                                        }
                                    } else if let Some(err) = &self.hdr_load_error_primary {
                                        HdrUiState::Failed(err.clone())
                                    } else if let Some(err) = &self.hdr_load_error_secondary {
                                        HdrUiState::Failed(err.clone())
                                    } else {
                                        HdrUiState::Ready {
                                            status: format!(
                                            "A: {} | B: {}",
                                            self.hdr_load_status_primary, self.hdr_load_status_secondary
                                        ),
                                        }
                                    };
                                    draw_control_panel(ui, &mut self.app, hdr_ui_state);
                                });
                            });
                        egui::Area::new("perf_panel_right_top".into())
                            .anchor(Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
                            .show(ui.ctx(), |ui| {
                                egui::Frame::window(ui.style())
                                    .fill(Color32::from_rgba_unmultiplied(14, 18, 26, 130))
                                    .stroke(egui::Stroke::new(
                                        1.0,
                                        Color32::from_rgba_unmultiplied(180, 190, 210, 80),
                                    ))
                                    .show(ui, |ui| {
                                        ui.label(format!("FPS: {:.1}", self.fps_smoothed));
                                        ui.label(format!("CPU: {:.1}%", self.cpu_usage_percent));
                                        ui.label(format!(
                                            "GPU(est): {:.1}%",
                                            self.gpu_usage_estimated_percent
                                        ));
                                    });
                            });
                    });
                    egui_state.handle_platform_output(window, full_output.platform_output);
                    let pixels_per_point = self.egui_ctx.pixels_per_point();
                    let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, pixels_per_point);
                    (paint_jobs, full_output.textures_delta, pixels_per_point)
                } else {
                    (Vec::new(), TexturesDelta::default(), 1.0)
                };

                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.render(&self.app, &paint_jobs, &textures_delta, pixels_per_point);
                }
                self.last_frame_at = Instant::now();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.poll_hdr_loading();
        if self.can_render() {
            let now = Instant::now();
            let next_frame = self.last_frame_at + self.frame_interval;
            if now >= next_frame {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(now + self.frame_interval));
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(next_frame));
            }
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

impl WinitApp {
    fn update_perf_stats(&mut self) {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_frame_at);
        let dt_ms = dt.as_secs_f32() * 1000.0;
        if dt_ms > 0.0 {
            self.frame_time_ms_smoothed = 0.9 * self.frame_time_ms_smoothed + 0.1 * dt_ms;
            self.fps_smoothed = 1000.0 / self.frame_time_ms_smoothed.max(0.0001);
        }
        let frame_budget_ms = self.frame_interval.as_secs_f32() * 1000.0;
        self.gpu_usage_estimated_percent = ((self.frame_time_ms_smoothed / frame_budget_ms) * 100.0).clamp(0.0, 100.0);

        if now.duration_since(self.last_stats_refresh) >= Duration::from_millis(500) {
            if let Some(pid) = self.current_pid {
                self.stats_system
                    .refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                if let Some(process) = self.stats_system.process(pid) {
                    self.cpu_usage_percent = process.cpu_usage();
                }
            }
            self.last_stats_refresh = now;
        }
    }

    fn poll_hdr_loading(&mut self) {
        if let Some(rx) = self.hdr_load_rx_primary.take() {
            let mut keep_receiver = true;
            loop {
                match rx.try_recv() {
                    Ok(HdrLoadMessage::Progress { value, status }) => {
                        self.hdr_load_progress_primary = value.clamp(0.0, 1.0);
                        self.hdr_load_status_primary = status;
                    }
                    Ok(HdrLoadMessage::Done(data)) => {
                        if let Some(gpu) = self.gpu.as_mut() {
                            gpu.update_sky_texture_a(data);
                        }
                        self.hdr_load_progress_primary = 1.0;
                        self.hdr_load_status_primary = "Uploaded A to GPU".to_string();
                        self.hdr_load_error_primary = None;
                        keep_receiver = false;
                        break;
                    }
                    Ok(HdrLoadMessage::Failed(err)) => {
                        self.hdr_load_error_primary = Some(err);
                        keep_receiver = false;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.hdr_load_error_primary = Some("background loader A disconnected".to_string());
                        keep_receiver = false;
                        break;
                    }
                }
            }
            if keep_receiver {
                self.hdr_load_rx_primary = Some(rx);
            }
        }

        if let Some(rx) = self.hdr_load_rx_secondary.take() {
            let mut keep_receiver = true;
            loop {
                match rx.try_recv() {
                    Ok(HdrLoadMessage::Progress { value, status }) => {
                        self.hdr_load_progress_secondary = value.clamp(0.0, 1.0);
                        self.hdr_load_status_secondary = status;
                    }
                    Ok(HdrLoadMessage::Done(data)) => {
                        if let Some(gpu) = self.gpu.as_mut() {
                            gpu.update_sky_texture_b(data);
                        }
                        self.hdr_load_progress_secondary = 1.0;
                        self.hdr_load_status_secondary = "Uploaded B to GPU".to_string();
                        self.hdr_load_error_secondary = None;
                        keep_receiver = false;
                        break;
                    }
                    Ok(HdrLoadMessage::Failed(err)) => {
                        self.hdr_load_error_secondary = Some(err);
                        keep_receiver = false;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.hdr_load_error_secondary = Some("background loader B disconnected".to_string());
                        keep_receiver = false;
                        break;
                    }
                }
            }
            if keep_receiver {
                self.hdr_load_rx_secondary = Some(rx);
            }
        }
    }

    fn can_render(&self) -> bool {
        self.is_active && !self.is_occluded
    }
}

pub fn run() -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let mut winit_app = WinitApp::default();
    event_loop
        .run_app(&mut winit_app)
        .map_err(|e| e.to_string())
}

fn normalize_wheel_delta(delta: MouseScrollDelta) -> f32 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => y,
        MouseScrollDelta::PixelDelta(PhysicalPosition { y, .. }) => (y as f32) / 40.0,
    }
}

#[cfg(test)]
fn world_dir_to_sky_uv(dir: [f32; 3]) -> (f32, f32) {
    let x = dir[0];
    let y = dir[1].clamp(-1.0, 1.0);
    let z = dir[2];
    let pi = core::f32::consts::PI;
    let phi = z.atan2(x);
    let theta = y.acos();
    let u = phi / (2.0 * pi) + 0.5;
    let v = theta / pi;
    (u, v)
}

#[cfg(test)]
mod tests {
    use super::{normalize_wheel_delta, world_dir_to_sky_uv};
    use winit::dpi::PhysicalPosition;
    use winit::event::MouseScrollDelta;

    fn assert_close(actual: f32, expected: f32) {
        let diff = (actual - expected).abs();
        assert!(diff < 1e-4, "actual={actual}, expected={expected}, diff={diff}");
    }

    #[test]
    fn normalize_line_wheel_delta() {
        assert_close(normalize_wheel_delta(MouseScrollDelta::LineDelta(0.0, 2.0)), 2.0);
    }

    #[test]
    fn normalize_pixel_wheel_delta() {
        let delta = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 120.0));
        assert_close(normalize_wheel_delta(delta), 3.0);
    }

    #[test]
    fn world_direction_to_uv_mapping_is_consistent() {
        let (u_px, v_px) = world_dir_to_sky_uv([1.0, 0.0, 0.0]);
        assert_close(u_px, 0.5);
        assert_close(v_px, 0.5);

        let (u_pz, v_pz) = world_dir_to_sky_uv([0.0, 0.0, 1.0]);
        assert_close(u_pz, 0.75);
        assert_close(v_pz, 0.5);

        let (u_nz, v_nz) = world_dir_to_sky_uv([0.0, 0.0, -1.0]);
        assert_close(u_nz, 0.25);
        assert_close(v_nz, 0.5);

        let (_u_py, v_py) = world_dir_to_sky_uv([0.0, 1.0, 0.0]);
        assert_close(v_py, 0.0);

        let (_u_ny, v_ny) = world_dir_to_sky_uv([0.0, -1.0, 0.0]);
        assert_close(v_ny, 1.0);
    }

}
