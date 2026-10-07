//! Finds the images a Markdown description refers to: `![alt](url)`, reference-style images,
//! and HTML `<img>` tags (also inside links), and splits the text around them.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use url::Url;

/// One image a description refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    /// The address as written.
    pub src: String,
    /// The address resolved against the change's page. `None` when it isn't an http(s) address.
    pub url: Option<Url>,
    pub alt: String,
    /// Display size asked for with `width` / `height` attributes, in pixels.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl ImageRef {
    /// A short name for placeholders: the alt text, else the file name, else "image".
    pub fn label(&self) -> String {
        let alt = self.alt.split_whitespace().collect::<Vec<_>>().join(" ");
        if !alt.is_empty() {
            return alt;
        }
        self.url
            .as_ref()
            .and_then(|u| u.path_segments()?.next_back().map(str::to_string))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "image".to_string())
    }
}

/// A run of description text, or an image in its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Image(ImageRef),
}

/// The images in `body`, in reading order.
pub fn images(body: &str, base: Option<&Url>) -> Vec<ImageRef> {
    segments(body, base)
        .into_iter()
        .filter_map(|s| match s {
            Segment::Image(image) => Some(image),
            Segment::Text(_) => None,
        })
        .collect()
}

/// `body` as text and images in reading order. A body without images is one text segment, so
/// callers render it exactly as before.
pub fn segments(body: &str, base: Option<&Url>) -> Vec<Segment> {
    let mut cuts: Vec<Cut> = Vec::new();
    let mut wrappers: Vec<Range<usize>> = Vec::new();
    let mut open_image: Option<(Range<usize>, String, String)> = None;
    let mut links: Vec<LinkState> = Vec::new();

    for (event, range) in Parser::new_ext(body, Options::empty()).into_offset_iter() {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                open_image = Some((range, dest_url.to_string(), String::new()));
            }
            Event::Text(t) | Event::Code(t) if open_image.is_some() => {
                if let Some((_, _, alt)) = open_image.as_mut() {
                    alt.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak if open_image.is_some() => {
                if let Some((_, _, alt)) = open_image.as_mut() {
                    alt.push(' ');
                }
            }
            Event::End(TagEnd::Image) => {
                if let Some((range, src, alt)) = open_image.take() {
                    let image = ImageRef {
                        url: resolve(&src, base),
                        src,
                        alt,
                        width: None,
                        height: None,
                    };
                    if let Some(link) = links.last_mut() {
                        link.cuts.push(cuts.len());
                    }
                    cuts.push(Cut {
                        range,
                        image: Some(image),
                    });
                }
            }
            Event::Start(Tag::Link { .. }) => links.push(LinkState {
                range,
                has_text: false,
                cuts: Vec::new(),
            }),
            Event::Text(_) | Event::Code(_) => {
                if let Some(link) = links.last_mut() {
                    link.has_text = true;
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some(link) = links.pop() {
                    widen_to_link(&mut cuts, &link);
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                for tag in tags(&html) {
                    let at = range.start + tag.range.start..range.start + tag.range.end;
                    if tag.name == "img" {
                        cuts.push(Cut {
                            range: at,
                            image: Some(image_from_tag(&tag, base)),
                        });
                    } else if WRAPPERS.contains(&tag.name.as_str()) {
                        wrappers.push(at);
                    }
                }
            }
            _ => {}
        }
    }

    if cuts.is_empty() {
        let text = body.trim_matches('\n');
        return if text.trim().is_empty() {
            Vec::new()
        } else {
            vec![Segment::Text(text.to_string())]
        };
    }
    cuts.extend(wrappers.into_iter().map(|range| Cut { range, image: None }));
    cuts.sort_by_key(|c| (c.range.start, c.range.end));

    let mut out = Vec::new();
    let mut cursor = 0;
    let push_text = |out: &mut Vec<Segment>, from: usize, to: usize| {
        if from < to {
            let piece = body[from..to].trim();
            if !piece.trim().is_empty() {
                out.push(Segment::Text(piece.to_string()));
            }
        }
    };
    for cut in cuts {
        push_text(&mut out, cursor, cut.range.start);
        cursor = cursor.max(cut.range.end);
        if let Some(image) = cut.image {
            out.push(Segment::Image(image));
        }
    }
    push_text(&mut out, cursor, body.len());
    out
}

/// The description as plain text with each image replaced by a one-line note, for places that
/// have no room for pictures.
pub fn flatten(body: &str, base: Option<&Url>) -> String {
    segments(body, base)
        .into_iter()
        .map(|s| match s {
            Segment::Text(t) => t,
            Segment::Image(image) => format!("▣ image: {}", image.label()),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct Cut {
    range: Range<usize>,
    image: Option<ImageRef>,
}

struct LinkState {
    range: Range<usize>,
    has_text: bool,
    cuts: Vec<usize>,
}

/// An image that is all a link holds (`[![alt](img)](page)`) takes the link's markup with it.
fn widen_to_link(cuts: &mut [Cut], link: &LinkState) {
    if link.has_text || link.cuts.is_empty() {
        return;
    }
    for (n, &at) in link.cuts.iter().enumerate() {
        cuts[at].range = if n == 0 {
            link.range.clone()
        } else {
            link.range.start..link.range.start
        };
    }
}

/// Tags that only wrap pictures. They go when a description has images, and stay otherwise.
const WRAPPERS: &[&str] = &[
    "a", "picture", "source", "p", "div", "center", "br", "details", "summary",
];

/// Resolves an image address against the change's page; only http(s) survives.
fn resolve(src: &str, base: Option<&Url>) -> Option<Url> {
    let src = src.trim();
    if src.is_empty() {
        return None;
    }
    let url = match Url::parse(src) {
        Ok(url) => url,
        Err(url::ParseError::RelativeUrlWithoutBase) => base?.join(src).ok()?,
        Err(_) => return None,
    };
    let web = matches!(url.scheme(), "http" | "https");
    (web && url.host_str().is_some() && url.username().is_empty() && url.password().is_none())
        .then_some(url)
}

struct HtmlTag {
    name: String,
    range: Range<usize>,
    attrs: Vec<(String, String)>,
}

fn image_from_tag(tag: &HtmlTag, base: Option<&Url>) -> ImageRef {
    let attr = |name: &str| {
        tag.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let pixels = |name: &str| {
        attr(name).and_then(|v| {
            v.trim()
                .trim_end_matches("px")
                .parse::<u32>()
                .ok()
                .filter(|n| *n > 0)
        })
    };
    let src = attr("src").unwrap_or_default();
    ImageRef {
        url: resolve(&src, base),
        src,
        alt: attr("alt").unwrap_or_default(),
        width: pixels("width"),
        height: pixels("height"),
    }
}

/// The `img` tags and wrapper tags (opening and closing) in a run of HTML, with lowercase names
/// and decoded attributes.
fn tags(html: &str) -> Vec<HtmlTag> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i..].find("-->").map_or(html.len(), |end| i + end + 3);
            continue;
        }
        let Some(end) = tag_end(html, i) else { break };
        let inner = &html[i + 1..end - 1];
        if let Some(tag) = parse_tag(inner, i..end) {
            out.push(tag);
        }
        i = end;
    }
    out
}

/// The index just past the `>` that closes the tag starting at `start`, skipping quoted values.
fn tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (offset, ch) in html[start + 1..].char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '>') => return Some(start + 1 + offset + 1),
            _ => {}
        }
    }
    None
}

