//! Fetching a description's pictures through the live backend, against a stub server: the
//! limits, the disk cache (including offline), and `forge-only`. No real network.
#![cfg(feature = "live")]

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
use rb_core::SourceId;
use rb_platform::{CommandOutput, CommandRunner, MemorySecretStore, PlatformError};
use review_buddy::app::{Cmd, Msg};
use review_buddy::config::Config;
use review_buddy::images::cache::{DiskCache, DEFAULT_CAP};
use review_buddy::images::Failure;
use review_buddy::providers::{Deps, Factory, ImageSettings, Live};
use review_buddy::runtime::{execute, Backend, Platform};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct NoCli;

impl CommandRunner for NoCli {
    fn run(
        &self,
        program: &str,
        _: &[&str],
        _: Option<&[u8]>,
    ) -> Result<CommandOutput, PlatformError> {
        Err(PlatformError::CommandFailed {
            program: program.into(),
        })
    }
}

fn png() -> Vec<u8> {
    let img = ImageBuffer::from_fn(6, 4, |x, y| Rgb([(x * 40) as u8, (y * 60) as u8, 120]));
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img)
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn live(server: &MockServer, cache: Option<DiskCache>, forge_only: bool) -> Arc<Live> {
    let config: Config = toml::from_str(&format!(
        r#"
[[source]]
name = "work"
kind = "github"
host = "ghe.test"
api_url = "{}"
auth = "env:RB_STUB_TOKEN"
"#,
        server.uri()
    ))
    .unwrap();
    let env: HashMap<String, String> =
        [("RB_STUB_TOKEN".to_string(), "ghp_stub".to_string())].into();
    let deps = Deps {
        runner: Arc::new(NoCli),
        store: Arc::new(MemorySecretStore::new()),
        getenv: Arc::new(move |k| env.get(k).cloned()),
    };
    let factory = Arc::new(Factory::from_config(&config, deps));
    let sources = factory.sources();
    Arc::new(Live::new(factory, sources, None, 4).with_images(ImageSettings { cache, forge_only }))
}

async fn fetch(
    live: &Arc<Live>,
    url: &str,
    tx: &UnboundedSender<Msg>,
    rx: &mut UnboundedReceiver<Msg>,
) -> Result<(u32, u32), Failure> {
    let cmd = Cmd::FetchImage {
        source: SourceId::new("work"),
        url: url.to_string(),
    };
    execute(
        cmd,
        tx,
        &Backend::Live(Arc::clone(live)),
        &Platform::system(),
    );
    let msg = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("an answer arrives")
        .expect("channel open");
    match msg {
        Msg::ImageLoaded {
            url: answered,
            result,
        } => {
            assert_eq!(answered, url);
            result.map(|img| (img.width(), img.height()))
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_picture_is_fetched_decoded_cached_and_shown_again_offline() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/shots/a.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(png()))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let cache = DiskCache::new(dir.path().join("images"), DEFAULT_CAP);
    let live = live(&server, Some(cache), false);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let url = format!("{}/shots/a.png", server.uri());

    assert_eq!(fetch(&live, &url, &tx, &mut rx).await, Ok((6, 4)));
    assert_eq!(
        fetch(&live, &url, &tx, &mut rx).await,
        Ok((6, 4)),
        "the second one is read from disk"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    drop(server);
    assert_eq!(
        fetch(&live, &url, &tx, &mut rx).await,
        Ok((6, 4)),
        "and it still shows with the server gone"
    );
}

#[tokio::test]
async fn bad_bytes_are_refused_by_what_they_are_not_what_they_are_called() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/logo.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "image/png")
                .set_body_bytes(b"<svg xmlns='http://www.w3.org/2000/svg'></svg>".to_vec()),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/page.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "image/png")
                .set_body_bytes(b"<html>sign in</html>".to_vec()),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/huge.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 10 * 1024 * 1024 + 1]))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/missing.png"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let live = live(&server, None, false);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let at = |tail: &str| format!("{}{tail}", server.uri());

    assert_eq!(
        fetch(&live, &at("/logo.png"), &tx, &mut rx).await,
        Err(Failure::Svg)
    );
    assert_eq!(
        fetch(&live, &at("/page.png"), &tx, &mut rx).await,
        Err(Failure::Unsupported)
    );
    assert_eq!(
        fetch(&live, &at("/huge.png"), &tx, &mut rx).await,
        Err(Failure::TooLarge)
    );
    assert_eq!(
        fetch(&live, &at("/missing.png"), &tx, &mut rx).await,
        Err(Failure::Status(404))
    );
}

#[tokio::test]
async fn forge_only_never_contacts_a_third_party() {
    let server = MockServer::start().await;
    let live = live(&server, None, true);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let result = fetch(
        &live,
        "https://tracker.example.invalid/pixel.png",
        &tx,
        &mut rx,
    )
    .await;
    assert_eq!(
        result,
        Err(Failure::External("tracker.example.invalid".into()))
    );
}

#[tokio::test]
async fn the_token_is_not_sent_over_plain_http_even_to_the_forge() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/a.png"))
        .respond_with(|req: &wiremock::Request| {
            assert!(
                !req.headers.contains_key("authorization"),
                "token sent over http"
            );
            ResponseTemplate::new(200).set_body_bytes(png())
        })
        .mount(&server)
        .await;
    let live = live(&server, None, false);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let url = format!("{}/a.png", server.uri());
    assert_eq!(fetch(&live, &url, &tx, &mut rx).await, Ok((6, 4)));
}

#[tokio::test]
async fn an_unknown_source_is_a_calm_failure() {
    let server = MockServer::start().await;
    let live = live(&server, None, false);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cmd = Cmd::FetchImage {
        source: SourceId::new("nope"),
        url: format!("{}/a.png", server.uri()),
    };
    execute(cmd, &tx, &Backend::Live(live), &Platform::system());
    let Some(Msg::ImageLoaded { result, .. }) = rx.recv().await else {
        panic!("an answer arrives");
    };
    assert_eq!(result.unwrap_err(), Failure::NoSource);
}
