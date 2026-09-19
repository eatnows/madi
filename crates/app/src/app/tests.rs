//! Harness tests for the app: real repos, real keystrokes, wheel events.
use super::*;
use gpui::{Entity, TestAppContext};
use maditor_git::branches::BranchRow;

/// The graph list is at most this much wider than its pane (820 min width vs the 460 pane).
const GRAPH_MIN_WIDTH_FOR_TEST: f32 = 820.0 - 400.0;
use std::{path::Path, process::Command};

/// The active tab's diff data, if it is a diff.
fn active_diff(m: &Maditor) -> Option<&workspace::DiffTab> {
    match &m.active_tab()?.body {
        workspace::TabBody::Diff(d) => Some(d),
        _ => None,
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A repo with `main` plus a linked worktree `wt` (branch feature) that edits one file.
fn fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("maditor-native-test-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(repo.join("b.txt"), "x\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let wt = root.join("wt");
    git(&repo, &["worktree", "add", "-q", "-b", "feature", wt.to_str().unwrap()]);
    std::fs::write(wt.join("a.txt"), "one\nTWO\n").unwrap();
    std::fs::write(wt.join("b.txt"), "y\n").unwrap();
    git(&wt, &["commit", "-qam", "edit"]);
    (root, repo)
}

fn config_in(root: &Path) -> Config {
    Config::at(Some(root.join("config.json")))
}

#[gpui::test]
fn scans_a_repo_and_loads_a_worktree_diff(cx: &mut TestAppContext) {
    let (root, repo) = fixture("scan");
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
    cx.run_until_parked();

    view.read_with(cx, |m, _| {
        assert_eq!(m.worktrees.len(), 2);
        assert!(m.pins.values().all(|b| b == "main"));
        assert_eq!(m.config.projects, vec![path.clone()]);
    });

    let wt_ix = view.read_with(cx, |m, _| m.worktrees.iter().position(|w| !w.is_main).unwrap());
    view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
    cx.run_until_parked();

    view.read_with(cx, |m, _| {
        assert!(!m.loading_diff);
        assert_eq!(m.files.len(), 2);
        assert_eq!(m.selected_file, None, "changes are listed, nothing opens by itself");
        assert!(m.active_tab().is_none());
    });

    // Picking a changed file opens its diff as a (preview) tab.
    view.update(cx, |m, cx| m.select_file(0, true, cx));
    view.read_with(cx, |m, _| {
        assert_eq!(m.selected_file, Some(0));
        let tab = m.active_tab().expect("a diff tab opened");
        assert!(tab.preview);
        assert!(active_diff(m).is_some_and(|d| d.additions + d.deletions > 0));
    });
}

#[gpui::test]
fn arrow_navigation_clamps_at_both_ends(cx: &mut TestAppContext) {
    let (root, repo) = fixture("nav");
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
    cx.run_until_parked();

    view.update(cx, |m, cx| m.move_worktree(1, cx));
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |m, _| m.selected_wt), Some(0));
    view.update(cx, |m, cx| m.move_worktree(1, cx));
    view.update(cx, |m, cx| m.move_worktree(1, cx));
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |m, _| m.selected_wt), Some(1), "clamped at the last worktree");

    let on_wt = view.read_with(cx, |m, _| !m.worktrees[m.selected_wt.unwrap()].is_main);
    if !on_wt {
        view.update(cx, |m, cx| m.move_worktree(-1, cx));
        cx.run_until_parked();
    }
    view.update(cx, |m, cx| m.move_file(1, cx));
    assert_eq!(view.read_with(cx, |m, _| m.selected_file), Some(0), "nothing was selected yet, so down picks the first");
    view.update(cx, |m, cx| m.move_file(1, cx));
    assert_eq!(view.read_with(cx, |m, _| m.selected_file), Some(1));
    assert_eq!(view.read_with(cx, |m, _| m.workspaces[&m.repo].tabs.len()), 1, "browsing reuses one preview tab");
    view.update(cx, |m, cx| m.move_file(1, cx));
    assert_eq!(view.read_with(cx, |m, _| m.selected_file), Some(1), "clamped at the last file");
    view.update(cx, |m, cx| m.move_file(-5, cx));
    assert_eq!(view.read_with(cx, |m, _| m.selected_file), Some(0));
}

