//! The tabs of the window, each with its own tree of panes: "all" first, then one per
//! project. Hidden tabs keep their sessions attached; pane ids are unique across tabs.

use std::path::Path;

use serde::Serialize;

use crate::layout::{Ids, Layout, PaneId, SessionKey, Shown};
use crate::projects::{self, Project};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tab {
    id: PaneId,
    layout: Layout,
    /// None for "all", whose bar shows every session.
    project: Option<Project>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: PaneId,
    #[serde(skip)]
    ids: Ids,
}

impl Default for Tabs {
    fn default() -> Self {
        let mut ids = Ids::default();
        let all = Tab { id: ids.take(), layout: Layout::new(&mut ids), project: None };
        Self { active: all.id, tabs: vec![all], ids }
    }
}

impl Tabs {
    /// "all", then a tab per project, "all" active.
    pub fn with_projects(projects: Vec<Project>) -> Self {
        let mut tabs = Self::default();
        for project in projects {
            tabs.push(project);
        }
        tabs.active = tabs.tabs[0].id;
        tabs
    }

    fn push(&mut self, project: Project) {
        let tab = Tab { id: self.ids.take(), layout: Layout::new(&mut self.ids), project: Some(project) };
        self.active = tab.id;
        self.tabs.push(tab);
    }

    fn index(&self) -> usize {
        self.tabs.iter().position(|tab| tab.id == self.active).unwrap_or(0)
    }

    #[cfg(test)]
    pub fn active_tab(&self) -> PaneId {
        self.active
    }

    /// The tree of the active tab, which the pane commands act on.
    pub fn current_mut(&mut self) -> &mut Layout {
        let index = self.index();
        &mut self.tabs[index].layout
    }

    /// The session being looked at: the one in the active pane of the visible tab.
    pub fn watched(&self) -> Option<&SessionKey> {
        self.tabs[self.index()].layout.active_session()
    }

    /// Splits the active pane of the active tab.
    pub fn split(&mut self, width: f64, height: f64) {
        let index = self.index();
        self.tabs[index].layout.split(&mut self.ids, width, height);
    }

    /// A tab for `project`, with one empty pane, after the others so their numbers do not
    /// change. A folder is one project only.
    pub fn add_project(&mut self, project: Project) -> Result<(), String> {
        if self.projects().iter().any(|known| known.directory == project.directory) {
            return Err(format!("{} is already a project", project.directory.display()));
        }
        self.push(project);
        Ok(())
    }

    /// The projects, in tab order: what `projects.json` keeps.
    pub fn projects(&self) -> Vec<Project> {
        self.tabs.iter().filter_map(|tab| tab.project.clone()).collect()
    }

    /// The tab a session working in `directory` belongs to: its project's, else "all".
    pub fn owner_tab(&self, directory: &Path) -> PaneId {
        let projects = self.tabs.iter().filter_map(|tab| tab.project.as_ref().map(|project| (tab.id, project)));
        projects::owner(directory, projects).unwrap_or(self.tabs[0].id)
    }

    /// Closes the active project tab and gives the focus to the one before it. "all" stays:
    /// returns the panes to detach, or None.
    pub fn close_tab(&mut self) -> Option<Vec<PaneId>> {
        let index = self.index();
        self.tabs[index].project.as_ref()?;
        let closed = self.tabs.remove(index);
        self.active = self.tabs[index.saturating_sub(1)].id;
        Some(closed.layout.leaves())
    }

    /// Moves to the tab `step` places forward (or back if negative), wrapping around.
    pub fn step(&mut self, step: i64) {
        let length = self.tabs.len() as i64;
        self.active = self.tabs[(self.index() as i64 + step).rem_euclid(length) as usize].id;
    }

    /// Moves to tab `number`, counted from 1; nothing happens if there is no such tab.
    pub fn go(&mut self, number: usize) {
        if let Some(tab) = number.checked_sub(1).and_then(|index| self.tabs.get(index)) {
            self.active = tab.id;
        }
    }

    /// Like `Layout::show`, across tabs: a session shown in another tab brings that tab and
    /// its pane to the front instead of being attached twice.
    pub fn show(&mut self, pane: PaneId, session: SessionKey) -> Option<Shown> {
        for tab in &mut self.tabs {
            if let Some(shown_in) = tab.layout.shown_in(&session) {
                tab.layout.focus(shown_in);
                self.active = tab.id;
                return Some(Shown::Already(shown_in));
            }
        }
        self.current_mut().show(pane, session)
    }

