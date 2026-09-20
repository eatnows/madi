//! The code editor view: a `madi_text::Document` with IME-aware input, keyboard actions, mouse
//! selection and a virtualized, scrollable rendering.
mod editor;

pub use editor::{bind_keys, Editor, EditorEvent};
