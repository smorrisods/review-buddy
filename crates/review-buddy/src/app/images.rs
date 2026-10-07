//! Pictures in the Overview: asking for them, recording the answers, and choosing one with `i`.

use std::sync::Arc;

use image::DynamicImage;
use rb_core::ChangeId;
use url::Url;

use super::update::set_status;
use super::{links, App, Cmd, Notice, NoticeKind, Screen, Tab};
use crate::images::extract::{self, ImageRef};
use crate::images::fetch::Failure;
use crate::images::hosts::{host_label, ForgeHosts, Trust};
use crate::images::MAX_IMAGES;

/// The images of a change's description, resolved against its page. At most [`MAX_IMAGES`].
pub fn refs_for(app: &App, id: &ChangeId) -> Vec<ImageRef> {
    let Some(info) = app.state.details.get(id) else {
        return Vec::new();
    };
    let base = links::change_url(app, id).and_then(|u| Url::parse(&u).ok());
    let mut found = extract::images(&info.body, base.as_ref());
    found.truncate(MAX_IMAGES);
    found
}

/// Asks for the selected change's images once its description has loaded. Does nothing when
/// pictures are off, which keeps every request (and the privacy cost of one) behind the setting.
pub fn ensure(app: &mut App) -> Vec<Cmd> {
    if !app.images.is_on() || app.screen != Screen::Dashboard {
        return Vec::new();
    }
    let Some(id) = app.selected_change().map(|c| c.id.clone()) else {
        return Vec::new();
    };
    if app.images.checked.as_ref() == Some(&id) || !app.state.details.contains_key(&id) {
        return Vec::new();
    }
    let hosts = app
        .state
        .source(&id.source_id)
        .map(|source| ForgeHosts::for_source(source, None));
    let mut cmds = Vec::new();
    for image in refs_for(app, &id) {
        let Some(url) = image.url else { continue };
        if app.images.knows(url.as_str()) {
            continue;
        }
        let outside = hosts.as_ref().is_none_or(|h| h.trust(&url) != Trust::Forge);
        if app.images.forge_only() && outside {
            app.images
                .fail(url.as_str(), Failure::External(host_label(&url)));
            continue;
        }
        app.images.start(url.as_str());
        cmds.push(Cmd::FetchImage {
            source: id.source_id.clone(),
            url: url.to_string(),
        });
    }
    app.images.checked = Some(id);
    cmds
}

pub fn on_loaded(app: &mut App, url: String, result: Result<Arc<DynamicImage>, Failure>) {
    let keep: Vec<String> = app
        .selected_change()
        .map(|c| c.id.clone())
        .map(|id| {
            refs_for(app, &id)
                .into_iter()
                .filter_map(|r| r.url.map(|u| u.to_string()))
                .collect()
        })
        .unwrap_or_default();
    app.images.finish(&url, result, &keep);
    app.mark_dirty();
}

/// `i`: selects the next image in the description, then none after the last.
pub fn next(app: &mut App) -> Vec<Cmd> {
    let Some(id) = app.selected_change().map(|c| c.id.clone()) else {
        let text = "Nothing to look at yet. Select a change first.";
        return set_status(app, Notice::new(NoticeKind::Info, text));
    };
    let refs = refs_for(app, &id);
    if refs.is_empty() {
        let text = "No images in this description.";
        return set_status(app, Notice::new(NoticeKind::Info, text));
    }
    let current = match &app.images.focus {
        Some((focused, at)) if *focused == id => Some(*at),
        _ => None,
    };
    let target = match current {
        None => Some(0),
        Some(at) if at + 1 < refs.len() => Some(at + 1),
        Some(_) => None,
    };
    app.images.focus = target.map(|at| (id, at));
    let Some(at) = target else {
        return set_status(
            app,
            Notice::new(
                NoticeKind::Info,
                "No image selected. i selects the first again.",
            ),
        );
    };
    if app.dashboard.tab != Tab::Overview {
        app.dashboard.tab = Tab::Overview;
        app.dashboard.detail_scroll = 0;
    }
    if let Some(scroll) = crate::ui::detail::scroll_to_image(app, at) {
        app.dashboard.detail_scroll = scroll;
    }
    let text = format!(
        "Image {} of {}: {}. o opens it, y copies its address.",
        at + 1,
        refs.len(),
        refs[at].label()
    );
    set_status(app, Notice::new(NoticeKind::Info, text))
}

