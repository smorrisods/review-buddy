//! What the interface knows about images: the terminal's renderer, each address's progress, the
//! focused image and the encoded pictures ready to draw.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use image::DynamicImage;
use ratatui::{layout::Rect, layout::Size, Frame};
use ratatui_image::{
    picker::{Picker, ProtocolType},
    sliced::{SlicedImage, SlicedProtocol},
    FilterType, Resize,
};
use rb_core::ChangeId;

use super::extract::ImageRef;
use super::fetch::Failure;
use crate::config::Images;

/// Decoded images kept in memory; older ones are read from the disk cache again.
const KEEP: usize = 16;
/// Encoded pictures kept for drawing, by address and size.
const KEEP_ENCODED: usize = 32;

#[derive(Debug, Clone)]
enum Entry {
    Loading,
    Ready(Arc<DynamicImage>),
    Failed(Failure),
}

/// Where one image stands, as the Overview draws it.
pub enum View<'a> {
    /// No renderer is available, so only a note is shown and nothing is fetched.
    Off,
    Loading,
    Failed(&'a Failure),
    Ready(&'a DynamicImage),
}

/// A reserved block of rows in the description that an image is drawn into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// The first row of the block, counted from the top of the description.
    pub line: usize,
    pub cols: u16,
    pub rows: u16,
    pub url: String,
}

pub struct State {
    mode: Images,
    picker: Option<Picker>,
    entries: HashMap<String, Entry>,
    order: VecDeque<String>,
    /// The change whose descriptions' images have been asked for.
    pub(crate) checked: Option<ChangeId>,
    /// The image `i` has selected: the change and its position in the description.
    pub focus: Option<(ChangeId, usize)>,
    encoded: RefCell<EncodedPictures>,
}

/// Pictures encoded for the terminal, by address and size in cells; `None` when encoding failed.
type EncodedPictures = HashMap<(String, u16, u16), Option<Rc<SlicedProtocol>>>;

impl Default for State {
    fn default() -> Self {
        Self::new(Images::Auto, None)
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("mode", &self.mode.as_str())
            .field("protocol", &self.protocol())
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl State {
    pub fn new(mode: Images, picker: Option<Picker>) -> Self {
        Self {
            mode,
            picker,
            entries: HashMap::new(),
            order: VecDeque::new(),
            checked: None,
            focus: None,
            encoded: RefCell::default(),
        }
    }

    /// Draws in the halfblocks fallback with a 10 by 20 pixel cell, whatever the terminal is.
    /// For tests, which must not depend on the machine they run on.
    pub fn with_halfblocks() -> Self {
        Self::new(Images::Halfblocks, Some(Picker::halfblocks()))
    }

    pub fn mode(&self) -> Images {
        self.mode
    }

    /// Whether images are fetched and drawn at all.
    pub fn is_on(&self) -> bool {
        self.picker.is_some()
    }

    pub fn forge_only(&self) -> bool {
        self.mode == Images::ForgeOnly
    }

    /// The graphics protocol in use, for the status line and tests.
    pub fn protocol(&self) -> Option<&'static str> {
        self.picker.as_ref().map(|p| match p.protocol_type() {
            ProtocolType::Halfblocks => "halfblocks",
            ProtocolType::Sixel => "sixel",
            ProtocolType::Kitty => "kitty",
            ProtocolType::Iterm2 => "iterm2",
        })
    }

    /// Pixels per terminal cell, `(width, height)`.
    pub fn font(&self) -> (u16, u16) {
        self.picker.as_ref().map_or((10, 20), |p| {
            let size = p.font_size();
            (size.width, size.height)
        })
    }

    pub fn view(&self, image: &ImageRef) -> View<'_> {
        if !self.is_on() {
            return View::Off;
        }
        match image
            .url
            .as_ref()
            .and_then(|u| self.entries.get(u.as_str()))
        {
            Some(Entry::Ready(img)) => View::Ready(img),
            Some(Entry::Failed(failure)) => View::Failed(failure),
            Some(Entry::Loading) | None => View::Loading,
        }
    }

    pub fn knows(&self, url: &str) -> bool {
        self.entries.contains_key(url)
    }

    pub fn start(&mut self, url: &str) {
        self.entries.insert(url.to_string(), Entry::Loading);
    }

    pub fn fail(&mut self, url: &str, failure: Failure) {
        self.entries.insert(url.to_string(), Entry::Failed(failure));
    }

