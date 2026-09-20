//! A project's file tree: the rows visible given which folders are expanded. Folders are read
//! lazily, so opening a huge project costs only its top level.
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

/// Lists a directory: folders first, then files, each alphabetical ignoring case. `.git` and
/// `.DS_Store` are noise, not something to edit.
fn read_dir_sorted(dir: &Path) -> Vec<(PathBuf, String, bool)> {
    let mut entries: Vec<(PathBuf, String, bool)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name != ".git" && name != ".DS_Store").then(|| {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                (e.path(), name, is_dir)
            })
        })
        .collect();
    entries.sort_by_key(|(_, name, is_dir)| (!*is_dir, name.to_lowercase()));
    entries
}

/// The visible rows of the tree: a folder's children appear only while it is expanded.
pub fn build_tree_rows(root: &Path, expanded: &HashSet<PathBuf>) -> Vec<TreeRow> {
    fn walk(dir: &Path, depth: usize, expanded: &HashSet<PathBuf>, out: &mut Vec<TreeRow>) {
        for (path, name, is_dir) in read_dir_sorted(dir) {
            let open = is_dir && expanded.contains(&path);
            out.push(TreeRow { path: path.clone(), name, depth, is_dir, expanded: open });
            if open {
                walk(&path, depth + 1, expanded, out);
            }
        }
    }
    let mut rows = Vec::new();
    walk(root, 0, expanded, &mut rows);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_first_hides_git_and_expands_lazily() {
        let proj = std::env::temp_dir().join("madi-project-test-tree");
        let _ = std::fs::remove_dir_all(&proj);
        std::fs::create_dir_all(proj.join("src/nested")).unwrap();
        std::fs::create_dir_all(proj.join(".git")).unwrap();
        std::fs::write(proj.join("README.md"), "hello\n").unwrap();
        std::fs::write(proj.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(proj.join("src/nested/deep.txt"), "deep\n").unwrap();
        std::fs::write(proj.join("zeta.bin"), [0u8, 159, 146, 150]).unwrap();
        let names = |rows: &[TreeRow]| rows.iter().map(|r| format!("{}{}", " ".repeat(r.depth), r.name)).collect::<Vec<_>>();

        let mut expanded = HashSet::new();
        assert_eq!(names(&build_tree_rows(&proj, &expanded)), ["src", "README.md", "zeta.bin"], ".git is hidden, folders come first");

        expanded.insert(proj.join("src"));
        expanded.insert(proj.join("src/nested"));
        assert_eq!(
            names(&build_tree_rows(&proj, &expanded)),
            ["src", " nested", "  deep.txt", " main.rs", "README.md", "zeta.bin"]
        );
    }
}
