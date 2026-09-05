/// Selection state for a directory listing. Operates on indices into the
/// current sorted entry list, not on pixel positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionState {
    selected: Vec<usize>,
    anchor: Option<usize>,
    last_focus: Option<usize>,
}

impl SelectionState {
    pub fn empty() -> Self {
        Self {
            selected: Vec::new(),
            anchor: None,
            last_focus: None,
        }
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    pub fn selected(&self) -> &[usize] {
        &self.selected
    }

    pub fn primary(&self) -> Option<usize> {
        self.last_focus
            .filter(|i| self.is_selected(*i))
            .or(self.anchor)
            .filter(|i| self.is_selected(*i))
            .or(self.selected.last().copied())
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// Single selection: replace the entire selection with the given index.
    pub fn select_single(&mut self, index: usize) {
        self.selected.clear();
        self.selected.push(index);
        self.anchor = Some(index);
        self.last_focus = Some(index);
    }

    /// Ctrl-click: toggle the index in the selection.
    pub fn toggle(&mut self, index: usize) {
        if let Some(pos) = self.selected.iter().position(|&i| i == index) {
            self.selected.remove(pos);
            if self.selected.is_empty() {
                self.last_focus = None;
                self.anchor = None;
            }
        } else {
            self.selected.push(index);
            self.last_focus = Some(index);
            if self.anchor.is_none() {
                self.anchor = Some(index);
            }
        }
    }

    /// Shift-click / Shift-arrow: select the inclusive range between the anchor
    /// and the new index. Keeps the existing anchor.
    pub fn range_select(&mut self, index: usize) {
        let anchor = self.anchor.unwrap_or(index);
        self.anchor = Some(anchor);
        let start = anchor.min(index);
        let end = anchor.max(index);
        self.selected.clear();
        self.selected.extend(start..=end);
        self.last_focus = Some(index);
    }

    /// Select all indices in `[0, len)`.
    pub fn select_all(&mut self, len: usize) {
        if len == 0 {
            self.clear();
            return;
        }
        self.selected.clear();
        self.selected.extend(0..len);
        self.anchor = Some(0);
        self.last_focus = Some(if len == 0 { 0 } else { len - 1 });
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.last_focus = None;
    }

    /// Recompute a rubber-band selection from its mouse-down baseline. Using
    /// the baseline (not the previous frame) lets the band shrink cleanly.
    pub fn marquee(
        &mut self,
        baseline: &Self,
        range: std::ops::Range<usize>,
        additive: bool,
        len: usize,
    ) {
        self.selected.clear();
        if additive {
            self.selected
                .extend(baseline.selected.iter().copied().filter(|&i| i < len));
        }
        self.selected
            .extend(range.start.min(len)..range.end.min(len));
        self.selected.sort_unstable();
        self.selected.dedup();
        self.anchor = self.selected.first().copied();
        self.last_focus = self.selected.last().copied();
    }

    /// Preserve file identity, anchor and focus when a snapshot is reordered.
    pub fn remap(&mut self, mut index: impl FnMut(usize) -> Option<usize>) {
        self.selected = self.selected.iter().filter_map(|&i| index(i)).collect();
        self.anchor = self.anchor.and_then(&mut index);
        self.last_focus = self.last_focus.and_then(index);
    }

    /// Move the focus by one step, optionally extending the selection when
    /// `extend` is true (Shift held). Returns the new focus index or `None` if
    /// the list is empty.
    pub fn move_focus(&mut self, current_len: usize, delta: isize, extend: bool) -> Option<usize> {
        if current_len == 0 {
            self.clear();
            return None;
        }

        let focus = self.last_focus.unwrap_or(0);
        let new_focus = if delta.is_negative() {
            focus.saturating_sub(delta.unsigned_abs())
        } else {
            (focus + delta.unsigned_abs()).min(current_len - 1)
        };

        if extend {
            self.range_select(new_focus);
        } else {
            self.select_single(new_focus);
        }
        Some(new_focus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marquee_shrinks_and_clears_without_leaving_old_rows_selected() {
        let baseline = SelectionState::empty();
        let mut s = baseline.clone();
        s.marquee(&baseline, 1..8, false, 10);
        s.marquee(&baseline, 3..5, false, 10);
        assert_eq!(s.selected(), &[3, 4]);
        s.marquee(&baseline, 12..15, false, 10);
        assert!(s.is_empty());
        assert_eq!(s.primary(), None);
    }

    #[test]
    fn additive_marquee_preserves_baseline_and_clamps_directory_bounds() {
        let mut baseline = SelectionState::empty();
        baseline.select_single(1);
        baseline.toggle(4);
        let mut s = baseline.clone();
        s.marquee(&baseline, 3..20, true, 6);
        assert_eq!(s.selected(), &[1, 3, 4, 5]);
        s.marquee(&baseline, 4..5, true, 6);
        assert_eq!(s.selected(), &[1, 4]);
        s.marquee(&baseline, 0..10, true, 0);
        assert!(s.is_empty());
    }

    #[test]
    fn single_select_then_toggle_multi() {
        let mut s = SelectionState::empty();
        s.select_single(2);
        assert_eq!(s.selected(), vec![2]);

        s.toggle(5);
        assert_eq!(s.selected(), vec![2, 5]);

        s.toggle(2);
        assert_eq!(s.selected(), vec![5]);
    }

    #[test]
    fn range_select_from_anchor() {
        let mut s = SelectionState::empty();
        s.select_single(3);
        s.range_select(6);
        assert_eq!(s.selected(), vec![3, 4, 5, 6]);

        // Branching backwards from anchor.
        s.range_select(1);
        assert_eq!(s.selected(), vec![1, 2, 3]);
    }

    #[test]
    fn select_all_and_clear() {
        let mut s = SelectionState::empty();
        s.select_all(4);
        assert_eq!(s.selected(), vec![0, 1, 2, 3]);
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn move_focus_basic_and_extend() {
        let mut s = SelectionState::empty();
        let f = s.move_focus(10, 3, false);
        assert_eq!(f, Some(3));
        assert_eq!(s.selected(), vec![3]);

        let f = s.move_focus(10, 2, true);
        assert_eq!(f, Some(5));
        assert_eq!(s.selected(), vec![3, 4, 5]);
    }
}