#[gpui::test]
fn picking_a_base_branch_pins_it_and_refreshes_ahead_behind(cx: &mut TestAppContext) {
    let (root, repo) = fixture("pick");
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
    cx.run_until_parked();

    let (wt_ix, wt_path) = view.read_with(cx, |m, _| {
        let ix = m.worktrees.iter().position(|w| !w.is_main).unwrap();
        (ix, m.worktrees[ix].path.clone())
    });
    assert_eq!(view.read_with(cx, |m, _| (m.worktrees[wt_ix].ahead, m.worktrees[wt_ix].behind)), (Some(1), Some(0)));

    view.update_in(cx, |m, window, cx| {
        m.open_picker(PickerTarget::WorktreeBase(wt_path.clone()), Point::default(), window, cx)
    });
    // Rows are alphabetical: feature, main. Activate "feature" (the worktree's own branch).
    let feature_row = view.read_with(cx, |m, cx| {
        m.picker_rows(cx)
            .iter()
            .position(|r| matches!(r, BranchRow::Branch { full_path, .. } if full_path == "feature"))
            .unwrap()
    });
    view.update_in(cx, |m, window, cx| m.picker_activate(feature_row, window, cx));
    cx.run_until_parked();

    view.read_with(cx, |m, _| {
        assert!(m.picker.is_none(), "picker closes after a pick");
        assert_eq!(m.pins.get(&wt_path).map(String::as_str), Some("feature"));
        let ix = m.worktrees.iter().position(|w| w.path == wt_path).unwrap();
        assert_eq!((m.worktrees[ix].ahead, m.worktrees[ix].behind), (Some(0), Some(0)));
    });
    assert_eq!(
        config_in(&root).pins[&path][&wt_path], "feature",
        "the pin was persisted"
    );
}

#[gpui::test]
fn graph_follows_the_worktree_until_pinned(cx: &mut TestAppContext) {
    let (root, repo) = fixture("graph");
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
    cx.run_until_parked();

    view.read_with(cx, |m, _| {
        assert_eq!(m.graph_branch, "main", "with only a project selected, default to main");
        assert_eq!(m.commits.len(), 1);
    });

    let wt_ix = view.read_with(cx, |m, _| m.worktrees.iter().position(|w| !w.is_main).unwrap());
    view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
    cx.run_until_parked();
    view.read_with(cx, |m, _| {
        assert_eq!(m.graph_branch, "feature");
        assert_eq!(m.commits.len(), 2);
        assert!(m.follow_worktree);
    });

    // Pin the graph to main via the panel's own picker: it stops following.
    view.update_in(cx, |m, window, cx| m.open_picker(PickerTarget::GraphBranch, Point::default(), window, cx));
    let main_row = view.read_with(cx, |m, cx| {
        m.picker_rows(cx)
            .iter()
            .position(|r| matches!(r, BranchRow::Branch { full_path, .. } if full_path == "main"))
            .unwrap()
    });
    view.update_in(cx, |m, window, cx| m.picker_activate(main_row, window, cx));
    cx.run_until_parked();
    view.read_with(cx, |m, _| {
        assert_eq!((m.graph_branch.as_str(), m.follow_worktree, m.commits.len()), ("main", false, 1));
    });

    // Clicking a worktree resumes following.
    view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
    cx.run_until_parked();
    view.read_with(cx, |m, _| assert_eq!((m.graph_branch.as_str(), m.follow_worktree), ("feature", true)));
}

