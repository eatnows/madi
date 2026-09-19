//! Worktrees of the open project: selecting one (which loads its changes), changing its pinned
//! base branch, removing it, and keyboard navigation through worktrees and their changed files.
use gpui::{prelude::*, Context};
use maditor_git::{diff, worktree::{self, WorktreeInfo}};
use maditor_ui::scroll::scroll_to_top;

use super::{FileItem, Maditor};

impl Maditor {
    /// While the git panel follows the selected worktree it shows that worktree's branch (or the
    /// default branch when only a project is selected).
    pub(super) fn sync_graph_branch(&mut self, cx: &mut Context<Self>) {
        let target = self
            .selected_wt
            .and_then(|i| self.worktrees.get(i))
            .and_then(|w| w.branch.clone())
            .or_else(|| {
                if self.branches.iter().any(|b| b == "main") {
                    Some("main".to_string())
                } else {
                    self.branches.first().cloned()
                }
            });
        self.git_panel.update(cx, |panel, cx| panel.follow(target, cx));
    }

    pub(super) fn select_worktree(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_wt = Some(ix);
        self.git_panel.update(cx, |panel, _| panel.resume_following());
        self.sync_graph_branch(cx);
        self.files.clear();
        self.file_items.clear();
        self.selected_file = None;
        scroll_to_top(&self.file_scroll);
        self.error = None;
        self.loading_diff = true;
        self.diff_gen += 1;
        let generation = self.diff_gen;

        let path = self.worktrees[ix].path.clone();
        let base = self.pins.get(&path).cloned().unwrap_or_default();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { diff::diff_against_base(path, base) }).await;
            this.update(cx, |this, cx| {
                if this.diff_gen != generation {
                    return;
                }
                this.loading_diff = false;
                match result {
                    Ok(result) => {
                        this.files = result.files;
                        let mut last = "";
                        for (i, f) in this.files.iter().enumerate() {
                            if f.section != last {
                                last = f.section;
                                this.file_items.push(FileItem::Label(if last == "committed" {
                                    "COMMITTED"
                                } else {
                                    "UNCOMMITTED"
                                }));
                            }
                            this.file_items.push(FileItem::File(i));
                        }
                        // The changes are listed, but no tab opens until one is picked: selecting a
                        // worktree shouldn't take over what the editor is showing.
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Picks a changed file of the selected worktree and opens its diff as a tab (a preview tab
    /// unless `preview` is false, so browsing with the arrow keys doesn't pile up tabs).
    pub(super) fn select_file(&mut self, ix: usize, preview: bool, cx: &mut Context<Self>) {
        self.selected_file = Some(ix);
        self.open_diff(ix, preview, cx);
    }

    /// Reloads the worktree list keeping the selection (by path); `reload_diff_for` re-diffs that
    /// worktree if it's the selected one.
    pub(super) fn refresh_worktrees(&mut self, reload_diff_for: Option<String>, cx: &mut Context<Self>) {
        let (repo, pins, generation) = (self.repo.clone(), self.pins.clone(), self.scan_gen);
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { worktree::list_worktrees(repo, pins) }).await;
            this.update(cx, |this, cx| {
                if this.scan_gen != generation {
                    return;
                }
                match result {
                    Ok(worktrees) => {
                        let selected_path = this.selected_wt.map(|i| this.worktrees[i].path.clone());
                        this.worktrees = worktrees;
                        this.selected_wt =
                            selected_path.and_then(|p| this.worktrees.iter().position(|w| w.path == p));
                        match (this.selected_wt, selected_path_matches(&this.selected_wt, &this.worktrees, reload_diff_for.as_deref())) {
                            (Some(ix), true) => this.select_worktree(ix, cx),
                            (None, _) => {
                                // The selected worktree is gone: clear its diff.
                                this.files.clear();
                                this.file_items.clear();
                                this.selected_file = None;
                                this.git_panel.update(cx, |panel, _| panel.resume_following());
                                this.sync_graph_branch(cx);
                            }
                            _ => {}
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn remove_worktree(&mut self, path: String, cx: &mut Context<Self>) {
        self.error = None;
        let repo = self.repo.clone();
        let generation = self.scan_gen;
        cx.spawn(async move |this, cx| {
            let target = path.clone();
            let result = cx
                .background_spawn(async move { worktree::remove_worktree(repo, target) })
                .await;
            this.update(cx, |this, cx| {
                if this.scan_gen != generation {
                    return;
                }
                match result {
                    Ok(()) => {
                        this.pins.remove(&path);
                        this.config.pins.insert(this.repo.clone(), this.pins.clone());
                        this.config.save();
                        this.refresh_worktrees(None, cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Pins a new base branch for a worktree, refreshing ahead/behind and (if it's the one on
    /// screen) its diff.
    pub(super) fn change_base(&mut self, worktree_path: String, branch: String, cx: &mut Context<Self>) {
        self.pins.insert(worktree_path.clone(), branch);
        self.config.pins.insert(self.repo.clone(), self.pins.clone());
        self.config.save();
        self.refresh_worktrees(Some(worktree_path), cx);
    }

    pub(super) fn move_worktree(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.worktrees.is_empty() {
            return;
        }
        let current = self.selected_wt.map(|i| i as isize).unwrap_or(-1);
        let next = (current + delta).clamp(0, self.worktrees.len() as isize - 1) as usize;
        if Some(next) != self.selected_wt {
            self.wt_scroll.scroll_to_item(next);
            self.select_worktree(next, cx);
        }
    }

    pub(super) fn move_file(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.files.is_empty() {
            return;
        }
        let current = self.selected_file.map(|i| i as isize).unwrap_or(-1);
        let next = (current + delta).clamp(0, self.files.len() as isize - 1) as usize;
        if Some(next) != self.selected_file {
            if let Some(pos) = self.file_items.iter().position(|it| matches!(it, FileItem::File(i) if *i == next)) {
                self.file_scroll.scroll_to_item(pos);
            }
            self.select_file(next, true, cx);
        }
    }
}

fn selected_path_matches(selected: &Option<usize>, worktrees: &[WorktreeInfo], path: Option<&str>) -> bool {
    match (selected, path) {
        (Some(i), Some(path)) => worktrees[*i].path == path,
        _ => false,
    }
}

