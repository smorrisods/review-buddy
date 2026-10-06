//! The project filter: which projects (GitHub `owner/repo`, GitLab project paths) the queue
//! shows, as a model the dashboard, the Show control and the commands all read.
//!
//! The filter stores what is **hidden**, not what is shown. Everything is shown by default, and a
//! project that first appears after a refresh stays shown until you hide it yourself, whether or
//! not you had narrowed the list before. Entries are exact project paths, or a path prefix
//! ending in `*` (`owner/legacy-*`), matched without regard to case. A project covered by a
//! pattern can still be switched back on: it is remembered as an exception to that pattern.

use std::collections::BTreeSet;

use rb_core::{ChangeState, ChangeSummary, SourceId};

use super::AppState;
use crate::config::{pattern_matches, Config};

/// A project on one source.
pub type Project = (SourceId, String);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectFilter {
    hidden: Vec<Project>,
    shown: BTreeSet<Project>,
}

impl ProjectFilter {
    /// The `hide_repos` of every `[[source]]`.
    pub fn from_config(config: &Config) -> Self {
        let hidden = config
            .sources
            .iter()
            .flat_map(|s| {
                s.hide_repos
                    .iter()
                    .map(|p| (SourceId::new(&s.name), p.clone()))
            })
            .collect();
        Self {
            hidden,
            shown: BTreeSet::new(),
        }
    }

    /// Nothing is hidden.
    pub fn is_clear(&self) -> bool {
        self.hidden.is_empty()
    }

    pub fn is_hidden(&self, source: &SourceId, repo: &str) -> bool {
        !self
            .shown
            .iter()
            .any(|(s, r)| s == source && r.eq_ignore_ascii_case(repo))
            && self
                .hidden
                .iter()
                .any(|(s, pattern)| s == source && pattern_matches(pattern, repo))
    }

    pub fn hides(&self, change: &ChangeSummary) -> bool {
        self.is_hidden(&change.id.source_id, &change.id.repo)
    }

    pub fn hide(&mut self, source: &SourceId, repo: &str) {
        self.shown
            .retain(|(s, r)| !(s == source && r.eq_ignore_ascii_case(repo)));
        if !self.is_hidden(source, repo) {
            self.hidden.push((source.clone(), repo.to_string()));
        }
    }

    pub fn show(&mut self, source: &SourceId, repo: &str) {
        self.hidden
            .retain(|(s, p)| !(s == source && p.eq_ignore_ascii_case(repo)));
        if self.is_hidden(source, repo) {
            self.shown.insert((source.clone(), repo.to_string()));
        }
    }

    pub fn toggle(&mut self, source: &SourceId, repo: &str) {
        if self.is_hidden(source, repo) {
            self.show(source, repo);
        } else {
            self.hide(source, repo);
        }
    }

    pub fn set_all(&mut self, projects: &[Project], shown: bool) {
        for (source, repo) in projects {
            if shown {
                self.show(source, repo);
            } else {
                self.hide(source, repo);
            }
        }
    }

