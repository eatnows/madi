//! The app frame: project rail, top bar, sidebar with the file tree, status bar.
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use gyeol::{div, text, uniform_list, Color, Cx, Element, SystemTheme, View};
use madi_project::{
    config::{Appearance, Config},
    tree::{build_tree_rows, TreeRow},
};

use crate::{icons, theme::Palette};

type El = Element<Madi>;

const ROW_H: f32 = 24.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SidebarView {
    Files,
    Worktrees,
}

/// What is remembered per project so switching projects never loses your place.
#[derive(Default)]
pub struct Workspace {
    pub expanded: HashSet<PathBuf>,
    pub selected: Option<PathBuf>,
}

pub struct Madi {
    pub config: Config,
    pub repo: String,
    pub workspaces: HashMap<String, Workspace>,
    pub tree_rows: Vec<TreeRow>,
    pub sidebar_view: SidebarView,
    pub sidebar_width: f32,
    /// Hides everything but the top bar and the editor area.
    pub focus_mode: bool,
}

impl Madi {
    pub fn new(initial: Option<String>, config: Config) -> Madi {
        let mut app = Madi {
            config,
            repo: String::new(),
            workspaces: HashMap::new(),
            tree_rows: Vec::new(),
            sidebar_view: SidebarView::Files,
            sidebar_width: 280.,
            focus_mode: false,
        };
        if let Some(path) = initial.or_else(|| app.config.last_project.clone()) {
            app.open_project(path);
        }
        app
    }

    pub fn project_name(path: &str) -> String {
        path.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(path).to_string()
    }

    /// Opens (and remembers) a project folder.
    pub fn open_project(&mut self, path: String) {
        if !self.config.projects.contains(&path) {
            self.config.projects.push(path.clone());
        }
        self.config.last_project = Some(path.clone());
        self.config.save();
        self.repo = path;
        self.refresh_tree();
    }

    fn workspace(&self) -> Option<&Workspace> {
        self.workspaces.get(&self.repo)
    }

    fn workspace_mut(&mut self) -> &mut Workspace {
        self.workspaces.entry(self.repo.clone()).or_default()
    }

    pub fn refresh_tree(&mut self) {
        let expanded = self.workspace().map(|w| w.expanded.clone()).unwrap_or_default();
        self.tree_rows = if self.repo.is_empty() { Vec::new() } else { build_tree_rows(Path::new(&self.repo), &expanded) };
    }

    pub fn toggle_dir(&mut self, path: &Path) {
        let ws = self.workspace_mut();
        if !ws.expanded.remove(path) {
            ws.expanded.insert(path.to_path_buf());
        }
        self.refresh_tree();
    }

    pub fn select(&mut self, path: PathBuf) {
        self.workspace_mut().selected = Some(path);
    }

    fn resolve_dark(&self, cx: &Cx) -> bool {
        match self.config.appearance {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::System => cx.system_theme() == SystemTheme::Dark,
        }
    }