    /// Records an answer. `keep` are the addresses on screen, which are never evicted.
    pub fn finish(
        &mut self,
        url: &str,
        result: Result<Arc<DynamicImage>, Failure>,
        keep: &[String],
    ) {
        let entry = match result {
            Ok(img) => Entry::Ready(img),
            Err(failure) => Entry::Failed(failure),
        };
        self.entries.insert(url.to_string(), entry);
        self.order.retain(|u| u != url);
        self.order.push_back(url.to_string());
        self.evict(keep);
    }

    fn evict(&mut self, keep: &[String]) {
        let mut spare = self.order.len().saturating_sub(KEEP);
        let mut at = 0;
        while spare > 0 && at < self.order.len() {
            if keep.contains(&self.order[at]) {
                at += 1;
                continue;
            }
            if let Some(url) = self.order.remove(at) {
                self.entries.remove(&url);
                self.encoded.borrow_mut().retain(|(u, _, _), _| *u != url);
                spare -= 1;
            }
        }
        // A change whose pictures were dropped asks for them again when it is next shown.
        self.checked = None;
    }

    /// Draws the picture for `slot`, `top` rows below the top of `area` (negative when the
    /// block has scrolled partly off the top). Rows outside `area` are clipped.
    pub fn render(&self, frame: &mut Frame, area: Rect, slot: &Slot, top: i16) {
        let Some(Entry::Ready(img)) = self.entries.get(&slot.url) else {
            return;
        };
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        let key = (slot.url.clone(), slot.cols, slot.rows);
        let mut encoded = self.encoded.borrow_mut();
        if encoded.len() >= KEEP_ENCODED && !encoded.contains_key(&key) {
            encoded.clear();
        }
        let protocol = encoded
            .entry(key)
            .or_insert_with(|| {
                SlicedProtocol::new_with_resize(
                    picker,
                    (**img).clone(),
                    Size::new(slot.cols, slot.rows),
                    Resize::Fit(Some(FilterType::Triangle)),
                )
                .ok()
                .map(Rc::new)
            })
            .clone();
        drop(encoded);
        match protocol {
            Some(protocol) => {
                frame.render_widget(SlicedImage::new(&protocol, (0, top).into()), area);
            }
            None => {
                let note = ratatui::widgets::Paragraph::new("(this image couldn't be drawn)");
                let row = i32::from(top).max(0);
                if row < i32::from(area.height) {
                    frame
                        .render_widget(note, Rect::new(area.x, area.y + row as u16, area.width, 1));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic() -> Arc<DynamicImage> {
        Arc::new(DynamicImage::new_rgb8(4, 4))
    }

    #[test]
    fn nothing_is_on_without_a_renderer() {
        let state = State::new(Images::Auto, None);
        assert!(!state.is_on());
        let image = ImageRef {
            src: String::new(),
            url: url::Url::parse("https://x.test/a.png").ok(),
            alt: String::new(),
            width: None,
            height: None,
        };
        assert!(matches!(state.view(&image), View::Off));
    }

    #[test]
    fn views_follow_progress() {
        let mut state = State::with_halfblocks();
        let image = ImageRef {
            src: String::new(),
            url: url::Url::parse("https://x.test/a.png").ok(),
            alt: String::new(),
            width: None,
            height: None,
        };
        assert!(matches!(state.view(&image), View::Loading));
        state.start("https://x.test/a.png");
        assert!(matches!(state.view(&image), View::Loading));
        state.finish("https://x.test/a.png", Ok(pic()), &[]);
        assert!(matches!(state.view(&image), View::Ready(_)));
        state.finish("https://x.test/a.png", Err(Failure::Svg), &[]);
        assert!(matches!(state.view(&image), View::Failed(Failure::Svg)));
    }

    #[test]
    fn old_pictures_are_evicted_but_the_ones_on_screen_stay() {
        let mut state = State::with_halfblocks();
        let name = |n: usize| format!("https://x.test/{n}.png");
        let keep = vec![name(0)];
        for n in 0..(KEEP + 4) {
            state.finish(&name(n), Ok(pic()), &keep);
        }
        assert!(state.knows(&name(0)), "kept because it is on screen");
        assert!(!state.knows(&name(1)));
        assert!(state.knows(&name(KEEP + 3)));
        assert_eq!(state.entries.len(), KEEP);
    }
}
