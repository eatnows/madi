mod app;

use gpui::{prelude::*, px, size, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions};

fn main() {
    // Optional first argument: a repo folder (added to the saved project list) or a single file.
    let initial = std::env::args()
        .nth(1)
        .map(|arg| std::path::absolute(&arg).map(|p| p.to_string_lossy().into_owned()).unwrap_or(arg));

    Application::new().run(move |cx: &mut App| {
        app::bind_keys(cx);
        madi_ui::text_input::bind_keys(cx);
        madi_editor::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1360.), px(820.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Madi".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| app::Madi::new(initial, madi_project::config::Config::load(), window, cx));
                // Repaint when the OS switches between light and dark (used by the "Auto" setting).
                let observed = view.clone();
                window
                    .observe_window_appearance(move |_, cx| observed.update(cx, |_, cx| cx.notify()))
                    .detach();
                view
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
