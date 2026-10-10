use winit::keyboard::{Key, ModifiersState, NamedKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HistoryDirection {
    Back,
    Forward,
}

pub(super) fn history_shortcut(key: &Key, modifiers: ModifiersState) -> Option<HistoryDirection> {
    match key {
        Key::Named(NamedKey::ArrowLeft) if modifiers.alt_key() => Some(HistoryDirection::Back),
        Key::Named(NamedKey::ArrowRight) if modifiers.alt_key() => Some(HistoryDirection::Forward),
        Key::Character(key) if modifiers.super_key() && key == "[" => Some(HistoryDirection::Back),
        Key::Character(key) if modifiers.super_key() && key == "]" => {
            Some(HistoryDirection::Forward)
        }
        _ => None,
    }
}

pub(super) fn document_point(x: f64, y: f64, scale: f64, scroll: f32) -> (f32, f32) {
    ((x / scale) as f32, (y / scale) as f32 + scroll)
}

pub(super) fn window_title(url: &url::Url, loading: bool, failed: bool) -> String {
    let state = if loading {
        "Loading - "
    } else if failed {
        "Load failed - "
    } else {
        ""
    };
    format!("rust-browser - {state}{url}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_requires_modifiers_and_clicks_include_scale_and_scroll() {
        let left = Key::Named(NamedKey::ArrowLeft);
        assert_eq!(history_shortcut(&left, ModifiersState::empty()), None);
        assert_eq!(
            history_shortcut(&left, ModifiersState::ALT),
            Some(HistoryDirection::Back)
        );
        assert_eq!(
            history_shortcut(&Key::Character("]".into()), ModifiersState::SUPER),
            Some(HistoryDirection::Forward)
        );
        assert_eq!(document_point(100.0, 60.0, 2.0, 120.0), (50.0, 150.0));
    }

    #[test]
    fn title_contains_url_and_distinguishes_loading_success_and_failure() {
        let url = url::Url::parse("https://example.org/page#part").unwrap();
        assert_eq!(
            window_title(&url, true, false),
            "rust-browser - Loading - https://example.org/page#part"
        );
        assert_eq!(
            window_title(&url, false, false),
            "rust-browser - https://example.org/page#part"
        );
        assert_eq!(
            window_title(&url, false, true),
            "rust-browser - Load failed - https://example.org/page#part"
        );
    }
}