#[gpui::test]
fn selecting_a_commit_loads_its_files_and_reclicking_deselects(cx: &mut TestAppContext) {
    let (root, repo) = fixture("commit");
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
    cx.run_until_parked();
    let wt_ix = view.read_with(cx, |m, _| m.worktrees.iter().position(|w| !w.is_main).unwrap());
    view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
    cx.run_until_parked();

    view.update(cx, |m, cx| m.select_commit(0, true, cx));
    cx.run_until_parked();
    view.read_with(cx, |m, _| {
        assert_eq!(m.selected_oid.as_deref(), Some(m.commits[0].oid.as_str()));
        assert_eq!(m.commit_files.len(), 2, "the tip commit edits a.txt and b.txt");
        assert_eq!(m.selected_cfile, Some(0));
        assert!(!m.commit_diff.is_empty());
    });

    view.update(cx, |m, cx| m.move_commit(1, cx));
    cx.run_until_parked();
    view.read_with(cx, |m, _| {
        assert_eq!(m.selected_oid.as_deref(), Some(m.commits[1].oid.as_str()));
        assert_eq!(m.commit_files.len(), 2, "the root commit adds both files");
    });
    view.update(cx, |m, cx| m.move_commit(1, cx));
    view.read_with(cx, |m, _| assert_eq!(m.selected_oid.as_deref(), Some(m.commits[1].oid.as_str()), "clamped"));

    view.update(cx, |m, cx| m.select_commit(1, true, cx));
    view.read_with(cx, |m, _| assert!(m.selected_oid.is_none() && m.commit_files.is_empty()));
}

#[gpui::test]
fn removing_a_worktree_keeps_the_other_selection_and_drops_its_pin(cx: &mut TestAppContext) {
    let (root, repo) = fixture("remove");
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
    cx.run_until_parked();

    let (main_ix, wt_path) = view.read_with(cx, |m, _| {
        (
            m.worktrees.iter().position(|w| w.is_main).unwrap(),
            m.worktrees.iter().find(|w| !w.is_main).unwrap().path.clone(),
        )
    });
    view.update(cx, |m, cx| m.select_worktree(main_ix, cx));
    cx.run_until_parked();

    view.update(cx, |m, cx| m.remove_worktree(wt_path.clone(), cx));
    cx.run_until_parked();

    view.read_with(cx, |m, _| {
        assert_eq!(m.worktrees.len(), 1);
        assert!(m.worktrees[0].is_main);
        assert_eq!(m.selected_wt, Some(0), "the still-existing selection is kept");
        assert!(!m.pins.contains_key(&wt_path));
        assert!(m.error.is_none());
    });
    assert!(!std::path::Path::new(&wt_path).exists(), "the working directory is gone");
    assert!(!config_in(&root).pins[&path].contains_key(&wt_path));
}

#[gpui::test]
fn removing_the_selected_worktree_clears_its_diff(cx: &mut TestAppContext) {
    let (root, repo) = fixture("remove-selected");
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
    cx.run_until_parked();
    let (wt_ix, wt_path) = view.read_with(cx, |m, _| {
        let ix = m.worktrees.iter().position(|w| !w.is_main).unwrap();
        (ix, m.worktrees[ix].path.clone())
    });
    view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |m, _| !m.files.is_empty()));

    view.update(cx, |m, cx| m.remove_worktree(wt_path, cx));
    cx.run_until_parked();
    view.read_with(cx, |m, _| {
        assert_eq!(m.selected_wt, None);
        assert!(m.files.is_empty());
        assert_eq!(m.graph_branch, "main", "the graph falls back to the default branch");
    });
}