    /// The `hide_repos` entries for `source`, ready to write. A pattern with exceptions is
    /// written out as the projects it still hides, so the file says what the queue does.
    pub fn entries_for(&self, source: &SourceId, known: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (s, pattern) in self.hidden.iter().filter(|(s, _)| s == source) {
            let excepted = self
                .shown
                .iter()
                .any(|(es, repo)| es == s && pattern_matches(pattern, repo));
            let entries: Vec<String> = if excepted {
                known
                    .iter()
                    .filter(|repo| pattern_matches(pattern, repo) && self.is_hidden(s, repo))
                    .cloned()
                    .collect()
            } else {
                vec![pattern.clone()]
            };
            for entry in entries {
                if !out.iter().any(|o| o.eq_ignore_ascii_case(&entry)) {
                    out.push(entry);
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub repo: String,
    /// Open changes in the project, whatever the filters say.
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProjects {
    pub source: SourceId,
    pub label: String,
    pub projects: Vec<ProjectRow>,
}

/// Every project that has an open change in `state`, grouped by source in config order, each
/// group sorted by name. Loaded and cached changes both live in `state.changes`.
pub fn catalog(state: &AppState) -> Vec<SourceProjects> {
    let mut groups: Vec<SourceProjects> = Vec::new();
    for change in state
        .changes
        .iter()
        .filter(|c| c.state == ChangeState::Open)
    {
        let at = match groups.iter().position(|g| g.source == change.id.source_id) {
            Some(at) => at,
            None => {
                let label = state
                    .sources
                    .iter()
                    .find(|s| s.id == change.id.source_id)
                    .map_or_else(|| change.id.source_id.to_string(), |s| s.label.clone());
                groups.push(SourceProjects {
                    source: change.id.source_id.clone(),
                    label,
                    projects: Vec::new(),
                });
                groups.len() - 1
            }
        };
        let projects = &mut groups[at].projects;
        match projects.iter_mut().find(|p| p.repo == change.id.repo) {
            Some(row) => row.count += 1,
            None => projects.push(ProjectRow {
                repo: change.id.repo.clone(),
                count: 1,
            }),
        }
    }
    for group in &mut groups {
        group
            .projects
            .sort_by_key(|p| (p.repo.to_lowercase(), p.repo.clone()));
    }
    let order = |id: &SourceId| {
        state
            .sources
            .iter()
            .position(|s| &s.id == id)
            .unwrap_or(usize::MAX)
    };
    groups.sort_by_key(|g| order(&g.source));
    groups
}

/// `(shown, total)` across the catalog.
pub fn tally(state: &AppState, groups: &[SourceProjects]) -> (usize, usize) {
    let filter = &state.queue_settings.projects;
    let total = groups.iter().map(|g| g.projects.len()).sum();
    let shown = groups
        .iter()
        .flat_map(|g| g.projects.iter().map(move |p| (&g.source, p)))
        .filter(|(source, p)| !filter.is_hidden(source, &p.repo))
        .count();
    (shown, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::queue::tests::change;
    use rb_core::MyRole;

    fn id(name: &str) -> SourceId {
        SourceId::new(name)
    }

    fn filter(entries: &[(&str, &str)]) -> ProjectFilter {
        ProjectFilter {
            hidden: entries
                .iter()
                .map(|(s, r)| (id(s), r.to_string()))
                .collect(),
            shown: BTreeSet::new(),
        }
    }

    #[test]
    fn everything_is_shown_until_something_is_hidden() {
        let mut f = ProjectFilter::default();
        assert!(f.is_clear() && !f.is_hidden(&id("a"), "o/r"));
        f.hide(&id("a"), "o/r");
        assert!(f.is_hidden(&id("a"), "o/r"));
        assert!(!f.is_clear());
        f.show(&id("a"), "o/r");
        assert!(f.is_clear());
    }

    #[test]
    fn hiding_is_per_source_and_ignores_case() {
        let f = filter(&[("a", "Acme/Widgets")]);
        assert!(f.is_hidden(&id("a"), "acme/widgets"));
        assert!(!f.is_hidden(&id("b"), "acme/widgets"));
    }

    #[test]
    fn projects_that_appear_later_are_shown_even_after_narrowing() {
        let mut f = ProjectFilter::default();
        f.hide(&id("a"), "o/old");
        assert!(!f.is_hidden(&id("a"), "o/brand-new"));
    }

    #[test]
    fn globs_hide_by_prefix_and_can_have_exceptions() {
        let mut f = filter(&[("a", "o/legacy-*")]);
        assert!(f.is_hidden(&id("a"), "o/legacy-api"));
        assert!(!f.is_hidden(&id("a"), "o/modern"));
        f.show(&id("a"), "o/legacy-api");
        assert!(!f.is_hidden(&id("a"), "o/legacy-api"));
        assert!(f.is_hidden(&id("a"), "o/legacy-web"));
        f.hide(&id("a"), "o/legacy-api");
        assert!(f.is_hidden(&id("a"), "o/legacy-api"));
        assert_eq!(
            f.hidden.len(),
            1,
            "no duplicate exact entry for a covered project"
        );
    }

    #[test]
    fn entries_for_expands_a_glob_that_has_exceptions() {
        let mut f = filter(&[("a", "o/legacy-*"), ("a", "o/old"), ("b", "x/y")]);
        let known: Vec<String> = ["o/legacy-api", "o/legacy-web", "o/old", "o/new"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            f.entries_for(&id("a"), &known),
            ["o/legacy-*", "o/old"],
            "untouched patterns are kept as written"
        );
        f.show(&id("a"), "o/legacy-api");
        assert_eq!(f.entries_for(&id("a"), &known), ["o/legacy-web", "o/old"]);
        assert_eq!(f.entries_for(&id("b"), &known), ["x/y"]);
        assert!(f.entries_for(&id("c"), &known).is_empty());
    }

    #[test]
    fn select_all_and_none_touch_only_the_projects_given() {
        let mut f = filter(&[("a", "o/one")]);
        let rows = [
            (id("a"), "o/two".to_string()),
            (id("a"), "o/three".to_string()),
        ];
        f.set_all(&rows, false);
        assert!(f.is_hidden(&id("a"), "o/two") && f.is_hidden(&id("a"), "o/three"));
        assert!(f.is_hidden(&id("a"), "o/one"));
        f.set_all(&rows, true);
        assert!(!f.is_hidden(&id("a"), "o/two"));
        assert!(
            f.is_hidden(&id("a"), "o/one"),
            "outside the rows, left alone"
        );
    }

    #[test]
    fn config_hide_repos_seed_the_filter() {
        let config: Config = toml::from_str(
            "[[source]]\nname = \"w\"\nkind = \"github\"\nhost = \"github.com\"\nhide_repos = [\"o/a\", \"o/b-*\"]\n",
        )
        .unwrap();
        let f = ProjectFilter::from_config(&config);
        assert!(f.is_hidden(&id("w"), "o/a") && f.is_hidden(&id("w"), "o/b-x"));
        assert!(!f.is_hidden(&id("w"), "o/c"));
    }

    fn state_with(repos: &[(&str, &str, u64)]) -> AppState {
        let mut state = AppState::default();
        for (source, repo, n) in repos {
            let mut c = change(*n, MyRole::Reviewing, 10);
            c.id.source_id = id(source);
            c.id.repo = (*repo).to_string();
            state.changes.push(c);
        }
        state
    }

    #[test]
    fn the_catalog_groups_by_source_counts_and_sorts() {
        let state = state_with(&[
            ("s2", "b/zed", 1),
            ("s1", "o/beta", 2),
            ("s1", "o/Alpha", 3),
            ("s1", "o/beta", 4),
        ]);
        let groups = catalog(&state);
        assert_eq!(groups.len(), 2);
        let s1 = groups.iter().find(|g| g.source == id("s1")).unwrap();
        let names: Vec<(&str, usize)> = s1
            .projects
            .iter()
            .map(|p| (p.repo.as_str(), p.count))
            .collect();
        assert_eq!(names, [("o/Alpha", 1), ("o/beta", 2)]);
    }

    #[test]
    fn hidden_projects_stay_listed_and_the_tally_counts_them_out() {
        let mut state = state_with(&[("s1", "o/a", 1), ("s1", "o/b", 2), ("s1", "o/c", 3)]);
        state.queue_settings.projects.hide(&id("s1"), "o/b");
        let groups = catalog(&state);
        assert_eq!(tally(&state, &groups), (2, 3));
    }

    #[test]
    fn closed_changes_do_not_make_a_project() {
        let mut state = state_with(&[("s1", "o/a", 1), ("s1", "o/gone", 2)]);
        state.changes[1].state = ChangeState::Merged;
        let groups = catalog(&state);
        assert_eq!(groups[0].projects.len(), 1);
    }
}
