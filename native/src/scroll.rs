//! Small scroll helpers shared across views.
use gpui::{Point, ScrollHandle, Styled, UniformListScrollHandle};

/// gpui reinterprets a wheel's *other* axis as movement along an element's own axis (vertical
/// input scrolls a horizontal-only container, horizontal input a vertical-only one), and nested
/// scrollers all receive every event. Left alone, a horizontal swipe over a list also drifted it
/// vertically. Locking each scroller to its axis keeps the two directions independent.
pub fn axis_locked<T: Styled>(mut el: T) -> T {
    el.style().restrict_scroll_to_axis = Some(true);
    el
}

pub fn scroll_to_top(handle: &ScrollHandle) {
    handle.set_offset(Point::default());
}

pub fn list_scroll_to_top(handle: &UniformListScrollHandle) {
    handle.0.borrow().base_handle.set_offset(Point::default());
}