#[gpui::test]
fn vertical_wheel_over_the_graph_scrolls_the_list_not_sideways(cx: &mut TestAppContext) {
    let (root, repo) = fixture("wheel");
    // Enough history that the list overflows vertically.
    for i in 0..40 {
        std::fs::write(repo.join("a.txt"), format!("rev {i}\n")).unwrap();
        git(&repo, &["commit", "-qam", &format!("commit {i}")]);
    }
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
    cx.run_until_parked();
    view.update(cx, |m, cx| {
        m.git_open = true;
        cx.notify();
    });
    cx.run_until_parked();

    let viewport = cx.update(|window, _| window.viewport_size());
    // Over the graph list: bottom-left region, above the bottom bar.
    let over_graph = gpui::point(gpui::px(300.), viewport.height - gpui::px(150.));
    cx.simulate_mouse_move(over_graph, None, gpui::Modifiers::default());
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: over_graph,
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-120.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();

    view.read_with(cx, |m, _| {
        let list = m.graph_scroll.0.borrow().base_handle.offset();
        let sideways = m.graph_hscroll.offset();
        assert!(list.y < gpui::px(0.), "the list scrolled down: {list:?}");
        assert_eq!(sideways.x, gpui::px(0.), "and did not move sideways: {sideways:?}");
    });
}

/// Opens a repo with enough history to overflow the graph, git panel open, mouse over the list.
fn graph_with_overflow<'a>(
    name: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<Maditor>, &'a mut gpui::VisualTestContext, gpui::Point<Pixels>) {
    let (root, repo) = fixture(name);
    for i in 0..40 {
        std::fs::write(repo.join("a.txt"), format!("rev {i}\n")).unwrap();
        git(&repo, &["commit", "-qam", &format!("commit {i}")]);
    }
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
    cx.run_until_parked();
    view.update(cx, |m, cx| {
        m.git_open = true;
        cx.notify();
    });
    cx.run_until_parked();
    let viewport = cx.update(|window, _| window.viewport_size());
    let over_graph = gpui::point(px(300.), viewport.height - px(150.));
    cx.simulate_mouse_move(over_graph, None, gpui::Modifiers::default());
    (view, cx, over_graph)
}

fn wheel(cx: &mut gpui::VisualTestContext, at: gpui::Point<Pixels>, dx: f32, dy: f32) {
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: at,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(dx), px(dy))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
}

#[gpui::test]
fn horizontal_wheel_over_the_graph_scrolls_sideways_only(cx: &mut TestAppContext) {
    let (view, cx, at) = graph_with_overflow("hwheel", cx);

    wheel(cx, at, -80., 0.);
    view.read_with(cx, |m, _| {
        let list = m.graph_scroll.0.borrow().base_handle.offset();
        let side = m.graph_hscroll.offset();
        assert!(side.x < px(0.), "scrolled sideways: {side:?}");
        assert_eq!(list.y, px(0.), "the list must not drift vertically: {list:?}");
    });

    // Far past the end: clamped to the content, never into blank space.
    wheel(cx, at, -5000., 0.);
    let max = view.read_with(cx, |m, _| m.graph_hscroll.offset().x);
    assert!(max >= px(-(GRAPH_MIN_WIDTH_FOR_TEST)), "clamped at the right edge: {max:?}");
    // ...and back to exactly the left edge, not past it.
    wheel(cx, at, 5000., 0.);
    view.read_with(cx, |m, _| assert_eq!(m.graph_hscroll.offset().x, px(0.)));
}

