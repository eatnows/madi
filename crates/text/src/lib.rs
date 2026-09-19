//! The text model behind the editor: no UI, so it's tested (and reused) on its own.
mod buffer;

pub use buffer::{Buffer, Pos};
