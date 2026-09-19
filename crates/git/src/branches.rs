//! Branch list shaping: groups "/"-prefixed names into collapsible folders and filters by a
//! search query, as a flat list of rows a picker can show.
use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
struct Node {
    full_path: String,
    is_branch: bool,
    children: BTreeMap<String, Node>,
}

#[derive(Debug, PartialEq)]
pub enum BranchRow {
    Folder { full_path: String, segment: String, depth: usize },
    Branch { full_path: String, depth: usize },
}

fn build_tree(branches: &[String]) -> Node {
    let mut root = Node::default();
    for branch in branches {
        let mut node = &mut root;
        let mut path = String::new();
        for part in branch.split('/') {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            node = node
                .children
                .entry(part.to_string())
                .or_insert_with(|| Node { full_path: path.clone(), ..Default::default() });
        }
        node.is_branch = true;
    }
    root
}

fn has_match(node: &Node, query: &str) -> bool {
    (node.is_branch && node.full_path.to_lowercase().contains(query)) || node.children.values().any(|c| has_match(c, query))
}

fn flatten_node(node: &Node, depth: usize, collapsed: &HashSet<String>, query: &str, out: &mut Vec<BranchRow>) {
    let mut children: Vec<(&String, &Node)> = node.children.iter().collect();
    children.sort_by_key(|(segment, _)| segment.to_lowercase());
    for (segment, child) in children {
        if !query.is_empty() && !has_match(child, query) {
            continue;
        }
        if child.children.is_empty() {
            out.push(BranchRow::Branch { full_path: child.full_path.clone(), depth });
            continue;
        }
        out.push(BranchRow::Folder { full_path: child.full_path.clone(), segment: segment.clone(), depth });
        // While searching, every folder on the way to a match stays open so results are reachable.
        if !query.is_empty() || !collapsed.contains(&child.full_path) {
            flatten_node(child, depth + 1, collapsed, query, out);
            if child.is_branch && child.full_path.to_lowercase().contains(query) {
                out.push(BranchRow::Branch { full_path: child.full_path.clone(), depth: depth + 1 });
            }
        }
    }
}

pub fn flatten(branches: &[String], collapsed: &HashSet<String>, query: &str) -> Vec<BranchRow> {
    let mut out = Vec::new();
    flatten_node(&build_tree(branches), 0, collapsed, &query.to_lowercase(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn label(row: &BranchRow) -> String {
        match row {
            BranchRow::Folder { full_path, depth, .. } => format!("{}{full_path}/", " ".repeat(*depth)),
            BranchRow::Branch { full_path, depth } => format!("{}{full_path}", " ".repeat(*depth)),
        }
    }

    fn labels(rows: &[BranchRow]) -> Vec<String> {
        rows.iter().map(label).collect()
    }

    #[test]
    fn groups_prefixes_into_folders() {
        let rows = flatten(&names(&["main", "feature/a", "feature/b", "dev"]), &HashSet::new(), "");
        assert_eq!(labels(&rows), ["dev", "feature/", " feature/a", " feature/b", "main"]);
    }

    #[test]
    fn collapsed_folder_hides_children() {
        let collapsed = HashSet::from(["feature".to_string()]);
        let rows = flatten(&names(&["main", "feature/a"]), &collapsed, "");
        assert_eq!(labels(&rows), ["feature/", "main"]);
    }

    #[test]
    fn search_keeps_the_folder_path_and_overrides_collapse() {
        let collapsed = HashSet::from(["feature".to_string()]);
        let rows = flatten(&names(&["main", "feature/alpha", "feature/beta"]), &collapsed, "ALP");
        assert_eq!(labels(&rows), ["feature/", " feature/alpha"]);
    }

    #[test]
    fn a_name_that_is_both_branch_and_folder_shows_up_as_both() {
        let rows = flatten(&names(&["feature", "feature/x"]), &HashSet::new(), "");
        assert_eq!(labels(&rows), ["feature/", " feature/x", " feature"]);
    }
}