    /// The window heading: what is selected (bold) and where it lives (dim).
    pub fn title_parts(&self) -> (String, String) {
        let project = Self::project_name(&self.repo);
        match self.workspace().and_then(|w| w.selected.as_ref()) {
            Some(path) if !path.is_dir() => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let dir = path.strip_prefix(&self.repo).ok().and_then(|p| p.parent()).map(|d| d.display().to_string()).unwrap_or_default();
                (name, if dir.is_empty() { project } else { format!("{project} · {dir}") })
            }
            _ if self.repo.is_empty() => ("Madi".into(), String::new()),
            _ => (project, String::new()),
        }
    }

    // ---- views -------------------------------------------------------------------------------

    fn rail(&self, p: &Palette) -> El {
        let tiles = self.config.projects.iter().map(|path| {
            let active = *path == self.repo;
            let letter = Self::project_name(path).chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            let target = path.clone();
            let tile = div()
                .size(32.)
                .items_center()
                .justify_center()
                .rounded(6.)
                .on_click(move |s: &mut Madi, _| s.open_project(target.clone()))
                .child(text(letter).text_color(if active { p.bg } else { p.text_dim }));
            if active { tile.bg(p.text_strong) } else { tile.border(1., p.border).hover_bg(p.selected) }
        });
        div().w(48.).bg(p.chrome).items_center().py(8.).gap(8.).children(tiles)
    }

    fn icon_button(&self, p: &Palette, active: bool, icon: El, on_click: impl Fn(&mut Madi, &mut Cx) + 'static) -> El {
        div()
            .size(28.)
            .items_center()
            .justify_center()
            .rounded(6.)
            .bg(if active { p.selected } else { Color::TRANSPARENT })
            .hover_bg(p.selected)
            .on_click(on_click)
            .child(icon)
    }

    fn topbar(&self, p: &Palette) -> El {
        let (name, place) = self.title_parts();
        div()
            .col()
            .child(
                div()
                    .row()
                    .items_center()
                    .h(40.)
                    .px(16.)
                    .gap(12.)
                    .bg(p.chrome)
                    .child(text(name).text_color(p.text_strong))
                    .child(text(place).text_size(12.).text_color(p.text_dim))
                    .child(div().grow())
                    .child(self.icon_button(p, self.focus_mode, icons::focus(if self.focus_mode { p.text_strong } else { p.text_dim }), |s, _| {
                        s.focus_mode = !s.focus_mode
                    })),
            )
            .child(div().h(1.).bg(p.border))
    }

    fn tree_row(&self, i: usize, p: &Palette) -> El {
        let row = &self.tree_rows[i];
        let selected = self.workspace().and_then(|w| w.selected.as_ref()) == Some(&row.path);
        let (path, is_dir) = (row.path.clone(), row.is_dir);
        let arrow = if !row.is_dir { "" } else if row.expanded { "▾" } else { "▸" };
        div()
            .row()
            .items_center()
            .px(8.)
            .bg(if selected { p.selected } else { Color::TRANSPARENT })
            .hover_bg(p.selected)
            .on_click(move |s: &mut Madi, _| {
                s.select(path.clone());
                if is_dir {
                    s.toggle_dir(&path);
                }
            })
            .child(div().w(row.depth as f32 * 14.))
            .child(div().w(14.).child(text(arrow).text_size(10.).text_color(p.text_dim)))
            .child(text(row.name.clone()).text_size(12.).text_color(if row.is_dir { p.text_strong } else { p.text }))
    }

    fn sidebar(&self, cx: &Cx, p: &Palette) -> El {
        let seg = |label: &str, view: SidebarView| {
            let active = self.sidebar_view == view;
            div()
                .px(8.)
                .py(4.)
                .rounded(6.)
                .bg(if active { p.selected } else { Color::TRANSPARENT })
                .on_click(move |s: &mut Madi, _| s.sidebar_view = view)
                .child(text(label).text_size(12.).text_color(if active { p.text_strong } else { p.text_dim }))
        };
        let body = match self.sidebar_view {
            SidebarView::Files => uniform_list(cx, ("tree", &self.repo), self.tree_rows.len(), ROW_H, |i| self.tree_row(i, p))
                .grow()
                .p(8.)
                .scrollbar(p.text_dim.with_alpha(0.4)),
            SidebarView::Worktrees => div().grow().p(16.).child(text("Worktrees move over in a later step.").text_size(12.).text_color(p.text_dim)),
        };
        div()
            .row()
            .w(self.sidebar_width)
            .child(
                div()
                    .grow()
                    .bg(p.panel)
                    .child(
                        div()
                            .px(12.)
                            .pt(12.)
                            .pb(8.)
                            .gap(8.)
                            .child(div().px(4.).child(text(Self::project_name(&self.repo)).text_color(p.text_strong)))
                            .child(div().row().gap(4.).child(seg("Files", SidebarView::Files)).child(seg("Worktrees", SidebarView::Worktrees))),
                    )
                    .child(div().h(1.).bg(p.border_soft))
                    .child(body),
            )
            .child(div().w(1.).bg(p.border))
    }

    fn main_area(&self, p: &Palette) -> El {
        div()
            .grow()
            .items_center()
            .justify_center()
            .gap(8.)
            .child(text("Nothing open").text_size(15.).text_color(p.text))
            .child(text("Pick a file in Files to edit it").text_size(12.).text_color(p.text_dim))
    }

    fn status_bar(&self, p: &Palette) -> El {
        div()
            .col()
            .child(div().h(1.).bg(p.border))
            .child(div().row().items_center().h(27.).px(12.).bg(p.chrome).child(text(self.repo.clone()).text_size(12.).text_color(p.text_dim)))
    }
}

