# Maditor

A native (Rust + [GPUI](https://www.gpui.rs)) code editor built around git worktrees: edit files,
and review what each worktree changed against its base branch without leaving the editor.

- **Files** – the project's tree; files open as tabs in the editor (Korean/IME input works).
- **Worktrees** – every git worktree with ahead/behind against a pinned base branch; a changed
  file opens as a diff tab (side-by-side, or unified when narrow).
- **Graph** – a commit graph for any branch, with the commit's files and diff.
- Light and dark themes (follows the OS, or pick one in the top bar).

## Layout

- `core/` – git logic (worktrees, diffs, log) with no UI dependency.
- `native/` – the GPUI app.

## Run

```sh
cd native
cargo run -- /path/to/a/repo   # the argument is optional; projects are remembered
cargo test
```

On macOS, GPUI needs Xcode's Metal toolchain: `xcodebuild -downloadComponent MetalToolchain`.
