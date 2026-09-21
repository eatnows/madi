# Madi development status

Last updated: 2026-09-21

## Current architecture

Madi is now a native Rust editor built on the local `gyeol` UI toolkit. The previous GPUI application, UI, and editor crates have been removed. The active binary is `madi` in `crates/app`.

```sh
cargo run -p madi -- /path/to/repository
cargo test --workspace
```

The model crates remain UI-independent:

- `crates/text`: document buffer, selection, undo/redo, IME handling
- `crates/git`: worktrees, diffs, commit graph, branch data
- `crates/project`: config, project scanning, file tree
- `crates/app`: Gyeol-based Madi application

## Implemented

### Project and file navigation

- Project rail with a native macOS folder picker
- File tree with lazy folder expansion
- Resizable sidebar (180–520px)
- File and folder context menu: create file, create folder, rename, delete
- Delete requires typing `DELETE` in the system confirmation prompt
- `Cmd+P` / `Ctrl+P`: Quick Open, with ordered-character path filtering, keyboard navigation, and IME input
- `Cmd+Shift+F` / `Ctrl+Shift+F`: project-wide text search; selecting a result opens the matching file and line

### Editor

- Tabs, preview tabs, dirty state, and tab closing confirmation
- Drag tabs to reorder them
- Save: `Cmd+S` / `Ctrl+S`
- Undo / redo: `Cmd+Z` / `Ctrl+Z`, `Cmd+Shift+Z` / `Ctrl+Shift+Z`
- Clipboard shortcuts: select all, copy, cut, paste
- Korean IME composition
- Mouse caret placement, selection drag, double-click word selection
- Automatic indentation on a new line
- Matching completion for `()`, `[]`, `{}`, single quotes, and double quotes; typing an existing closing delimiter moves over it
- Font-size setting, light/dark/system theme, focus mode

### Git and worktrees

- Worktree list, base-branch picker, removal confirmation, and changed-file diff tabs
- Split diff view with virtualized rows and horizontal scrolling
- Commit graph with curved lanes, branch picker, resize handles, and paged history loading
- Project and worktree context menus

### Settings and plugins

- Settings tabs: General, Editor, Plugins
- The Plugins tab is currently a UI placeholder with an empty-state view.
- Planned language plugins supply file extensions, Tree-sitter grammar/highlight queries, and optional LSP/analyzer metadata.

## Language-plugin direction

Language support should be installed rather than bundled into the editor. The editor checks a file extension against installed plugin manifests.

```text
file extension → installed language plugin → syntax grammar/highlight queries
                                      └→ optional analyzer / LSP server → completions and diagnostics
```

Syntax highlighting requires a grammar and highlight query. Suggestions, diagnostics, definition lookup, and rename additionally require an analyzer/LSP server such as `rust-analyzer` or `typescript-language-server`.

Plugin installation should support platform-specific analyzer downloads, version pinning, SHA-256 verification, and use of an already installed analyzer where possible.

## Not yet implemented

- Plugin registry, downloads, integrity verification, installed-plugin storage, and activation
- Tree-sitter syntax highlighting
- LSP/analyzer lifecycle and editor features: completions, diagnostics, go-to-definition, rename
- Split editor panes (horizontal and vertical)
- Recently opened files and session restoration
- Symbol navigation and code folding
- Git stage/unstage, commit creation, branch create/switch workflows

## Recent commits

- `0b9d904` Add plugins settings page
- `e11fadc` Add file explorer management actions
- `2c4417b` Search text across project files
- `32cce0e` Complete matching delimiters while editing
- `86f7a7a` Allow tabs to be reordered by dragging
- `6004e8e` Add Quick Open for project files
- `31730b3` Add a resizable sidebar
- `8c9e8b1` Replace the GPUI app with Gyeol

## Verification

Use `cargo test --workspace` before merging a feature. UI behavior is covered with Gyeol's windowless `TestHost` tests; these exercise keyboard shortcuts, IME, clicks, dragging, scrolling, and temporary Git repositories.
