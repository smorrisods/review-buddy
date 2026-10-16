//! The terminal window title, so a tab bar or a tmux status line says what is on screen.

use super::{App, Screen};

const NAME: &str = "review-buddy";
/// Longest title sent, in characters. Terminals and tab bars cut it anyway.
const MAX_CHARS: usize = 120;

/// The title for what the app is showing now. Pure, so the runtime can compare it with the last
/// one sent and write only on a change.
pub fn window_title(app: &App) -> String {
    let place = match app.screen {
        Screen::FirstRun => "setup".to_string(),
        Screen::Settings => "settings".to_string(),
        Screen::Diff => diff_place(app),
        Screen::Dashboard => dashboard_place(app),
    };
    let demo = if app.demo { " (demo)" } else { "" };
    clean(&format!("{NAME}{demo} · {place}"))
}

fn dashboard_place(app: &App) -> String {
    match app.selected_change() {
        Some(change) => format!("{} · {}", change.id.short_ref(), change.title),
        None => "queue".to_string(),
    }
}

fn diff_place(app: &App) -> String {
    let Some(diff) = app.diff.as_ref() else {
        return "diff".to_string();
    };
    let change = diff.id.short_ref();
    match diff.data.as_ref() {
        Some(data) if !data.files.is_empty() => {
            let path = data
                .files
                .get(diff.file)
                .map_or("", |f| f.diff.path.as_str());
            format!(
                "{change} · {path} · file {} of {}",
                diff.file + 1,
                data.files.len()
            )
        }
        _ => format!("{change} · diff"),
    }
}

/// Forge text goes into the title, so control characters (an escape would end the sequence early)
/// are dropped and the length is capped.
fn clean(text: &str) -> String {
    let flat: String = text.chars().filter(|c| !c.is_control()).collect();
    if flat.chars().count() <= MAX_CHARS {
        return flat;
    }
    let mut cut: String = flat.chars().take(MAX_CHARS - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppConfig;

    fn app() -> App {
        App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: rb_theme::ColourDepth::TrueColour,
            no_color: false,
            size: (120, 40),
        })
    }

    #[test]
    fn queue_with_nothing_selected() {
        assert_eq!(window_title(&app()), "review-buddy · queue");
    }

    #[test]
    fn settings_and_setup_name_their_screen() {
        let mut app = app();
        app.screen = Screen::Settings;
        assert_eq!(window_title(&app), "review-buddy · settings");
        app.screen = Screen::FirstRun;
        assert_eq!(window_title(&app), "review-buddy · setup");
    }

    #[test]
    fn demo_is_labelled() {
        let mut app = app();
        app.demo = true;
        assert_eq!(window_title(&app), "review-buddy (demo) · queue");
    }

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(clean("a\x1b]0;evil\x07b\n"), "a]0;evilb");
    }

    #[test]
    fn long_titles_are_cut_with_an_ellipsis() {
        let out = clean(&"x".repeat(300));
        assert_eq!(out.chars().count(), MAX_CHARS);
        assert!(out.ends_with('…'));
    }
}
