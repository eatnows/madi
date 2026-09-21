//! The text model behind the editor: no UI, so it's tested (and reused) on its own.
mod buffer;
mod document;

pub use buffer::{Buffer, Pos};
pub use document::{find_in_line, Document};
