mod app;
mod config;
mod diff_view;
mod graph;
mod picker;
mod text_input;
mod theme;

use gpui::{prelude::*, px, size, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions};

fn main() {
    // Optional first argument: a repo to open (added to the saved project list).
    let initial = std::env::args().nth(1);

    Application::new().run(move |cx: &mut App| {
        app::bind_keys(cx);
        text_input::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Maditor".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| app::Maditor::new(initial, config::Config::load(), cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}
