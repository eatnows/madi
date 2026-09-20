//! Git logic shared by every part of Madi: worktrees, diffs, history, and the shapes derived
//! from them (graph lanes, diff layouts, branch lists). No UI dependency.
pub mod branches;
pub mod diff;
pub mod diff_layout;
pub mod git_log;
pub mod graph;
pub mod time;
pub mod worktree;