#[gpui::test]
fn diff_tab_scrolls_both_ways_independently_and_keeps_its_position_per_tab(cx: &mut TestAppContext) {
    let (root, repo) = fixture("diff-scroll");
    let wt = root.join("wt");
    // 120 lines, one very long: overflows the diff pane both vertically and horizontally.
    let big: String = (0..120)
        .map(|i| if i == 60 { format!("{}\n", "x".repeat(600)) } else { format!("line {i}\n") })
        .collect();
    std::fs::write(wt.join("big.txt"), big).unwrap();
    git(&wt, &["add", "big.txt"]);
    git(&wt, &["commit", "-qm", "big"]);

    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
    cx.run_until_parked();
    let wt_ix = view.read_with(cx, |m, _| m.worktrees.iter().position(|w| !w.is_main).unwrap());
    view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
    cx.run_until_parked();
    let big_ix = view.read_with(cx, |m, _| m.files.iter().position(|f| f.path == "big.txt").unwrap());
    view.update(cx, |m, cx| m.select_file(big_ix, false, cx));
    cx.run_until_parked();

    let over_diff = gpui::point(px(1200.), px(500.));
    cx.simulate_mouse_move(over_diff, None, gpui::Modifiers::default());
    let offsets = |cx: &mut gpui::VisualTestContext| {
        view.read_with(cx, |m, _| {
            let d = active_diff(m).expect("the diff tab is active");
            (d.hscroll.offset(), d.vscroll.0.borrow().base_handle.offset())
        })
    };
    assert!(view.read_with(cx, |m, _| maditor_ui::diff_view::width(&active_diff(m).unwrap().data, false)) > 1400., "content is wider than the pane");

    wheel(cx, over_diff, -300., 0.);
    let (side, list) = offsets(cx);
    assert!(side.x < px(0.), "horizontal wheel scrolls the diff sideways: {side:?}");
    assert_eq!(list.y, px(0.), "and does not drift vertically: {list:?}");

    wheel(cx, over_diff, 0., -200.);
    let (side_after, list) = offsets(cx);
    assert!(list.y < px(0.), "vertical wheel scrolls the list: {list:?}");
    assert_eq!(side_after.x, side.x, "and does not move it sideways");

    // Far past either end is clamped to the content.
    wheel(cx, over_diff, 20000., 0.);
    assert_eq!(offsets(cx).0.x, px(0.), "clamped at the left edge");

    // Another file opens in its own tab at the top-left, and this one keeps its scroll position.
    wheel(cx, over_diff, -300., -200.);
    let scrolled = offsets(cx);
    view.update(cx, |m, cx| m.select_file(0, false, cx));
    cx.run_until_parked();
    let (side, list) = offsets(cx);
    assert_eq!((side.x, list.y), (px(0.), px(0.)), "a new tab starts at the top-left");
    view.update_in(cx, |m, window, cx| m.activate_tab(0, Some(window), cx));
    cx.run_until_parked();
    assert_eq!(offsets(cx), scrolled, "switching back restores the earlier position");
}

#[gpui::test]
fn appearance_cycles_persists_and_resolves_light_or_dark(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join("maditor-native-test-appearance");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(None, config, cx));
    // (The palette itself is a process-wide switch that parallel tests also flip on render,
    // so assert on what this view resolves rather than on the global.)
    let resolves_dark = |cx: &mut gpui::VisualTestContext| view.update_in(cx, |m, window, _| m.resolve_dark(window));

    assert_eq!(view.read_with(cx, |m, _| m.config.appearance), Appearance::System);
    view.update(cx, |m, cx| m.cycle_appearance(cx));
    assert_eq!(view.read_with(cx, |m, _| m.config.appearance), Appearance::Light);
    assert!(!resolves_dark(cx), "Light forces light regardless of the OS");
    view.update(cx, |m, cx| m.cycle_appearance(cx));
    assert!(resolves_dark(cx), "Dark forces dark regardless of the OS");
    view.update(cx, |m, cx| m.cycle_appearance(cx));
    assert_eq!(view.read_with(cx, |m, _| m.config.appearance), Appearance::System);

    // The choice was written to disk.
    view.update(cx, |m, cx| m.cycle_appearance(cx));
    assert_eq!(config_in(&root).appearance, Appearance::Light);
}

