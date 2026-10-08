//! Fetching and decoding one remote image, with hard limits.
//!
//! Redirects are followed by hand, a few hops at most, and the rules are applied again at every
//! hop: the token goes only to the forge's own web and API hosts, `forge-only` refuses every
//! other host, and addresses that point at this machine or a private network are refused for
//! third parties. The body is read in chunks and abandoned the moment it passes the size cap.
//! Nothing here logs, and messages name a host, never a path or query.

use std::io::Cursor;
use std::time::Duration;

use image::{DynamicImage, ImageFormat, ImageReader};
use rb_platform::Secret;
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION, LOCATION};
use url::Url;

use super::hosts::{host_label, ForgeHosts, Trust};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_redirects: usize,
    /// The longest side, in pixels, that is decoded at all.
    pub max_side: u32,
    pub max_pixels: u64,
    pub timeout: Duration,
    /// Decoded images are shrunk to fit this many pixels on a side before they are kept.
    pub keep_side: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 10 * 1024 * 1024,
            max_redirects: 4,
            max_side: 8192,
            max_pixels: 40_000_000,
            timeout: Duration::from_secs(20),
            keep_side: 1280,
        }
    }
}

/// Why an image isn't shown. Each one has a calm sentence for the placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// `forge-only` is on and the image lives somewhere else.
    External(String),
    /// The address points at this machine or a private network.
    Local,
    NotWeb,
    TooLarge,
    TooManyRedirects,
    Status(u16),
    Network,
    Timeout,
    Svg,
    Unsupported,
    Dimensions,
    Decode,
    NoSource,
}

impl Failure {
    pub fn reason(&self) -> String {
        match self {
            Self::External(host) => format!(
                "not loaded: hosted on {host}, outside your forge (ui.images = \"forge-only\")"
            ),
            Self::Local => "not loaded: it points at this machine or a private network".into(),
            Self::NotWeb => "not loaded: it isn't a web address".into(),
            Self::TooLarge => "too large to show (over 10 MB)".into(),
            Self::TooManyRedirects => "redirected too many times".into(),
            Self::Status(401 | 403) => "your sign-in can't see it".into(),
            Self::Status(404) => "not found".into(),
            Self::Status(code) => format!("the server answered {code}"),
            Self::Network => "couldn't be reached".into(),
            Self::Timeout => "took too long to load".into(),
            Self::Svg => "SVG images aren't drawn".into(),
            Self::Unsupported => "not a PNG, JPEG, GIF or WebP image".into(),
            Self::Dimensions => "too many pixels to show".into(),
            Self::Decode => "couldn't be decoded".into(),
            Self::NoSource => "no source is connected".into(),
        }
    }
}

/// How a token is presented to the forge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStyle {
    Bearer,
    PrivateToken,
}

