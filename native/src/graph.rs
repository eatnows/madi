//! Commit graph shaping: which "lane" each commit sits in and how lines connect between rows, as
//! `git log --graph` draws it. Pure so it's unit-tested without any UI.
use maditor_core::git_log::CommitInfo;

pub struct GraphRow {
    pub lane: usize,
    /// Lanes with a straight line passing through this row untouched.
    pub pass_through: Vec<usize>,
    /// Other lanes converging into `lane` at this row, drawn as curves joining up into it.
    pub converge_from: Vec<usize>,
    /// New lanes spawned by this commit's extra (merge) parents, curving away from `lane`.
    pub diverge_to: Vec<usize>,
    /// Whether this commit's own lane continues downward (it has a first parent to walk to).
    pub continues: bool,
    pub max_lane: usize,
}

/// Assigns each commit a lane. `lanes[k]` holds the oid lane `k` is waiting to reach next.
/// Commits arrive in topological order (children before parents), so each commit lands in the lane
/// already waiting for it; merge commits open extra lanes for their other parents, and lanes that
/// end up waiting for the same commit converge and get freed.
pub fn compute_rows(commits: &[CommitInfo]) -> Vec<GraphRow> {
    let mut lanes: Vec<Option<&str>> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let matches: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(_, waiting)| **waiting == Some(commit.oid.as_str()))
            .map(|(i, _)| i)
            .collect();

        let lane = match matches.first() {
            Some(&first) => {
                for &extra in &matches[1..] {
                    lanes[extra] = None;
                }
                first
            }
            None => lanes.iter().position(Option::is_none).unwrap_or(lanes.len()),
        };
        if lane == lanes.len() {
            lanes.push(None);
        }

        let pass_through: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(i, waiting)| *i != lane && waiting.is_some() && !matches.contains(i))
            .map(|(i, _)| i)
            .collect();

        let mut parents = commit.parent_oids.iter();
        let first_parent = parents.next();
        lanes[lane] = first_parent.map(String::as_str);

        let mut diverge_to = Vec::new();
        for parent in parents {
            if lanes.iter().any(|w| *w == Some(parent.as_str())) {
                continue; // already tracked; converges naturally further down
            }
            let free = lanes.iter().position(Option::is_none).unwrap_or(lanes.len());
            if free == lanes.len() {
                lanes.push(None);
            }
            lanes[free] = Some(parent.as_str());
            diverge_to.push(free);
        }

        let max_lane = pass_through
            .iter()
            .chain(&diverge_to)
            .copied()
            .chain([lane])
            .max()
            .unwrap_or(0);
        rows.push(GraphRow {
            lane,
            pass_through,
            converge_from: matches.iter().skip(1).copied().collect(),
            diverge_to,
            continues: first_parent.is_some(),
            max_lane,
        });
    }
    rows
}

/// "5m ago", "3h ago", "2d ago", "4mo ago", "1y ago".
pub fn relative_time(now: i64, timestamp: i64) -> String {
    let minutes = ((now - timestamp) as f64 / 60.0).round() as i64;
    if minutes < 1 {
        return "just now".into();
    }
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = (minutes as f64 / 60.0).round() as i64;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = (hours as f64 / 24.0).round() as i64;
    if days < 30 {
        return format!("{days}d ago");
    }
    let months = (days as f64 / 30.0).round() as i64;
    if months < 12 {
        return format!("{months}mo ago");
    }
    format!("{}y ago", (months as f64 / 12.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(oid: &str, parents: &[&str]) -> CommitInfo {
        CommitInfo {
            oid: oid.into(),
            short_oid: oid.into(),
            summary: String::new(),
            body: String::new(),
            author_name: String::new(),
            author_email: String::new(),
            timestamp: 0,
            parent_oids: parents.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_linear_history_stays_in_lane_zero() {
        let rows = compute_rows(&[commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])]);
        assert!(rows.iter().all(|r| r.lane == 0 && r.max_lane == 0 && r.pass_through.is_empty()));
        assert_eq!(rows.iter().map(|r| r.continues).collect::<Vec<_>>(), [true, true, false]);
    }

    #[test]
    fn a_merge_opens_a_lane_that_converges_back_at_the_fork_point() {
        // M merges C1 (main) and B1 (branch); both descend from BASE.
        let rows = compute_rows(&[
            commit("M", &["C1", "B1"]),
            commit("C1", &["BASE"]),
            commit("B1", &["BASE"]),
            commit("BASE", &[]),
        ]);
        assert_eq!((rows[0].lane, rows[0].diverge_to.clone()), (0, vec![1]));
        assert_eq!((rows[1].lane, rows[1].pass_through.clone()), (0, vec![1]));
        assert_eq!((rows[2].lane, rows[2].pass_through.clone()), (1, vec![0]));
        assert_eq!((rows[3].lane, rows[3].converge_from.clone()), (0, vec![1]));
        assert_eq!(rows.iter().map(|r| r.max_lane).collect::<Vec<_>>(), [1, 1, 1, 0]);
    }

    #[test]
    fn formats_relative_times() {
        assert_eq!(relative_time(1000, 990), "just now");
        assert_eq!(relative_time(10_000, 10_000 - 5 * 60), "5m ago");
        assert_eq!(relative_time(100_000, 100_000 - 3 * 3600), "3h ago");
        assert_eq!(relative_time(10_000_000, 10_000_000 - 2 * 86_400), "2d ago");
    }
}
