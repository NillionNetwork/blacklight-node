use crossterm::event::{self, Event, KeyCode, KeyEvent};
use std::time::Duration;

/// Tab selection state for TUI applications
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Nodes,
    HtxSubmitted,
    HtxAssigned,
    HtxResponded,
}

impl Tab {
    /// Get all available tabs
    pub fn all() -> &'static [Tab] {
        &[
            Tab::Overview,
            Tab::Nodes,
            Tab::HtxSubmitted,
            Tab::HtxAssigned,
            Tab::HtxResponded,
        ]
    }

    /// Get the title for this tab
    pub fn title(&self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Nodes => "Nodes",
            Tab::HtxSubmitted => "HTX Submitted",
            Tab::HtxAssigned => "HTX Assigned",
            Tab::HtxResponded => "HTX Responded",
        }
    }

    /// Move to the next tab (wraps around)
    pub fn next(&self) -> Tab {
        let tabs = Self::all();
        let current_index = tabs.iter().position(|t| t == self).unwrap_or(0);
        let next_index = (current_index + 1) % tabs.len();
        tabs[next_index]
    }

    /// Move to the previous tab (wraps around)
    pub fn prev(&self) -> Tab {
        let tabs = Self::all();
        let current_index = tabs.iter().position(|t| t == self).unwrap_or(0);
        let prev_index = if current_index == 0 {
            tabs.len() - 1
        } else {
            current_index - 1
        };
        tabs[prev_index]
    }
}

/// Poll for keyboard events with a timeout
pub fn poll_event(timeout: Duration) -> std::io::Result<Option<Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}

/// Check if a key event matches a specific key code
pub fn is_key(event: &Event, code: KeyCode) -> bool {
    matches!(
        event,
        Event::Key(KeyEvent { code: c, .. }) if *c == code
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_navigation() {
        assert_eq!(Tab::Overview.next(), Tab::Nodes);
        assert_eq!(Tab::HtxResponded.next(), Tab::Overview); // wraps around

        assert_eq!(Tab::Nodes.prev(), Tab::Overview);
        assert_eq!(Tab::Overview.prev(), Tab::HtxResponded); // wraps around
    }

    #[test]
    fn test_tab_titles() {
        assert_eq!(Tab::Overview.title(), "Overview");
        assert_eq!(Tab::Nodes.title(), "Nodes");
    }

    #[test]
    fn test_all_tabs() {
        let tabs = Tab::all();
        assert_eq!(tabs.len(), 5);
        assert_eq!(tabs[0], Tab::Overview);
    }
}