/// The address `o` and `y` act on when an image is selected on the dashboard.
pub fn focused_url(app: &App) -> Option<String> {
    if app.screen != Screen::Dashboard || app.dashboard.tab != Tab::Overview {
        return None;
    }
    let (focused, at) = app.images.focus.as_ref()?;
    let current = &app.selected_change()?.id;
    if focused != current {
        return None;
    }
    refs_for(app, current)
        .get(*at)?
        .url
        .as_ref()
        .map(Url::to_string)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rb_core::{
        AuthMode, ChangeState, ChangeSummary, CiState, ForgeKind, MyReview, MyRole, Scope, Source,
        SourceId, Timestamp,
    };
    use rb_theme::ColourDepth;

    use super::*;
    use crate::app::{update, AppConfig, ChangeInfo, Msg, Snapshot};
    use crate::config::Images;
    use crate::images::State;

    const BODY: &str = "Look:\n\n![Login](https://cdn.example.net/login.png)\n\n![Inside](https://github.com/user-attachments/assets/abc)\n\n![Relative](/acme/web/raw/a.png)\n";

    fn source(kind: ForgeKind, host: &str) -> Source {
        Source {
            id: SourceId::new("s"),
            kind,
            host: host.into(),
            label: "s".into(),
            scope: Scope::everything(),
            auth: AuthMode::Cli,
            in_all: true,
            include_drafts: true,
            tag_colour: None,
        }
    }

    fn change() -> ChangeSummary {
        ChangeSummary {
            id: ChangeId {
                source_id: SourceId::new("s"),
                kind: ForgeKind::GitHub,
                repo: "acme/web".into(),
                number: 7,
            },
            title: "Pictures".into(),
            author: "mira".into(),
            author_is_bot: false,
            state: ChangeState::Open,
            draft: false,
            created_at: Timestamp(1),
            updated_at: Timestamp(2),
            branch: "feat".into(),
            base: "main".into(),
            head_sha: "abc".into(),
            base_sha: "def".into(),
            adds: 1,
            dels: 1,
            files: 1,
            ci: CiState::Pass,
            labels: Vec::new(),
            reviewers: Vec::new(),
            my_role: MyRole::Reviewing,
            my_review: MyReview::None,
            my_reviewed_sha: None,
            i_commented: false,
            has_new_activity: false,
        }
    }

    fn app(images: State, body: &str) -> App {
        loaded(images, body).0
    }

    /// The app with `body` loaded, and the commands that loading it produced.
    fn loaded(images: State, body: &str) -> (App, Vec<Cmd>) {
        let mut app = App::new(AppConfig {
            theme_id: "liminal-hq".into(),
            depth: ColourDepth::TrueColour,
            no_color: false,
            size: (160, 40),
        });
        app.images = images;
        let change = change();
        let mut details = std::collections::HashMap::new();
        details.insert(
            change.id.clone(),
            ChangeInfo {
                body: body.into(),
                ..ChangeInfo::default()
            },
        );
        let cmds = update(
            &mut app,
            Msg::Loaded(Box::new(Snapshot {
                label: "t".into(),
                sources: vec![source(ForgeKind::GitHub, "github.com")],
                changes: vec![change],
                now: Timestamp(10),
                details,
            })),
        );
        (app, cmds)
    }

    fn fetches(cmds: &[Cmd]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                Cmd::FetchImage { url, .. } => Some(url.clone()),
                _ => None,
            })
            .collect()
    }

    fn press(app: &mut App, c: char) -> Vec<Cmd> {
        update(
            app,
            Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
        )
    }

    #[test]
    fn nothing_is_fetched_while_images_are_off() {
        let (mut off, cmds) = loaded(State::default(), BODY);
        assert!(fetches(&cmds).is_empty());
        assert!(fetches(&update(&mut off, Msg::Tick)).is_empty());
        let (_, cmds) = loaded(State::new(Images::Off, None), BODY);
        assert!(fetches(&cmds).is_empty());
    }

    #[test]
    fn a_loaded_description_asks_for_each_image_once() {
        let (mut app, cmds) = loaded(State::with_halfblocks(), BODY);
        assert_eq!(
            fetches(&cmds),
            [
                "https://cdn.example.net/login.png",
                "https://github.com/user-attachments/assets/abc",
                "https://github.com/acme/web/raw/a.png",
            ]
        );
        assert!(
            fetches(&update(&mut app, Msg::Tick)).is_empty(),
            "asked once"
        );
    }

    #[test]
    fn answers_are_recorded_and_failures_stay_failures() {
        let mut app = app(State::with_halfblocks(), BODY);
        update(
            &mut app,
            Msg::ImageLoaded {
                url: "https://cdn.example.net/login.png".into(),
                result: Ok(Arc::new(DynamicImage::new_rgb8(4, 4))),
            },
        );
        update(
            &mut app,
            Msg::ImageLoaded {
                url: "https://github.com/user-attachments/assets/abc".into(),
                result: Err(Failure::Svg),
            },
        );
        let id = app.selected_change().unwrap().id.clone();
        let refs = refs_for(&app, &id);
        assert!(matches!(
            app.images.view(&refs[0]),
            crate::images::View::Ready(_)
        ));
        assert!(matches!(
            app.images.view(&refs[1]),
            crate::images::View::Failed(Failure::Svg)
        ));
        assert!(matches!(
            app.images.view(&refs[2]),
            crate::images::View::Loading
        ));
        assert!(
            fetches(&update(&mut app, Msg::Tick)).is_empty(),
            "no retry loop"
        );
    }

    #[test]
    fn forge_only_refuses_other_hosts_without_asking_for_them() {
        let (app, cmds) = loaded(
            State::new(
                Images::ForgeOnly,
                Some(ratatui_image::picker::Picker::halfblocks()),
            ),
            BODY,
        );
        assert_eq!(
            fetches(&cmds),
            [
                "https://github.com/user-attachments/assets/abc",
                "https://github.com/acme/web/raw/a.png"
            ]
        );
        let id = app.selected_change().unwrap().id.clone();
        let refs = refs_for(&app, &id);
        assert!(matches!(
            app.images.view(&refs[0]),
            crate::images::View::Failed(Failure::External(host)) if host == "cdn.example.net"
        ));
    }

    #[test]
    fn at_most_eight_images_are_asked_for() {
        let body: String = (0..12)
            .map(|n| format!("![n{n}](https://cdn.example.net/{n}.png)\n\n"))
            .collect();
        let (_, cmds) = loaded(State::with_halfblocks(), &body);
        assert_eq!(fetches(&cmds).len(), MAX_IMAGES);
    }

    #[test]
    fn i_walks_through_the_images_and_o_opens_the_selected_one() {
        let mut app = app(State::with_halfblocks(), BODY);
        assert_eq!(focused_url(&app), None);
        press(&mut app, 'i');
        assert_eq!(
            focused_url(&app).as_deref(),
            Some("https://cdn.example.net/login.png")
        );
        let cmds = press(&mut app, 'o');
        assert!(
            matches!(cmds.first(), Some(Cmd::OpenUrl(u)) if u == "https://cdn.example.net/login.png")
        );
        let cmds = press(&mut app, 'y');
        assert!(
            matches!(cmds.first(), Some(Cmd::Copy(u)) if u == "https://cdn.example.net/login.png")
        );
        press(&mut app, 'i');
        press(&mut app, 'i');
        assert_eq!(
            focused_url(&app).as_deref(),
            Some("https://github.com/acme/web/raw/a.png")
        );
        press(&mut app, 'i');
        assert_eq!(focused_url(&app), None, "after the last, none");
        let cmds = press(&mut app, 'o');
        assert!(
            matches!(cmds.first(), Some(Cmd::OpenUrl(u)) if u.ends_with("/pull/7")),
            "o opens the change again"
        );
    }

    #[test]
    fn i_says_so_when_there_is_nothing_to_select() {
        let mut app = app(State::with_halfblocks(), "Just words.");
        press(&mut app, 'i');
        assert_eq!(
            app.status.as_ref().unwrap().notice.text,
            "No images in this description."
        );
        assert!(app.images.focus.is_none());
    }

    #[test]
    fn i_works_even_when_images_are_off_so_o_can_still_open_them() {
        let mut app = app(State::default(), BODY);
        press(&mut app, 'i');
        assert!(focused_url(&app).is_some());
    }

    #[test]
    fn a_reloaded_description_asks_again() {
        let mut app = app(State::with_halfblocks(), BODY);
        let id = app.selected_change().unwrap().id.clone();
        let cmds = update(
            &mut app,
            Msg::InfoLoaded {
                id,
                result: Ok(Box::new(ChangeInfo {
                    body: format!("{BODY}\n![New](https://cdn.example.net/new.png)"),
                    ..ChangeInfo::default()
                })),
            },
        );
        assert_eq!(fetches(&cmds), ["https://cdn.example.net/new.png"]);
    }
}