#[derive(Clone)]
pub struct Token {
    pub secret: Secret,
    pub style: TokenStyle,
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

pub struct Fetcher {
    http: reqwest::Client,
    hosts: ForgeHosts,
    token: Option<Token>,
    forge_only: bool,
    limits: Limits,
}

impl Fetcher {
    pub fn new(
        hosts: ForgeHosts,
        token: Option<Token>,
        forge_only: bool,
        limits: Limits,
    ) -> Result<Self, Failure> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(limits.timeout)
            .connect_timeout(Duration::from_secs(8))
            .user_agent(concat!("review-buddy/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Failure::Network)?;
        Ok(Self {
            http,
            hosts,
            token,
            forge_only,
            limits,
        })
    }

    /// The raw bytes of the image at `start`, within the limits.
    pub async fn fetch(&self, start: &Url) -> Result<Vec<u8>, Failure> {
        let mut url = start.clone();
        for _ in 0..=self.limits.max_redirects {
            self.check(&url)?;
            let mut request = self.http.get(url.clone()).header(ACCEPT, "image/*");
            if let (Some(token), true) = (&self.token, self.hosts.sends_token(&url)) {
                request = match token.style {
                    TokenStyle::Bearer => {
                        let value = format!("Bearer {}", token.secret.expose());
                        request.header(AUTHORIZATION, sensitive(&value)?)
                    }
                    TokenStyle::PrivateToken => {
                        request.header("PRIVATE-TOKEN", sensitive(token.secret.expose())?)
                    }
                };
            }
            let mut response = request.send().await.map_err(|e| {
                if e.is_timeout() {
                    Failure::Timeout
                } else {
                    Failure::Network
                }
            })?;
            let status = response.status();
            if status.is_redirection() {
                let next = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|location| url.join(location).ok())
                    .ok_or(Failure::Network)?;
                url = next;
                continue;
            }
            if !status.is_success() {
                return Err(Failure::Status(status.as_u16()));
            }
            if response
                .content_length()
                .is_some_and(|n| n > self.limits.max_bytes as u64)
            {
                return Err(Failure::TooLarge);
            }
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| {
                if e.is_timeout() {
                    Failure::Timeout
                } else {
                    Failure::Network
                }
            })? {
                if body.len() + chunk.len() > self.limits.max_bytes {
                    return Err(Failure::TooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            return Ok(body);
        }
        Err(Failure::TooManyRedirects)
    }

    /// The rules every hop must pass.
    fn check(&self, url: &Url) -> Result<(), Failure> {
        let web = matches!(url.scheme(), "http" | "https");
        if !web
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(Failure::NotWeb);
        }
        let trust = self.hosts.trust(url);
        if self.forge_only && trust != Trust::Forge {
            return Err(Failure::External(host_label(url)));
        }
        if trust != Trust::Forge && self.hosts.is_local(url) {
            return Err(Failure::Local);
        }
        Ok(())
    }
}

fn sensitive(value: &str) -> Result<HeaderValue, Failure> {
    let mut header = HeaderValue::from_str(value).map_err(|_| Failure::Network)?;
    header.set_sensitive(true);
    Ok(header)
}

/// What the bytes are, from their first bytes and never from a file name or a header.
pub fn sniff(bytes: &[u8]) -> Result<ImageFormat, Failure> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Ok(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Ok(ImageFormat::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok(ImageFormat::WebP)
    } else if looks_like_svg(bytes) {
        Err(Failure::Svg)
    } else {
        Err(Failure::Unsupported)
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(2048)];
    let text = String::from_utf8_lossy(head);
    let text = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .to_ascii_lowercase();
    text.starts_with("<svg")
        || ((text.starts_with("<?xml") || text.starts_with("<!doctype svg"))
            && text.contains("<svg"))
}

/// Decodes `bytes` within the pixel limits and shrinks the result to `keep_side`.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<DynamicImage, Failure> {
    let format = sniff(bytes)?;
    let reader = || ImageReader::with_format(Cursor::new(bytes), format);
    let (width, height) = reader().into_dimensions().map_err(|_| Failure::Decode)?;
    if width == 0
        || height == 0
        || width.max(height) > limits.max_side
        || u64::from(width) * u64::from(height) > limits.max_pixels
    {
        return Err(Failure::Dimensions);
    }
    let mut decoder = reader();
    let mut caps = image::Limits::default();
    caps.max_image_width = Some(limits.max_side);
    caps.max_image_height = Some(limits.max_side);
    caps.max_alloc = Some(256 * 1024 * 1024);
    decoder.limits(caps);
    let decoded = decoder.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => Failure::Dimensions,
        _ => Failure::Decode,
    })?;
    Ok(
        if decoded.width().max(decoded.height()) > limits.keep_side {
            decoded.thumbnail(limits.keep_side, limits.keep_side)
        } else {
            decoded
        },
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{ImageBuffer, Rgb};
    use wiremock::matchers::{header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = ImageBuffer::from_fn(width, height, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        });
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    fn local_hosts(server: &MockServer) -> ForgeHosts {
        let _ = server;
        ForgeHosts::insecure_for_tests(&["127.0.0.1"])
    }

    fn source() -> rb_core::Source {
        rb_core::Source {
            id: rb_core::SourceId::new("s"),
            kind: rb_core::ForgeKind::GitHub,
            host: "github.com".into(),
            label: "s".into(),
            scope: rb_core::Scope::everything(),
            auth: rb_core::AuthMode::Cli,
            in_all: true,
            include_drafts: true,
            tag_colour: None,
        }
    }

    fn token() -> Token {
        Token {
            secret: Secret::new("sekrit-token"),
            style: TokenStyle::Bearer,
        }
    }

    fn fetcher(hosts: ForgeHosts, forge_only: bool) -> Fetcher {
        Fetcher::new(hosts, Some(token()), forge_only, Limits::default()).unwrap()
    }

    fn url(server: &MockServer, tail: &str) -> Url {
        Url::parse(&format!("{}{tail}", server.uri())).unwrap()
    }

    #[test]
    fn sniffing_trusts_bytes_not_names() {
        assert_eq!(sniff(&png(2, 2)).unwrap(), ImageFormat::Png);
        assert_eq!(
            sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0]).unwrap(),
            ImageFormat::Jpeg
        );
        assert_eq!(sniff(b"GIF89a....").unwrap(), ImageFormat::Gif);
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 ").unwrap(), ImageFormat::WebP);
        assert_eq!(
            sniff(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
            Err(Failure::Svg)
        );
        assert_eq!(
            sniff(b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<svg></svg>"),
            Err(Failure::Svg)
        );
        assert_eq!(
            sniff(b"<html><body>no</body></html>"),
            Err(Failure::Unsupported)
        );
        assert_eq!(sniff(b""), Err(Failure::Unsupported));
    }

    #[test]
    fn decoding_enforces_pixel_limits_and_shrinks_what_it_keeps() {
        let limits = Limits {
            max_side: 100,
            keep_side: 40,
            ..Limits::default()
        };
        assert_eq!(
            decode(&png(101, 5), &limits).unwrap_err(),
            Failure::Dimensions
        );
        let kept = decode(&png(100, 50), &limits).unwrap();
        assert_eq!((kept.width(), kept.height()), (40, 20));
        let small = decode(&png(10, 10), &limits).unwrap();
        assert_eq!(small.width(), 10);
        let pixels = Limits {
            max_pixels: 99,
            ..Limits::default()
        };
        assert_eq!(
            decode(&png(10, 10), &pixels).unwrap_err(),
            Failure::Dimensions
        );
        assert_eq!(
            decode(b"\x89PNG\r\n\x1a\ntruncated", &Limits::default()).unwrap_err(),
            Failure::Decode
        );
    }

    #[tokio::test]
    async fn fetches_bytes_and_sends_the_token_to_a_forge_host() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a.png"))
            .and(header("authorization", "Bearer sekrit-token"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png(4, 4)))
            .mount(&server)
            .await;
        let bytes = fetcher(local_hosts(&server), false)
            .fetch(&url(&server, "/a.png"))
            .await
            .unwrap();
        assert_eq!(sniff(&bytes).unwrap(), ImageFormat::Png);
    }

    #[tokio::test]
    async fn a_third_party_host_is_fetched_anonymously() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a.png"))
            .respond_with(|req: &wiremock::Request| {
                assert!(!req.headers.contains_key("authorization"), "token leaked");
                assert!(!req.headers.contains_key("private-token"), "token leaked");
                ResponseTemplate::new(200).set_body_bytes(png(2, 2))
            })
            .mount(&server)
            .await;
        // The mock server is on 127.0.0.1, which these hosts do not own.
        let hosts = ForgeHosts::insecure_for_tests(&["github.com"]);
        let result = fetcher(hosts, false).fetch(&url(&server, "/a.png")).await;
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn the_token_is_dropped_when_a_redirect_leaves_the_forge() {
        let other = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/final.png"))
            .respond_with(|req: &wiremock::Request| {
                assert!(
                    !req.headers.contains_key("authorization"),
                    "token followed a redirect"
                );
                ResponseTemplate::new(200).set_body_bytes(png(2, 2))
            })
            .mount(&other)
            .await;
        let forge = MockServer::start().await;
        // `localhost` stands in for a different host name on the same machine.
        let target = format!("http://localhost:{}/final.png", other.address().port());
        Mock::given(method("GET"))
            .and(path("/start.png"))
            .and(header_exists("authorization"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", target.as_str()))
            .mount(&forge)
            .await;
        let hosts = ForgeHosts::insecure_for_tests(&["127.0.0.1"]);
        let result = fetcher(hosts, false)
            .fetch(&url(&forge, "/start.png"))
            .await;
        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn forge_only_refuses_other_hosts_including_after_a_redirect() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/start.png"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", "https://example.com/x.png?sig=abc"),
            )
            .mount(&server)
            .await;
        let result = fetcher(local_hosts(&server), true)
            .fetch(&url(&server, "/start.png"))
            .await;
        assert_eq!(result, Err(Failure::External("example.com".into())));
        let direct = fetcher(local_hosts(&server), true)
            .fetch(&Url::parse("https://example.org/a.png").unwrap())
            .await;
        assert_eq!(direct, Err(Failure::External("example.org".into())));
    }

    #[tokio::test]
    async fn redirect_loops_stop() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/again.png"))
            .mount(&server)
            .await;
        let result = fetcher(local_hosts(&server), false)
            .fetch(&url(&server, "/a.png"))
            .await;
        assert_eq!(result, Err(Failure::TooManyRedirects));
    }

    #[tokio::test]
    async fn oversize_bodies_are_refused_by_header_and_by_count() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/big.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 2048]))
            .mount(&server)
            .await;
        let limits = Limits {
            max_bytes: 1024,
            ..Limits::default()
        };
        let fetcher = Fetcher::new(local_hosts(&server), None, false, limits).unwrap();
        assert_eq!(
            fetcher.fetch(&url(&server, "/big.png")).await,
            Err(Failure::TooLarge)
        );
    }

    #[tokio::test]
    async fn statuses_become_calm_failures() {
        let server = MockServer::start().await;
        for (tail, code) in [
            ("/gone.png", 404),
            ("/private.png", 403),
            ("/boom.png", 500),
        ] {
            Mock::given(method("GET"))
                .and(path(tail))
                .respond_with(ResponseTemplate::new(code))
                .mount(&server)
                .await;
        }
        let f = fetcher(local_hosts(&server), false);
        assert_eq!(
            f.fetch(&url(&server, "/gone.png")).await,
            Err(Failure::Status(404))
        );
        assert_eq!(
            f.fetch(&url(&server, "/private.png"))
                .await
                .unwrap_err()
                .reason(),
            "your sign-in can't see it"
        );
        assert_eq!(
            f.fetch(&url(&server, "/boom.png"))
                .await
                .unwrap_err()
                .reason(),
            "the server answered 500"
        );
    }

    #[tokio::test]
    async fn private_networks_and_other_schemes_are_refused() {
        let plain = Fetcher::new(
            ForgeHosts::for_source(&source(), None),
            None,
            false,
            Limits::default(),
        )
        .unwrap();
        for local in [
            "http://127.0.0.1:9/a.png",
            "http://169.254.169.254/latest/meta-data",
            "http://localhost/a.png",
        ] {
            assert_eq!(
                plain.fetch(&Url::parse(local).unwrap()).await,
                Err(Failure::Local),
                "{local}"
            );
        }
        assert_eq!(
            plain
                .fetch(&Url::parse("ftp://example.com/a.png").unwrap())
                .await,
            Err(Failure::NotWeb)
        );
    }

    #[test]
    fn the_token_stays_out_of_debug_output() {
        assert!(!format!("{:?}", token()).contains("sekrit"));
    }
}