    /// The pane, in whichever tab, no longer shows a session.
    pub fn clear(&mut self, pane: PaneId) {
        for tab in &mut self.tabs {
            tab.layout.clear(pane);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(short_id: &str) -> SessionKey {
        SessionKey { account: "lystriq".into(), short_id: short_id.into() }
    }

    /// A project tab for a new folder.
    fn add(tabs: &mut Tabs) {
        let folder = format!("/home/u/project{}", tabs.tabs.len());
        tabs.add_project(Project::new(folder.into())).unwrap();
    }

    fn active_tab(tabs: &Tabs) -> &Tab {
        &tabs.tabs[tabs.index()]
    }

    fn panes(tabs: &Tabs) -> Vec<PaneId> {
        tabs.tabs.iter().flat_map(|tab| tab.layout.leaves()).collect()
    }

    #[test]
    fn a_new_tab_comes_last_with_one_empty_active_pane() {
        let mut tabs = Tabs::default();
        add(&mut tabs);
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.index(), 1);
        let layout = &active_tab(&tabs).layout;
        assert_eq!(layout.leaves().len(), 1);
        assert_eq!(layout.leaves(), [layout.active()]);
        assert_eq!(layout.shown_in(&key("aaa")), None);
    }

    #[test]
    fn pane_ids_are_unique_across_tabs() {
        let mut tabs = Tabs::default();
        tabs.split(1000.0, 500.0);
        add(&mut tabs);
        tabs.split(1000.0, 500.0);
        add(&mut tabs);
        let mut all = panes(&tabs);
        assert_eq!(all.len(), 5);
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn the_all_tab_cannot_be_closed() {
        let mut tabs = Tabs::default();
        assert_eq!(tabs.close_tab(), None);
        add(&mut tabs);
        tabs.go(1);
        assert_eq!(tabs.close_tab(), None);
        assert_eq!(tabs.tabs.len(), 2);
    }

    #[test]
    fn closing_a_tab_returns_its_panes_and_focuses_the_previous_tab() {
        let mut tabs = Tabs::default();
        add(&mut tabs);
        tabs.split(1000.0, 500.0);
        let middle = panes(&tabs)[1..].to_vec();
        add(&mut tabs);
        tabs.go(2);
        assert_eq!(tabs.close_tab(), Some(middle));
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.index(), 0);
    }

    #[test]
    fn closing_a_project_tab_forgets_the_project() {
        let mut tabs = Tabs::default();
        add(&mut tabs);
        add(&mut tabs);
        tabs.go(2);
        assert!(tabs.close_tab().is_some());
        assert_eq!(tabs.projects(), [Project::new("/home/u/project2".into())]);
    }

    #[test]
    fn projects_come_back_after_all_in_order_with_all_active() {
        let projects = vec![Project::new("/home/u/blog".into()), Project::new("/home/u/api".into())];
        let tabs = Tabs::with_projects(projects.clone());
        assert_eq!(tabs.tabs[0].project, None);
        assert_eq!(tabs.projects(), projects);
        assert_eq!(tabs.index(), 0);
    }

    #[test]
    fn a_folder_is_one_project_only() {
        let mut tabs = Tabs::default();
        tabs.add_project(Project::new("/home/u/api".into())).unwrap();
        assert!(tabs.add_project(Project::new("/home/u/api".into())).is_err());
        assert_eq!(tabs.projects().len(), 1);
    }

    #[test]
    fn a_session_belongs_to_its_project_tab_else_to_all() {
        let mut tabs = Tabs::default();
        let all = tabs.active;
        tabs.add_project(Project::new("/home/u/api".into())).unwrap();
        let api = tabs.active;
        assert_eq!(tabs.owner_tab(Path::new("/home/u/api/src")), api);
        assert_eq!(tabs.owner_tab(Path::new("/home/u")), all);
    }

    #[test]
    fn steps_wrap_around() {
        let mut tabs = Tabs::default();
        add(&mut tabs);
        add(&mut tabs);
        assert_eq!(tabs.index(), 2);
        tabs.step(1);
        assert_eq!(tabs.index(), 0);
        tabs.step(-1);
        assert_eq!(tabs.index(), 2);
        tabs.step(-1);
        assert_eq!(tabs.index(), 1);
    }

    #[test]
    fn go_ignores_missing_numbers() {
        let mut tabs = Tabs::default();
        add(&mut tabs);
        tabs.go(1);
        assert_eq!(tabs.index(), 0);
        tabs.go(0);
        tabs.go(3);
        assert_eq!(tabs.index(), 0);
        tabs.go(2);
        assert_eq!(tabs.index(), 1);
    }

    #[test]
    fn a_session_shown_in_another_tab_brings_that_tab_and_pane_to_the_front() {
        let mut tabs = Tabs::default();
        tabs.split(1000.0, 500.0);
        let first_tab = tabs.active;
        let shown = active_tab(&tabs).layout.leaves()[0];
        tabs.show(shown, key("aaa"));
        tabs.current_mut().focus_step(1);
        add(&mut tabs);
        let pane = active_tab(&tabs).layout.active();
        assert_eq!(tabs.show(pane, key("aaa")), Some(Shown::Already(shown)));
        assert_eq!(tabs.active, first_tab);
        assert_eq!(active_tab(&tabs).layout.active(), shown);
        assert_eq!(tabs.show(pane, key("bbb")), None, "pane is in the other tab");
    }

    #[test]
    fn a_pane_in_a_hidden_tab_can_be_cleared() {
        let mut tabs = Tabs::default();
        let pane = active_tab(&tabs).layout.active();
        tabs.show(pane, key("aaa"));
        add(&mut tabs);
        tabs.clear(pane);
        let other = active_tab(&tabs).layout.active();
        assert_eq!(tabs.show(other, key("aaa")), Some(Shown::Attach));
    }

    #[test]
    fn the_page_gets_the_tabs_and_the_active_one() {
        let mut tabs = Tabs::default();
        add(&mut tabs);
        let json = serde_json::to_value(&tabs).unwrap_or_default();
        assert_eq!(json["tabs"].as_array().map(Vec::len), Some(2));
        assert_eq!(json["active"], json["tabs"][1]["id"]);
        assert_eq!(json["tabs"][1]["layout"]["root"]["kind"], "leaf");
        assert!(json.get("ids").is_none());
    }

    #[test]
    fn the_watched_session_is_the_one_of_the_visible_tab() {
        let mut tabs = Tabs::default();
        let pane = active_tab(&tabs).layout.active();
        tabs.show(pane, key("aaa"));
        assert_eq!(tabs.watched(), Some(&key("aaa")));
        add(&mut tabs);
        assert_eq!(tabs.watched(), None);
        tabs.go(1);
        assert_eq!(tabs.watched(), Some(&key("aaa")));
    }
}
