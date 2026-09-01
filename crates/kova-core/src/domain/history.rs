use super::location::Location;

/// Navigation history for a single pane/tab.
///
/// Maintains two stacks: `back` contains locations before the current one;
/// `forward` contains locations after the current one. This design is
/// intentionally simple and unit-testable outside of any UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationHistory {
    back: Vec<Location>,
    current: Option<Location>,
    forward: Vec<Location>,
}

impl NavigationHistory {
    pub fn new(initial: Option<Location>) -> Self {
        Self {
            back: Vec::new(),
            current: initial,
            forward: Vec::new(),
        }
    }

    pub fn current(&self) -> Option<&Location> {
        self.current.as_ref()
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Navigate to a new location, clearing the forward stack.
    pub fn navigate(&mut self, location: Location) {
        if let Some(current) = self.current.take() {
            self.back.push(current);
        }
        self.current = Some(location);
        self.forward.clear();
    }

    /// Go back one step, moving the current location onto the forward stack.
    pub fn back(&mut self) -> Option<Location> {
        if let Some(prev) = self.back.pop() {
            if let Some(current) = self.current.take() {
                self.forward.push(current);
            }
            self.current = Some(prev.clone());
            Some(prev)
        } else {
            None
        }
    }

    /// Go forward one step, moving the current location onto the back stack.
    pub fn forward(&mut self) -> Option<Location> {
        if let Some(next) = self.forward.pop() {
            if let Some(current) = self.current.take() {
                self.back.push(current);
            }
            self.current = Some(next.clone());
            Some(next)
        } else {
            None
        }
    }

    /// Replace the current location without affecting history.
    pub fn replace_current(&mut self, location: Location) {
        self.current = Some(location);
    }

    pub fn back_stack(&self) -> &[Location] {
        &self.back
    }

    pub fn forward_stack(&self) -> &[Location] {
        &self.forward
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
    fn navigate_a_b_c_back_back_forward_branching() {
        let mut h = NavigationHistory::new(Some(loc("A")));
        h.navigate(loc("B"));
        h.navigate(loc("C"));
        assert_eq!(h.current().unwrap().path.to_str().unwrap(), "C");

        h.back();
        assert_eq!(h.current().unwrap().path.to_str().unwrap(), "B");

        h.back();
        assert_eq!(h.current().unwrap().path.to_str().unwrap(), "A");

        h.forward();
        assert_eq!(h.current().unwrap().path.to_str().unwrap(), "B");

        // Branching: from B navigate to D. C must disappear from forward.
        h.navigate(loc("D"));
        assert_eq!(h.current().unwrap().path.to_str().unwrap(), "D");
        assert!(!h.can_go_forward());
        assert!(h.forward_stack().is_empty());
    }

    #[test]
    fn fresh_history_without_initial_location() {
        let mut h = NavigationHistory::new(None);
        assert!(h.current().is_none());
        assert!(!h.can_go_back());
        h.navigate(loc("X"));
        assert_eq!(h.current().unwrap().path.to_str().unwrap(), "X");
    }
}