#[gpui::test]
fn narrow_panes_switch_the_diff_to_unified(cx: &mut TestAppContext) {
    // The threshold is what decides the layout; the pane math feeds it the available width.
    assert!(maditor_ui::diff_view::SPLIT_MIN_WIDTH > 500.);
    let (root, repo) = fixture("responsive");
    let path = repo.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
    cx.run_until_parked();
    let wt_ix = view.read_with(cx, |m, _| m.worktrees.iter().position(|w| !w.is_main).unwrap());
    view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
    cx.run_until_parked();
    view.update(cx, |m, cx| m.select_file(0, true, cx));
    view.read_with(cx, |m, _| {
        // a.txt changes one line: 2 split rows (equal + replaced) vs 3 unified lines.
        let data = &active_diff(m).unwrap().data;
        let (split, unified) = (data.len(false), data.len(true));
        assert!(unified > split, "unified={unified} split={split}");
    });
}

fn project_with_files(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("maditor-native-test-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    let proj = root.join("proj");
    std::fs::create_dir_all(proj.join("src/nested")).unwrap();
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    std::fs::write(proj.join("README.md"), "hello\n").unwrap();
    std::fs::write(proj.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(proj.join("src/nested/deep.txt"), "deep\n").unwrap();
    std::fs::write(proj.join("zeta.bin"), [0u8, 159, 146, 150]).unwrap();
    (root, proj)
}

#[gpui::test]
fn edit_mode_opens_edits_and_saves_a_file_and_guards_unsaved_closes(cx: &mut TestAppContext) {
    cx.update(|cx| maditor_editor::bind_keys(cx));
    let (root, proj) = project_with_files("edit");
    let path = proj.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
    cx.run_until_parked();

    // A folder without .git is editor-only: it lands in Edit mode with its tree loaded.
    let (root2, plain) = project_with_files("edit-plain");
    std::fs::remove_dir_all(plain.join(".git")).unwrap();
    let plain_path = plain.to_string_lossy().into_owned();
    view.update(cx, |m, cx| m.open_project(plain_path.clone(), cx));
    cx.run_until_parked();
    view.read_with(cx, |m, _| {
        assert_eq!(m.issue, Some(Issue::NotARepo));
        assert!(m.sidebar_view == SidebarView::Files, "no git means the files view");
        assert_eq!(m.tree_rows.len(), 3);
    });
    let _ = root2;

    // Open a file, type into it, then save with the editor's own shortcut.
    let readme = plain.join("README.md");
    view.update_in(cx, |m, window, cx| m.open_file(readme.clone(), false, window, cx));
    cx.simulate_input("X");
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    view.read_with(cx, |m, cx| assert_eq!(m.dirty_tab_count(&plain_path, cx), 1));

    // Closing the dirty tab asks first; declining keeps it.
    view.update(cx, |m, cx| m.request_close_tab(0, cx));
    view.read_with(cx, |m, _| assert!(m.confirm.is_some(), "unsaved changes need a confirmation"));
    view.update_in(cx, |m, _, _| m.confirm = None);
    assert_eq!(view.read_with(cx, |m, _| m.workspaces[&plain_path].tabs.len()), 1);

    cx.simulate_keystrokes(&format!("{mod_key}-s"));
    assert_eq!(std::fs::read_to_string(&readme).unwrap(), "Xhello\n");
    view.read_with(cx, |m, cx| assert_eq!(m.dirty_tab_count(&plain_path, cx), 0));

    // Saved, so closing needs no confirmation.
    view.update(cx, |m, cx| m.request_close_tab(0, cx));
    view.read_with(cx, |m, _| {
        assert!(m.confirm.is_none());
        assert!(m.workspaces[&plain_path].tabs.is_empty());
    });
}

#[gpui::test]
fn preview_tabs_are_replaced_until_pinned_by_editing_or_reopening(cx: &mut TestAppContext) {
    cx.update(|cx| maditor_editor::bind_keys(cx));
    let (root, proj) = project_with_files("preview");
    let path = proj.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
    cx.run_until_parked();
    let tabs = |cx: &mut gpui::VisualTestContext| {
        view.read_with(cx, |m, _| {
            m.workspaces[&path].tabs.iter().map(|t| (t.title.clone(), t.preview)).collect::<Vec<_>>()
        })
    };

    view.update_in(cx, |m, window, cx| m.open_file(proj.join("README.md"), true, window, cx));
    view.update_in(cx, |m, window, cx| m.open_file(proj.join("src/main.rs"), true, window, cx));
    assert_eq!(tabs(cx), [("main.rs".to_string(), true)], "a second preview replaces the first");

    // Typing into it pins the tab, so the next preview opens beside it instead of replacing it.
    cx.simulate_input("x");
    assert_eq!(tabs(cx), [("main.rs".to_string(), false)]);
    view.update_in(cx, |m, window, cx| m.open_file(proj.join("README.md"), true, window, cx));
    assert_eq!(tabs(cx), [("main.rs".to_string(), false), ("README.md".to_string(), true)]);

    // Opening the same file for real pins its tab rather than duplicating it.
    view.update_in(cx, |m, window, cx| m.open_file(proj.join("README.md"), false, window, cx));
    assert_eq!(tabs(cx), [("main.rs".to_string(), false), ("README.md".to_string(), false)]);
    assert_eq!(view.read_with(cx, |m, _| m.workspaces[&path].active), Some(1));

    // Closing the active tab moves to its neighbour.
    view.update(cx, |m, cx| m.close_tab(1, cx));
    assert_eq!(view.read_with(cx, |m, _| m.workspaces[&path].active), Some(0));
}

#[gpui::test]
fn opening_a_binary_file_reports_it_instead_of_a_tab(cx: &mut TestAppContext) {
    let (root, proj) = project_with_files("binary");
    let path = proj.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
    cx.run_until_parked();
    view.update_in(cx, |m, window, cx| m.open_file(proj.join("zeta.bin"), false, window, cx));
    view.read_with(cx, |m, _| {
        assert!(m.error.as_deref().unwrap_or("").contains("not a UTF-8 text file"), "{:?}", m.error);
        assert!(m.workspaces.get(&path).map(|w| w.tabs.is_empty()).unwrap_or(true));
    });
}

#[gpui::test]
fn closing_a_project_with_unsaved_edits_asks_first(cx: &mut TestAppContext) {
    cx.update(|cx| maditor_editor::bind_keys(cx));
    let (root, proj) = project_with_files("close-dirty");
    std::fs::remove_dir_all(proj.join(".git")).unwrap();
    let path = proj.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
    cx.run_until_parked();
    view.update_in(cx, |m, window, cx| m.open_file(proj.join("README.md"), false, window, cx));
    cx.simulate_input("!");

    view.update(cx, |m, cx| m.request_close_project(&path, cx));
    view.read_with(cx, |m, _| {
        assert!(m.confirm.is_some());
        assert!(m.config.projects.contains(&path), "still open until confirmed");
    });
    view.update(cx, |m, cx| m.run_confirmed(ConfirmAction::CloseProject(path.clone()), cx));
    view.read_with(cx, |m, _| {
        assert!(!m.config.projects.contains(&path));
        assert!(!m.workspaces.contains_key(&path), "its editors are dropped");
    });
}

#[gpui::test]
fn non_git_folder_reports_why_and_can_be_closed(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join("maditor-native-test-plain");
    let _ = std::fs::remove_dir_all(&root);
    let plain = root.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let path = plain.to_string_lossy().into_owned();
    let config = config_in(&root);
    let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
    cx.run_until_parked();

    view.read_with(cx, |m, _| {
        assert_eq!(m.issue, Some(Issue::NotARepo));
        assert!(m.worktrees.is_empty());
    });

    view.update(cx, |m, cx| m.close_project(&path, cx));
    view.read_with(cx, |m, _| {
        assert!(m.config.projects.is_empty());
        assert!(m.repo.is_empty());
        assert_eq!(m.config.last_project, None);
    });
    // ...and the closure was persisted.
    assert!(config_in(&root).projects.is_empty());
}
