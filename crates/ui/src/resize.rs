//! A thin drag handle for resizing a panel. The owner stores the drag state and applies it as the
//! mouse moves (it needs to hear moves anywhere in the window, not just over the handle).
use gpui::{div, prelude::*, px, App, CursorStyle, Div, MouseButton, MouseDownEvent, Stateful, Window};

use crate::theme::*;

/// `horizontal`: the handle is a vertical bar resizing a panel's width (dragged left/right).
pub fn handle(
    id: &'static str,
    horizontal: bool,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let base = div().id(id).flex_none().hover(|d| d.bg(BORDER())).on_mouse_down(MouseButton::Left, on_press);
    if horizontal {
        base.w(px(4.)).h_full().cursor(CursorStyle::ResizeLeftRight)
    } else {
        base.h(px(4.)).w_full().cursor(CursorStyle::ResizeUpDown)
    }
}
