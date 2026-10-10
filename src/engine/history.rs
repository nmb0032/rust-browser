use url::Url;

const MAX_HISTORY_ENTRIES: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub url: Url,
    pub scroll_offset: f32,
}

#[derive(Debug)]
pub struct History {
    entries: Vec<HistoryEntry>,
    index: usize,
}

impl History {
    pub fn new(url: Url) -> Self {
        Self {
            entries: vec![HistoryEntry {
                url,
                scroll_offset: 0.0,
            }],
            index: 0,
        }
    }

    pub fn current(&self) -> &HistoryEntry {
        &self.entries[self.index]
    }

    pub fn current_mut(&mut self) -> &mut HistoryEntry {
        &mut self.entries[self.index]
    }

    pub fn push(&mut self, url: Url) {
        self.entries.truncate(self.index + 1);
        self.entries.push(HistoryEntry {
            url,
            scroll_offset: 0.0,
        });
        if self.entries.len() > MAX_HISTORY_ENTRIES {
            self.entries.remove(0);
        }
        self.index = self.entries.len() - 1;
    }

    pub fn back(&mut self) -> Option<&HistoryEntry> {
        if self.index == 0 {
            return None;
        }
        self.index -= 1;
        Some(self.current())
    }

    pub fn forward(&mut self) -> Option<&HistoryEntry> {
        if self.index + 1 >= self.entries.len() {
            return None;
        }
        self.index += 1;
        Some(self.current())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_restores_scroll_and_new_navigation_discards_forward_entries() {
        let url = |path| Url::parse(&format!("https://example.org/{path}")).unwrap();
        let mut history = History::new(url("a"));
        assert!(history.back().is_none());
        assert!(history.forward().is_none());
        history.current_mut().scroll_offset = 120.0;
        history.push(url("b"));
        history.current_mut().url = url("redirected");
        history.push(url("c"));
        assert_eq!(history.back().unwrap().url, url("redirected"));
        assert_eq!(history.back().unwrap().scroll_offset, 120.0);
        assert_eq!(history.forward().unwrap().url, url("redirected"));
        history.push(url("d"));
        assert!(history.forward().is_none());
        assert_eq!(history.current().scroll_offset, 0.0);
    }

    #[test]
    fn history_is_bounded() {
        let mut history = History::new(Url::parse("https://example.org/").unwrap());
        for index in 0..MAX_HISTORY_ENTRIES * 2 {
            history.push(Url::parse(&format!("https://example.org/{index}")).unwrap());
        }
        assert_eq!(history.entries.len(), MAX_HISTORY_ENTRIES);
        assert_eq!(history.index, MAX_HISTORY_ENTRIES - 1);
    }
}
