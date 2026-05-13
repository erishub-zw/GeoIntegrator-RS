use crate::app::{App, GravityParams, IntegratorSettings};
use egui::{ProgressBar, Slider, Ui};

pub(super) enum HdrUiState {
    Loading { status: String, progress: f32 },
    Failed(String),
    Ready { status: String },
}

pub(super) fn draw_control_panel(ui: &mut Ui, app: &mut App, hdr_state: HdrUiState) {
    match hdr_state {
        HdrUiState::Loading { status, progress } => {
            ui.label(status);
            ui.add(ProgressBar::new(progress.clamp(0.0, 1.0)).show_percentage());
        }
        HdrUiState::Failed(err) => {
            ui.colored_label(egui::Color32::RED, format!("HDR load failed: {err}"));
        }
        HdrUiState::Ready { status } => {
            ui.colored_label(egui::Color32::GREEN, format!("HDR loaded ({status})"));
        }
    }

    ui.heading("Tone Mapping");
    ui.add(Slider::new(&mut app.tone_mapping.exposure, 0.05..=16.0).text("Exposure"));
    ui.add(Slider::new(&mut app.tone_mapping.gamma, 1.0..=3.0).text("Gamma"));
    ui.checkbox(
        &mut app.tone_mapping.debug_direction_view,
        "Direction Debug View",
    );

    ui.separator();
    ui.heading("Gravity Params (M2)");
    ui.add(
        Slider::new(
            &mut app.gravity.mass,
            GravityParams::MASS_MIN..=GravityParams::MASS_MAX,
        )
        .text("Mass"),
    );
    ui.add(
        Slider::new(
            &mut app.gravity.spin,
            GravityParams::SPIN_MIN..=GravityParams::SPIN_MAX,
        )
        .text("Spin"),
    );
    ui.add(
        Slider::new(
            &mut app.gravity.charge,
            GravityParams::CHARGE_MIN..=GravityParams::CHARGE_MAX,
        )
        .text("Charge"),
    );
    ui.checkbox(&mut app.gravity.is_wormhole, "Wormhole Mode");
    ui.add(
        Slider::new(
            &mut app.gravity.horizon_radius,
            GravityParams::HORIZON_RADIUS_MIN..=GravityParams::HORIZON_RADIUS_MAX,
        )
        .text(if app.gravity.is_wormhole {
            "Throat Radius"
        } else {
            "Horizon Radius"
        }),
    );
    if app.gravity.is_wormhole {
        ui.small("Mode hint: wormhole enabled. Secondary sky texture will be used for the wormhole side.");
    } else {
        ui.small("Mode hint: black-hole mode. Primary sky texture is used.");
    }
    app.gravity.sanitize();

    ui.separator();
    ui.heading("Integrator (M3)");
    ui.checkbox(&mut app.integrator.enabled, "Enable Geodesic Integrator");
    ui.checkbox(&mut app.integrator.debug_steps_view, "Debug Steps View");
    ui.checkbox(&mut app.integrator.adaptive_step, "Adaptive Step");
    ui.add(
        Slider::new(
            &mut app.integrator.step_size,
            IntegratorSettings::STEP_SIZE_MIN..=IntegratorSettings::STEP_SIZE_MAX,
        )
        .text("Base Step"),
    );
    ui.add(
        Slider::new(
            &mut app.integrator.min_step_size,
            IntegratorSettings::STEP_SIZE_MIN..=IntegratorSettings::STEP_SIZE_MAX,
        )
        .text("Min Step"),
    );
    ui.add(
        Slider::new(
            &mut app.integrator.max_step_size,
            IntegratorSettings::STEP_SIZE_MIN..=IntegratorSettings::STEP_SIZE_MAX,
        )
        .text("Max Step"),
    );
    ui.add(
        Slider::new(
            &mut app.integrator.max_steps,
            IntegratorSettings::MAX_STEPS_MIN..=IntegratorSettings::MAX_STEPS_MAX,
        )
        .text("Max Steps"),
    );
    ui.add(
        Slider::new(
            &mut app.integrator.escape_radius,
            IntegratorSettings::ESCAPE_RADIUS_MIN..=IntegratorSettings::ESCAPE_RADIUS_MAX,
        )
        .text("Escape Radius"),
    );
    ui.add(
        Slider::new(
            &mut app.integrator.adaptive_radius_scale,
            IntegratorSettings::ADAPTIVE_RADIUS_SCALE_MIN..=IntegratorSettings::ADAPTIVE_RADIUS_SCALE_MAX,
        )
        .text("Adaptive Radius Scale"),
    );
    app.integrator.sanitize();
}
