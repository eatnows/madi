mod app;
mod diff_view;
mod editor;
mod scroll;
mod text_input;
mod theme;

use gpui::{prelude::*, px, size, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions};

fn main() {
    // Optional first argument: a repo to open (added to the saved project list).
    let initial = std::env::args().nth(1);

    Application::new().run(move |cx: &mut App| {
        app::bind_keys(cx);
        text_input::bind_keys(cx);
        editor::view::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1360.), px(820.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Maditor".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| app::Maditor::new(initial, maditor_project::config::Config::load(), cx));
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
