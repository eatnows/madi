# Maditor

A native (Rust + [GPUI](https://www.gpui.rs)) code editor built around git worktrees: edit files,
and review what each worktree changed against its base branch without leaving the editor.

- **Files** – the project's tree; files open as tabs in the editor (Korean/IME input works).
- **Worktrees** – every git worktree with ahead/behind against a pinned base branch; a changed
  file opens as a diff tab (side-by-side, or unified when narrow).
- **Graph** – a commit graph for any branch, with the commit's files and diff.
- Light and dark themes (follows the OS, or pick one in the top bar).

## Layout

A Cargo workspace, split by responsibility (the crate boundaries mirror what Zed does: models with
no UI dependency, reusable UI pieces, views, and the app that assembles them):

| crate | what it holds | gpui? |
| --- | --- | --- |
| `crates/text` | text buffer with undo, and `Document`: caret, selection, editing commands, IME offsets | no |
| `crates/git` | worktrees, diffs, history, graph lanes, diff layout, branch lists | no |
| `crates/project` | settings, repo scan, lazily expanded file tree | no |
| `crates/ui` | theme, scroll helpers, IME text input, diff drawing, small widgets | yes |
| `crates/editor` | the editor view, a thin layer over `text::Document` | yes |
| `crates/app` | the `maditor` binary: workspace/tabs, sidebar, git panel, overlays, chrome | yes |

Logic lives in the crates without gpui, so it is tested without a window; the app's tests drive
real keystrokes, wheel events and repos through GPUI's test harness.

## Run

```sh
cargo run -p maditor -- /path/to/a/repo   # the argument is optional; projects are remembered
cargo test --workspace
```

On macOS, GPUI needs Xcode's Metal toolchain: `xcodebuild -downloadComponent MetalToolchain`.