impl View for Madi {
    fn view(&self, cx: &mut Cx) -> El {
        let p = Palette::new(self.resolve_dark(cx));
        let mut middle = div().row().grow();
        if !self.focus_mode {
            middle = middle.child(self.rail(&p)).child(div().w(1.).bg(p.border)).child(self.sidebar(cx, &p));
        }
        let mut root = div().bg(p.bg).text_color(p.text).text_size(13.).child(self.topbar(&p)).child(middle.child(self.main_area(&p)));
        if !self.focus_mode {
            root = root.child(self.status_bar(&p));
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyeol::testing::TestHost;

    /// A project with `src/main.rs`, `src/lib.rs` and `README.md` in a fresh temp folder.
    fn project(name: &str) -> (PathBuf, String) {
        let root = std::env::temp_dir().join(format!("madi-gyeol-test-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let proj = root.join("proj");
        std::fs::create_dir_all(proj.join("src")).unwrap();
        std::fs::write(proj.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(proj.join("src/lib.rs"), "\n").unwrap();
        std::fs::write(proj.join("README.md"), "# hi\n").unwrap();
        let path = proj.to_string_lossy().into_owned();
        (root, path)
    }

    fn app(name: &str) -> (TestHost<Madi>, PathBuf, String) {
        let (root, path) = project(name);
        let config = Config::at(Some(root.join("config.json")));
        (TestHost::new(Madi::new(Some(path.clone()), config), (1200., 800.)), root, path)
    }

    fn has_text(host: &TestHost<Madi>, content: &str) -> bool {
        host.scene().texts().any(|t| t.content == content)
    }

    #[test]
    fn the_sidebar_lists_folders_first_and_folders_expand_on_click() {
        let (mut host, _, _) = app("tree");
        let names: Vec<&str> = host.scene().texts().map(|t| t.content.as_str()).collect();
        let (src, readme) = (names.iter().position(|n| *n == "src").unwrap(), names.iter().position(|n| *n == "README.md").unwrap());
        assert!(src < readme, "folders come first");
        assert!(!has_text(&host, "main.rs"), "collapsed");

        host.click_text("src");
        assert!(has_text(&host, "main.rs") && has_text(&host, "lib.rs"), "expanded");
        host.click_text("src");
        assert!(!has_text(&host, "main.rs"), "collapsed again");
    }

    #[test]
    fn selecting_a_file_updates_the_heading() {
        let (mut host, _, path) = app("select");
        assert_eq!(host.state().title_parts(), (Madi::project_name(&path), String::new()));
        host.click_text("src");
        host.click_text("main.rs");
        assert_eq!(host.state().title_parts(), ("main.rs".to_string(), format!("{} · src", Madi::project_name(&path))));
        assert!(has_text(&host, "main.rs"), "the heading and the tree both show it");
    }

    #[test]
    fn the_rail_switches_between_projects_and_remembers_each_ones_folders() {
        let (root, first) = project("rail-a");
        let (_, second) = project("rail-b");
        let config = Config::at(Some(root.join("config.json")));
        let mut app = Madi::new(Some(first.clone()), config);
        app.open_project(second.clone());
        let mut host = TestHost::new(app, (1200., 800.));
        assert_eq!(host.state().repo, second);

        host.click_text("src");
        assert!(has_text(&host, "main.rs"));
        // Two tiles, both labelled "P" (proj): the first one is at the top of the rail.
        host.click((24., 40. + 8. + 16. + 1.));
        assert_eq!(host.state().repo, first);
        assert!(!has_text(&host, "main.rs"), "the first project's tree was never expanded");
        host.click((24., 40. + 8. + 32. + 8. + 16. + 1.));
        assert_eq!(host.state().repo, second);
        assert!(has_text(&host, "main.rs"), "the second project's expansion was remembered");
    }

    #[test]
    fn focus_mode_hides_the_chrome_around_the_editor() {
        let (mut host, _, path) = app("focus");
        let name = Madi::project_name(&path);
        assert!(has_text(&host, &path), "the status bar shows the project path");
        host.state_mut().focus_mode = true;
        host.frame();
        assert!(!has_text(&host, &path), "no status bar");
        assert!(!has_text(&host, "Files"), "no sidebar");
        assert!(has_text(&host, &name), "the top bar stays");
    }

    #[test]
    fn the_theme_follows_the_system_unless_forced() {
        let (mut host, _, _) = app("theme");
        let light = host.scene().quads().next().unwrap().background;
        host.set_system_theme(SystemTheme::Dark);
        let dark = host.scene().quads().next().unwrap().background;
        assert_ne!(light, dark, "System follows the OS");

        host.state_mut().config.appearance = Appearance::Light;
        host.frame();
        assert_eq!(host.scene().quads().next().unwrap().background, light, "Light ignores a dark OS");
    }
}
