//! A project's lifecycle: opening (and remembering) a folder, scanning its git repo off the UI
//! thread, and closing it.
use gpui::{prelude::*, Context, PathPromptOptions};
use maditor_project::scan::{scan_repo, ScanOutcome};

use super::{Confirm, ConfirmAction, Maditor, SidebarView};

impl Maditor {
    pub(super) fn open_project(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.config.projects.contains(&path) {
            self.config.projects.push(path.clone());
        }
        self.config.last_project = Some(path.clone());
        self.config.save();
        self.scan(path, cx);
    }

    pub(super) fn reset_view(&mut self) {
        self.issue = None;
        self.error = None;
        self.worktrees.clear();
        self.branches.clear();
        self.picker = None;
        self.selected_wt = None;
        self.files.clear();
        self.file_items.clear();
        self.selected_file = None;
        self.loading_diff = false;
        self.diff_gen += 1;
        self.follow_worktree = true;
        self.graph_branch.clear();
        self.clear_graph();
    }

    pub(super) fn scan(&mut self, path: String, cx: &mut Context<Self>) {
        self.reset_view();
        self.repo = path.clone();
        self.scan_gen += 1;
        let generation = self.scan_gen;
        let saved = self.config.pins.get(&path).cloned().unwrap_or_default();
        self.refresh_tree();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let repo = path.clone();
            let result = cx.background_spawn(async move { scan_repo(repo, saved) }).await;
            this.update(cx, |this, cx| {
                if this.scan_gen != generation {
                    return;
                }
                match result {
                    Ok(ScanOutcome::Issue(issue)) => {
                        this.issue = Some(issue);
                        // Without git there are no worktrees to show, but the files can still be edited.
                        this.sidebar_view = SidebarView::Files;
                    }
                    Ok(ScanOutcome::Loaded(scan)) => {
                        this.worktrees = scan.worktrees;
                        this.branches = scan.branches;
                        this.pins = scan.pins.clone();
                        this.config.pins.insert(path, scan.pins);
                        this.config.save();
                        this.sync_graph_branch(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn add_project(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open a git repository".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                if let Some(path) = paths.into_iter().next() {
                    let path = path.to_string_lossy().into_owned();
                    this.update(cx, |this, cx| this.open_project(path, cx)).ok();
                }
            }
        })
        .detach();
    }

    /// Closing a project drops its open files, so unsaved changes get a confirmation first.
    pub(super) fn request_close_project(&mut self, path: &str, cx: &mut Context<Self>) {
        let dirty = self.dirty_tab_count(path, cx);
        if dirty == 0 {
            self.close_project(path, cx);
            return;
        }
        self.confirm = Some(Confirm {
            title: "Unsaved changes".into(),
            message: format!("{dirty} open file(s) in this project have unsaved changes. Close the project and discard them?"),
            label: "Discard".into(),
            action: ConfirmAction::CloseProject(path.to_string()),
        });
        cx.notify();
    }

    pub(super) fn close_project(&mut self, path: &str, cx: &mut Context<Self>) {
        self.workspaces.remove(path);
        let idx = self.config.projects.iter().position(|p| p == path);
        self.config.projects.retain(|p| p != path);
        if self.config.last_project.as_deref() == Some(path) {
            self.config.last_project = None;
        }
        self.config.save();
        if self.repo == path {
            let next = idx
                .and_then(|i| self.config.projects.get(i.min(self.config.projects.len().saturating_sub(1))))
                .cloned();
            match next {
                Some(next) => self.open_project(next, cx),
                None => {
                    self.reset_view();
                    self.repo.clear();
                }
            }
        }
        cx.notify();
    }
}