fn parse_tag(inner: &str, range: Range<usize>) -> Option<HtmlTag> {
    let closing = inner.starts_with('/');
    let inner = inner.trim_start_matches('/').trim_end_matches('/');
    let name_end = inner
        .find(|c: char| c.is_whitespace())
        .unwrap_or(inner.len());
    let name = inner[..name_end].to_ascii_lowercase();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    if closing {
        return WRAPPERS.contains(&name.as_str()).then_some(HtmlTag {
            name,
            range,
            attrs: Vec::new(),
        });
    }
    Some(HtmlTag {
        name,
        range,
        attrs: attributes(&inner[name_end..]),
    })
}

fn attributes(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text.trim_start();
    while !rest.is_empty() {
        let key_end = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let key = rest[..key_end].to_ascii_lowercase();
        rest = rest[key_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (raw, remaining) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => match after[1..].find(q) {
                    Some(end) => (&after[1..1 + end], &after[end + 2..]),
                    None => (&after[1..], ""),
                },
                _ => {
                    let end = after
                        .find(|c: char| c.is_whitespace())
                        .unwrap_or(after.len());
                    (&after[..end], &after[end..])
                }
            };
            value = decode_entities(raw);
            rest = remaining;
        }
        if !key.is_empty() {
            out.push((key, value));
        }
        rest = rest.trim_start();
    }
    out
}

