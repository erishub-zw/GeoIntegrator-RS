use egone::app::winit_runner;

fn main() {
    if let Err(err) = winit_runner::run() {
        eprintln!("application error: {err}");
    }
}
