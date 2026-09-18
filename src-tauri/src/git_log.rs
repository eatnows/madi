use git2::{Repository, Sort};
use serde::Serialize;

#[derive(Serialize)]
pub struct CommitInfo {
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
    /// The commit message body (everything after the summary line), for showing full detail.
    pub body: String,
    pub author_name: String,
    pub author_email: String,
    /// Seconds since the Unix epoch (author time), for the frontend to format as "2 hours ago".
    pub timestamp: i64,
    pub parent_oids: Vec<String>,
}

/// Walks `branch`'s history (topological order, so parents always come after children) starting
/// at its tip, for rendering a commit graph. `skip`/`limit` page through it — the lane algorithm
/// needs every commit from the start to stay consistent, so the frontend re-derives lanes over
/// the full accumulated list on each page rather than this endpoint tracking lane state itself.
#[tauri::command]
pub fn git_log(repo_path: String, branch: String, skip: usize, limit: usize) -> Result<Vec<CommitInfo>, String> {
    let repo = Repository::open(&repo_path).map_err(|e| e.to_string())?;
    let target = repo
        .find_branch(&branch, git2::BranchType::Local)
        .map_err(|e| e.to_string())?
        .get()
        .target()
        .ok_or_else(|| format!("branch '{branch}' has no target"))?;

    let mut revwalk = repo.revwalk().map_err(|e| e.to_string())?;
    revwalk.push(target).map_err(|e| e.to_string())?;
    revwalk
        .set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for oid in revwalk.skip(skip).take(limit) {
        let oid = oid.map_err(|e| e.to_string())?;
        let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
        let author = commit.author();
        let oid_str = oid.to_string();
        out.push(CommitInfo {
            short_oid: oid_str[..7.min(oid_str.len())].to_string(),
            oid: oid_str,
            summary: commit.summary().ok().flatten().unwrap_or("").to_string(),
            body: commit.body().ok().flatten().unwrap_or("").to_string(),
            author_name: author.name().unwrap_or("").to_string(),
            author_email: author.email().unwrap_or("").to_string(),
            timestamp: commit.time().seconds(),
            parent_oids: commit.parent_ids().map(|id| id.to_string()).collect(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_main_branch_history() {
        let repo_root = env!("CARGO_MANIFEST_DIR").to_string() + "/..";
        let commits = git_log(repo_root, "main".to_string(), 0, 5).expect("git_log should succeed");
        assert!(!commits.is_empty());
        // Topological order: every commit's parent, if within the page, must appear later.
        let index_of: std::collections::HashMap<&str, usize> = commits
            .iter()
            .enumerate()
            .map(|(i, c)| (c.oid.as_str(), i))
            .collect();
        for (i, c) in commits.iter().enumerate() {
            for parent in &c.parent_oids {
                if let Some(&pi) = index_of.get(parent.as_str()) {
                    assert!(pi > i, "parent must come after child in topological order");
                }
            }
        }
    }

    #[test]
    fn skip_pages_through_the_same_sequence() {
        let repo_root = env!("CARGO_MANIFEST_DIR").to_string() + "/..";
        let whole = git_log(repo_root.clone(), "main".to_string(), 0, 4).unwrap();
        let page1 = git_log(repo_root.clone(), "main".to_string(), 0, 2).unwrap();
        let page2 = git_log(repo_root, "main".to_string(), 2, 2).unwrap();
        let paged: Vec<&str> = page1.iter().chain(&page2).map(|c| c.oid.as_str()).collect();
        let unpaged: Vec<&str> = whole.iter().map(|c| c.oid.as_str()).collect();
        assert_eq!(paged, unpaged);
    }
}
