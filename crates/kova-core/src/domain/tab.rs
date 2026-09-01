use super::history::NavigationHistory;
use super::location::Location;
use super::selection::SelectionState;
use super::sort::SortDescriptor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

/// State for a single tab. Each tab owns its own location, history, selection,
/// and sort state. The tab model is UI-agnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabState {
    pub id: TabId,
    pub label: String,
    pub history: NavigationHistory,
    pub selection: SelectionState,
    pub sort: SortDescriptor,
}

impl TabState {
    pub fn new(id: TabId, label: String, initial: Location) -> Self {
        Self {
            id,
            label,
            history: NavigationHistory::new(Some(initial)),
            selection: SelectionState::empty(),
            sort: SortDescriptor::by_name(),
        }
    }

    pub fn current_location(&self) -> Option<&Location> {
        self.history.current()
    }

    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }
}

/// Container managing all tabs and the active tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabCollection {
    tabs: Vec<TabState>,
    active: TabId,
    next_id: u64,
}

impl TabCollection {
    pub fn new(initial: Location) -> Self {
        let first = TabId(1);
        let tab = TabState::new(first, tab_label_from_location(&initial), initial);
        Self {
            tabs: vec![tab],
            active: first,
            next_id: 2,
        }
    }

    pub fn active_id(&self) -> TabId {
        self.active
    }

    pub fn active(&self) -> Option<&TabState> {
        self.tabs.iter().find(|t| t.id == self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut TabState> {
        let id = self.active;
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn tabs(&self) -> &[TabState] {
        &self.tabs
    }

    pub fn tabs_mut(&mut self) -> &mut [TabState] {
        &mut self.tabs
    }

    pub fn get(&self, id: TabId) -> Option<&TabState> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut TabState> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn create(&mut self, initial: Location) -> TabId {
        let id = TabId(self.next_id);
        self.next_id += 1;
        let tab = TabState::new(id, tab_label_from_location(&initial), initial);
        self.tabs.push(tab);
        self.active = id;
        id
    }

    /// Close a tab. Returns the new active id, or `None` if the last tab was
    /// closed. If the last tab is closed, the caller is responsible for creating
    /// a new fallback tab or exiting.
    pub fn close(&mut self, id: TabId) -> Option<TabId> {
        if self.tabs.len() <= 1 {
            return None;
        }

        let idx = self.tabs.iter().position(|t| t.id == id)?;
        self.tabs.remove(idx);

        if self.active == id {
            let new_active = self
                .tabs
                .get(idx.min(self.tabs.len() - 1))
                .or_else(|| self.tabs.last())
                .map(|t| t.id)
                .unwrap_or(TabId(0));
            self.active = new_active;
        }

        Some(self.active)
    }

    pub fn switch_to(&mut self, id: TabId) -> bool {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active = id;
            true
        } else {
            false
        }
    }
}

fn tab_label_from_location(location: &Location) -> String {
    location
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| location.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn loc(s: &str) -> Location {
        Location::new(PathBuf::from(s))
    }

    #[test]
    fn create_switch_close_tabs_keep_independent_history() {
        let mut tabs = TabCollection::new(loc("A"));
        let first_id = tabs.active_id();
        tabs.active_mut().unwrap().history.navigate(loc("B"));

        let second_id = tabs.create(loc("X"));
        assert_eq!(tabs.active_id(), second_id);
        assert_eq!(tabs.tabs().len(), 2);

        tabs.switch_to(first_id);
        assert_eq!(
            tabs.active()
                .unwrap()
                .current_location()
                .unwrap()
                .path
                .to_str()
                .unwrap(),
            "B"
        );

        tabs.switch_to(second_id);
        assert_eq!(
            tabs.active()
                .unwrap()
                .current_location()
                .unwrap()
                .path
                .to_str()
                .unwrap(),
            "X"
        );

        tabs.close(second_id);
        assert_eq!(tabs.active_id(), first_id);
        assert_eq!(tabs.tabs().len(), 1);
    }

    #[test]
    fn close_last_tab_returns_none() {
        let mut tabs = TabCollection::new(loc("A"));
        let id = tabs.active_id();
        assert!(tabs.close(id).is_none());
    }
}
