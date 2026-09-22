# Madi

A native Rust code editor built on the [gyeol](https://github.com/eatnows/gyeol) UI toolkit and centered on git worktrees: edit files,
and review what each worktree changed against its base branch without leaving the editor.

- **Files** – the project's tree; files open as tabs in the editor (Korean/IME input works).
- **Worktrees** – every git worktree with ahead/behind against a pinned base branch; a changed
  file opens as a diff tab (side-by-side, or unified when narrow).
- **Graph** – a commit graph for any branch, with the commit's files and diff.
- Settings (`Cmd+,`): theme (follows the OS or forced light/dark) and editor font size, in tabs so more can be added.
- Focus mode (top-bar icon, `Cmd+Alt+Z`): hides the rail, sidebar, git panel and status bar.
- Syntax highlighting and "go to symbol" for Rust, TOML, JSON and Markdown, via bundled tree-sitter grammars.

## Install

Via [Homebrew](https://brew.sh) (macOS):

```sh
brew tap eatnows/madi
brew install madi
```

This builds from source, so it needs the Rust toolchain (`brew install rust`, or Homebrew installs it
automatically as a build dependency) — the first install takes a minute or two.

## Layout

A Cargo workspace split by responsibility. The model crates have no UI dependency; the app draws
them with gyeol:

| crate | what it holds | UI runtime |
| --- | --- | --- |
| `crates/text` | text buffer with undo, and `Document`: caret, selection, editing commands, IME offsets | no |
| `crates/git` | worktrees, diffs, history, graph lanes, diff layout, branch lists | no |
| `crates/project` | settings, repo scan, lazily expanded file tree | no |
| `crates/plugins` | downloading, checksum-verifying and installing language plugins (not wired to highlighting yet) | no |
| `crates/syntax` | syntax highlighting and symbol extraction, via bundled tree-sitter grammars | no |
| `crates/app` | the `madi` binary: editor, workspace/tabs, sidebar, git panel, overlays, chrome | [gyeol](https://github.com/eatnows/gyeol) |

Logic lives in the crates without UI dependencies. The app's TestHost tests drive keystrokes,
IME, mouse events, scrolling, and repositories without opening a window.

## Build from source

```sh
cargo run -p madi -- /path/to/a/repo   # a folder (remembered as a project) or a single file; optional
cargo test --workspace
```
