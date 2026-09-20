mod app;
mod icons;
mod theme;

use gyeol::WindowOptions;
use madi_project::config::Config;

fn main() {
    // Optional first argument: a project folder to open (added to the saved project list).
    let initial = std::env::args().nth(1).map(|arg| std::path::absolute(&arg).map(|p| p.to_string_lossy().into_owned()).unwrap_or(arg));
    let options = WindowOptions::new("Madi").size(1360., 820.).min_size(720., 480.);
    if let Err(e) = gyeol::run_view_with(options, app::Madi::new(initial, Config::load())) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
