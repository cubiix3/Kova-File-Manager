use super::location::Location;
use super::tab::{TabCollection, TabId};

/// Lightweight navigation controller that combines tab state and location
/// navigation. It does not perform I/O; it only mutates tab history and emits
/// the effective target location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationController {
    pub tabs: TabCollection,
}

impl NavigationController {
    pub fn new(initial: Location) -> Self {
        Self {
            tabs: TabCollection::new(initial),
        }
    }

    pub fn active_tab_id(&self) -> TabId {
        self.tabs.active_id()
    }

    /// Navigate the active tab to a new location, clearing forward history.
    pub fn navigate(&mut self, location: Location) {
        if let Some(tab) = self.tabs.active_mut() {
            tab.history.navigate(location);
        }
    }

    pub fn back(&mut self) -> Option<Location> {
        let tab = self.tabs.active_mut()?;
        tab.history.back()
    }

    pub fn forward(&mut self) -> Option<Location> {
        let tab = self.tabs.active_mut()?;
        tab.history.forward()
    }

    pub fn parent(&mut self) -> Option<Location> {
        let tab = self.tabs.active_mut()?;
        let current = tab.history.current()?.clone();
        current.parent()
    }

    pub fn current(&self) -> Option<&Location> {
        self.tabs.active().and_then(|t| t.history.current())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn loc(s: &str) -> Location {
        Location::new(PathBuf::from(s))
    }

    #[test]
    fn navigate_parent_returns_location_without_pushing_history() {
        let mut nav = NavigationController::new(loc(r"C:\a\b\c"));
        let parent = nav.parent().unwrap();
        assert_eq!(parent.path.to_str().unwrap(), r"C:\a\b");
        // parent() only computes the next location; it does not modify history.
        assert_eq!(nav.current().unwrap().path.to_str().unwrap(), r"C:\a\b\c");
    }

    #[test]
    fn back_forward_across_tabs_isolated() {
        let mut nav = NavigationController::new(loc("A"));
        nav.navigate(loc("B"));
        nav.navigate(loc("C"));

        let first_id = nav.active_tab_id();
        let second_id = nav.tabs.create(loc("X"));
        nav.tabs.switch_to(second_id);

        assert_eq!(nav.current().unwrap().path.to_str().unwrap(), "X");
        nav.tabs.switch_to(first_id);
        assert!(nav.back().is_some());
        assert_eq!(nav.current().unwrap().path.to_str().unwrap(), "B");
    }
}