fn decode_entities(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://github.com/acme/web/pull/7").unwrap()
    }

    fn only_images(body: &str) -> Vec<ImageRef> {
        images(body, Some(&base()))
    }

    #[test]
    fn markdown_images_carry_alt_and_address() {
        let found = only_images("Before ![Login screen](https://x.test/a.png) after");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].alt, "Login screen");
        assert_eq!(
            found[0].url.as_ref().unwrap().as_str(),
            "https://x.test/a.png"
        );
    }

    #[test]
    fn text_around_an_image_is_kept_in_order() {
        let segs = segments("Before\n\n![a](https://x.test/a.png)\n\nAfter", None);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], Segment::Text("Before".into()));
        assert!(matches!(segs[1], Segment::Image(_)));
        assert_eq!(segs[2], Segment::Text("After".into()));
    }

    #[test]
    fn a_body_without_images_is_one_untouched_segment() {
        let body = "# Title\n\n<!-- note -->\n<a href=\"x\">link</a> and text";
        assert_eq!(segments(body, None), vec![Segment::Text(body.to_string())]);
        assert!(segments("  \n ", None).is_empty());
    }

    #[test]
    fn reference_style_images_resolve() {
        let found = only_images("![Diagram][d]\n\n[d]: https://x.test/d.png");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].alt, "Diagram");
        assert_eq!(
            found[0].url.as_ref().unwrap().as_str(),
            "https://x.test/d.png"
        );
    }

    #[test]
    fn img_tags_read_attributes_in_any_quote_style() {
        let found = only_images(
            r#"<img width="320px" alt='Before &amp; after' height=200 src="https://x.test/p.png?a=1&amp;b=2">"#,
        );
        assert_eq!(found.len(), 1);
        let image = &found[0];
        assert_eq!(image.alt, "Before & after");
        assert_eq!((image.width, image.height), (Some(320), Some(200)));
        assert_eq!(
            image.url.as_ref().unwrap().as_str(),
            "https://x.test/p.png?a=1&b=2"
        );
    }

    #[test]
    fn img_tags_are_found_in_any_case_and_in_html_blocks() {
        let found = only_images(
            "<p align=\"center\">\n  <IMG SRC=\"https://x.test/a.png\" ALT=\"Hi\">\n</p>\n",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].alt, "Hi");
        let segs = segments(
            "<p align=\"center\">\n  <IMG SRC=\"https://x.test/a.png\">\n</p>\n",
            None,
        );
        assert_eq!(segs.len(), 1, "wrapper tags go with the picture: {segs:?}");
    }

    #[test]
    fn images_inside_links_take_the_link_markup_with_them() {
        let segs = segments(
            "See [![Build](https://x.test/b.png)](https://ci.test/run/1) now\n\n<a href=\"https://x.test\"><img src=\"https://x.test/c.png\" alt=\"C\"></a>",
            None,
        );
        let text: Vec<&str> = segs
            .iter()
            .filter_map(|s| match s {
                Segment::Text(t) => Some(t.as_str()),
                Segment::Image(_) => None,
            })
            .collect();
        assert_eq!(text, ["See", "now"]);
        assert_eq!(
            segs.iter()
                .filter(|s| matches!(s, Segment::Image(_)))
                .count(),
            2
        );
    }

    #[test]
    fn a_link_with_words_keeps_its_text() {
        let segs = segments(
            "[the ![icon](https://x.test/i.png) page](https://x.test)",
            None,
        );
        assert!(segs.iter().any(|s| matches!(s, Segment::Image(_))));
        assert!(matches!(&segs[0], Segment::Text(t) if t.starts_with("[the")));
    }

    #[test]
    fn relative_addresses_resolve_against_the_change_page() {
        let found =
            only_images("![a](/acme/web/assets/a.png) ![b](//cdn.test/b.png) ![c](shots/c.png)");
        let urls: Vec<String> = found
            .iter()
            .map(|i| i.url.as_ref().unwrap().to_string())
            .collect();
        assert_eq!(
            urls,
            [
                "https://github.com/acme/web/assets/a.png",
                "https://cdn.test/b.png",
                "https://github.com/acme/web/pull/shots/c.png"
            ]
        );
    }

    #[test]
    fn relative_addresses_without_a_base_and_other_schemes_have_no_url() {
        let found = images(
            "![a](a.png) ![b](data:image/png;base64,AAAA) ![c](file:///etc/passwd) ![d](ftp://x.test/d.png) ![e](https://u:p@x.test/e.png)",
            None,
        );
        assert_eq!(found.len(), 5);
        assert!(found.iter().all(|i| i.url.is_none()), "{found:?}");
    }

    #[test]
    fn github_user_attachment_addresses_are_ordinary_images() {
        let found = only_images(
            "![image](https://github.com/user-attachments/assets/2b1f8a7c-1111-4222-8333-444455556666)",
        );
        assert_eq!(
            found[0].url.as_ref().unwrap().path(),
            "/user-attachments/assets/2b1f8a7c-1111-4222-8333-444455556666"
        );
    }

    #[test]
    fn images_in_code_are_not_images() {
        assert!(only_images(
            "`![a](https://x.test/a.png)`\n\n```\n<img src=\"https://x.test/b.png\">\n```"
        )
        .is_empty());
    }

    #[test]
    fn labels_fall_back_to_the_file_name_then_a_plain_word() {
        let named = &only_images("![](https://x.test/shots/login.png)")[0];
        assert_eq!(named.label(), "login.png");
        let bare = ImageRef {
            src: String::new(),
            url: None,
            alt: "  ".into(),
            width: None,
            height: None,
        };
        assert_eq!(bare.label(), "image");
        let spaced = &only_images("![two\nlines](https://x.test/a.png)")[0];
        assert_eq!(spaced.label(), "two lines");
    }

    #[test]
    fn flatten_replaces_images_with_notes() {
        let text = flatten("Look ![the bug](https://x.test/a.png) here", None);
        assert_eq!(text, "Look\n▣ image: the bug\nhere");
    }
}
