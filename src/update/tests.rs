use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
        Arc,
    },
};

const KEY: &str = include_str!("../../tests/fixtures/update/test.pub");
const NEW: &[u8] = include_bytes!("../../tests/fixtures/update/0.1.0-alpha.6.json");
const NEW_SIG: &[u8] = include_bytes!("../../tests/fixtures/update/0.1.0-alpha.6.json.minisig");
const SAME: &[u8] = include_bytes!("../../tests/fixtures/update/0.1.0-alpha.5.json");
const SAME_SIG: &[u8] = include_bytes!("../../tests/fixtures/update/0.1.0-alpha.5.json.minisig");
const OLD: &[u8] = include_bytes!("../../tests/fixtures/update/0.1.0-alpha.4.json");
const OLD_SIG: &[u8] = include_bytes!("../../tests/fixtures/update/0.1.0-alpha.4.json.minisig");
const NEXT: &[u8] = include_bytes!("../../tests/fixtures/update/0.1.0-alpha.7.json");
const NEXT_SIG: &[u8] = include_bytes!("../../tests/fixtures/update/0.1.0-alpha.7.json.minisig");

struct Server {
    url: String,
    hits: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn start(routes: Vec<(String, u16, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let count = hits.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = vec![];
                loop {
                    let mut byte = [0];
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    bytes.push(byte[0]);
                    if bytes.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or("");
                if path == "/stop" {
                    break;
                }
                count.fetch_add(1, AtomicOrdering::SeqCst);
                let (_, status, body) = routes
                    .iter()
                    .find(|r| r.0 == path)
                    .expect("unexpected request");
                let redirect = if matches!(status, 301 | 302 | 303 | 307 | 308) {
                    format!("Location: {}\r\n", std::str::from_utf8(body).unwrap())
                } else {
                    String::new()
                };
                let header = format!(
                    "HTTP/1.1 {status} Test\r\n{redirect}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(header.as_bytes()).unwrap();
                stream.write_all(body).unwrap();
            }
        });
        Self {
            url,
            hits,
            thread: Some(thread),
        }
    }
    fn signed(body: &[u8], signature: &[u8]) -> Self {
        Self::start(vec![
            ("/latest.json".into(), 200, body.to_vec()),
            ("/latest.json.minisig".into(), 200, signature.to_vec()),
        ])
    }
    fn check(&self) -> Check {
        Check::new("0.1.0-alpha.5", None, Some(self.url.clone()), None).unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let url = Url::parse(&self.url).unwrap();
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", url.port().unwrap())).unwrap();
        stream
            .write_all(b"GET /stop HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn semver_precedence_and_strict_validation() {
    let ordered = [
        "0.1.0-alpha",
        "0.1.0-alpha.1",
        "0.1.0-alpha.9",
        "0.1.0-alpha.10",
        "0.1.0-beta",
        "0.1.0",
        "0.1.1",
        "1.0.0",
    ];
    for pair in ordered.windows(2) {
        assert!(Version::parse(pair[0]).unwrap() < Version::parse(pair[1]).unwrap());
    }
    assert_eq!(
        Version::parse("0.1.0+build.1").unwrap(),
        Version::parse("0.1.0+build.2").unwrap()
    );
    for invalid in [
        "0.1",
        "v0.1.0",
        "01.0.0",
        "0.1.0-01",
        "0.1.0-",
        "0.1.0+",
        "0.1.0+a+b",
        "0.1.0-α",
    ] {
        assert!(Version::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn http_new_version_produces_notification_with_release_action() {
    let server = Server::signed(NEW, NEW_SIG);
    let available = server
        .check()
        .run_with_key(KEY.trim())
        .unwrap()
        .notification
        .unwrap();
    assert_eq!(
        available.notes_url,
        format!("{RELEASES}/tag/v0.1.0-alpha.6")
    );
    let notification = available.notification();
    assert_eq!(notification.id, "update-0.1.0-alpha.6");
    assert!(notification.title.contains("0.1.0-alpha.5 → 0.1.0-alpha.6"));
    assert_eq!(notification.action_label.as_deref(), Some("Ver release"));
    assert!(notification.action.is_some());
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 2);
}

#[test]
fn http_equal_and_downgrade_are_silent() {
    for (body, sig) in [(SAME, SAME_SIG), (OLD, OLD_SIG)] {
        let server = Server::signed(body, sig);
        assert!(server
            .check()
            .run_with_key(KEY.trim())
            .unwrap()
            .notification
            .is_none());
    }
}

#[test]
fn http_signature_is_checked_before_json_and_fails_closed() {
    let server = Server::signed(b"not JSON", NEW_SIG);
    let error = server.check().fetch(KEY.trim()).err().unwrap();
    assert!(
        error.downcast_ref::<minisign_verify::Error>().is_some(),
        "{error:?}"
    );
    assert!(server.check().run_with_key(KEY.trim()).is_none());
    let server = Server::signed(NEW, SAME_SIG);
    assert!(server.check().run_with_key(KEY.trim()).is_none());
    let server = Server::signed(NEW, NEW_SIG);
    assert!(
        server.check().run().is_none(),
        "production placeholder must never trust test key"
    );
    let mut altered = NEW.to_vec();
    altered.push(b' ');
    let server = Server::signed(&altered, NEW_SIG);
    assert!(server.check().run_with_key(KEY.trim()).is_none());
}

#[test]
fn http_signed_invalid_json_version_and_missing_assets_are_silent() {
    for (body, sig) in [
        (
            &include_bytes!("../../tests/fixtures/update/invalid.json")[..],
            &include_bytes!("../../tests/fixtures/update/invalid.json.minisig")[..],
        ),
        (
            &include_bytes!("../../tests/fixtures/update/invalid-version.json")[..],
            &include_bytes!("../../tests/fixtures/update/invalid-version.json.minisig")[..],
        ),
        (
            &include_bytes!("../../tests/fixtures/update/missing-assets.json")[..],
            &include_bytes!("../../tests/fixtures/update/missing-assets.json.minisig")[..],
        ),
    ] {
        let server = Server::signed(body, sig);
        assert!(server.check().run_with_key(KEY.trim()).is_none());
    }
}

#[test]
fn http_404_rate_limits_missing_signature_and_oversize_are_silent() {
    for status in [404, 403, 429] {
        let server = Server::start(vec![("/latest.json".into(), status, vec![])]);
        assert!(server.check().run_with_key(KEY.trim()).is_none());
        assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 1);
    }
    let server = Server::start(vec![
        ("/latest.json".into(), 200, NEW.to_vec()),
        ("/latest.json.minisig".into(), 404, vec![]),
    ]);
    assert!(server.check().run_with_key(KEY.trim()).is_none());
    let server = Server::signed(&vec![b' '; MANIFEST_LIMIT as usize + 1], NEW_SIG);
    assert!(server.check().run_with_key(KEY.trim()).is_none());
}

#[test]
fn offline_check_runs_on_worker_and_returns_silently() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/latest.json", listener.local_addr().unwrap());
    drop(listener);
    let check = Check::new("0.1.0-alpha.5", None, Some(url), None).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        sender
            .send(check.run_with_key(KEY.trim()).is_none())
            .unwrap()
    });
    assert!(receiver.recv_timeout(Duration::from_secs(5)).unwrap());
    worker.join().unwrap();
}

#[test]
fn daily_attempt_is_persisted_including_failure_and_future_clock() {
    let server = Server::signed(NEW, NEW_SIG);
    let mut preferences = crate::preferences::Preferences::default();
    let now = 1_800_000_000;
    for _ in 0..2 {
        if reserve_attempt(&mut preferences.last_update_check, now, 0) {
            assert!(server
                .check()
                .run_with_key(KEY.trim())
                .unwrap()
                .notification
                .is_some());
        }
        preferences = serde_json::from_slice(&serde_json::to_vec(&preferences).unwrap()).unwrap();
    }
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 2);
    assert_eq!(preferences.last_update_check, Some(now));
    assert!(!reserve_attempt(
        &mut preferences.last_update_check,
        now - 1,
        0
    ));
    assert!(!reserve_attempt(
        &mut preferences.last_update_check,
        now + 86399,
        0
    ));
    assert!(reserve_attempt(
        &mut preferences.last_update_check,
        now + 86400,
        0
    ));
    let server = Server::start(vec![("/latest.json".into(), 429, vec![])]);
    let mut last = None;
    for _ in 0..2 {
        if reserve_attempt(&mut last, now, 0) {
            assert!(server.check().run_with_key(KEY.trim()).is_none());
        }
    }
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 1);
}

#[test]
fn dismissal_survives_disk_and_newer_version_reappears() {
    let root = std::env::temp_dir().join(format!("dish-update-dismiss-{}", std::process::id()));
    let path = root.join("state.json");
    let mut prefs = crate::preferences::Preferences::default();
    let mut queue = crate::notifications::NotificationQueue::default();
    let server = Server::signed(NEW, NEW_SIG);
    let notification = || {
        server
            .check()
            .run_with_key(KEY.trim())
            .unwrap()
            .notification
            .unwrap()
            .notification()
    };
    assert!(queue.push(notification(), &prefs.dismissed_notifications));
    assert!(queue.dismiss("update-0.1.0-alpha.6"));
    prefs
        .dismissed_notifications
        .insert("update-0.1.0-alpha.6".into());
    crate::fs_security::write_atomic(&path, &serde_json::to_vec(&prefs).unwrap()).unwrap();
    let restored: crate::preferences::Preferences =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!queue.push(notification(), &restored.dismissed_notifications));
    let next = Server::signed(NEXT, NEXT_SIG);
    assert!(queue.push(
        next.check()
            .run_with_key(KEY.trim())
            .unwrap()
            .notification
            .unwrap()
            .notification(),
        &restored.dismissed_notifications
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_uses_semver_channel_and_never_falls_back_from_missing_manifest() {
    let releases: Vec<Release> = serde_json::from_value(serde_json::json!([
        {"tag_name":"v0.1.0-alpha.9", "prerelease":true, "draft":false, "assets":[]},
        {"tag_name":"v0.1.0-alpha.10", "prerelease":true, "draft":false, "assets":[]},
        {"tag_name":"v99.0.0", "prerelease":false, "draft":true, "assets":[]},
        {"tag_name":"invalid", "prerelease":false, "draft":false, "assets":[]}
    ]))
    .unwrap();
    let current = Version::parse("0.1.0-alpha.5").unwrap();
    let selected = select_release(&releases, &current, UpdateChannel::Prerelease).unwrap();
    assert_eq!(selected.tag_name, "v0.1.0-alpha.10");
    assert!(selected.assets.is_empty());
    assert!(select_release(&releases, &current, UpdateChannel::Stable).is_none());
}

#[test]
fn manifest_schema_and_url_validation() {
    let http = Http::new(None).unwrap();
    let original: serde_json::Value = serde_json::from_slice(NEW).unwrap();
    for (field, value) in [
        ("schema", serde_json::json!(2)),
        ("tag", serde_json::json!("v0.1.0")),
        ("channel", serde_json::json!("stable")),
        ("target", serde_json::json!("windows")),
        ("notes_url", serde_json::json!("https://evil.example")),
        ("minimum_glibc", serde_json::json!("2.x")),
        ("published_at", serde_json::json!("yesterday")),
    ] {
        let mut value_manifest = original.clone();
        value_manifest[field] = value;
        assert!(
            serde_json::from_value::<Manifest>(value_manifest)
                .unwrap()
                .validate(&http)
                .is_err(),
            "{field}"
        );
    }
    let mut duplicate = original.clone();
    let asset = duplicate["assets"][0].clone();
    duplicate["assets"].as_array_mut().unwrap().push(asset);
    assert!(serde_json::from_value::<Manifest>(duplicate)
        .unwrap()
        .validate(&http)
        .is_err());
    for invalid in [
        "http://github.com/a",
        "https://evil.example/a",
        "https://user:secret@github.com/a",
        "file:///tmp/a",
    ] {
        assert!(http.validate_url(&Url::parse(invalid).unwrap()).is_err());
    }
    assert!(Http::new(Some("http://evil.example/latest.json")).is_err());
    assert!(Http::new(Some("http://127.0.0.1:1234/latest.json")).is_ok());
}

#[test]
fn http_discovery_downloads_signed_manifest_and_refuses_partial_release() {
    // Construct URLs before the worker starts so discovery also uses loopback.
    // Server routes are immutable; its address is supplied through a second local server.
    let assets = Server::signed(NEW, NEW_SIG);
    let manifest: Manifest = serde_json::from_slice(NEW).unwrap();
    let binary = &manifest.assets[0];
    let mut release = serde_json::json!({"tag_name":"v0.1.0-alpha.6", "draft":false, "prerelease":true, "assets":[
        {"name":"latest.json", "browser_download_url":assets.url, "size":NEW.len()},
        {"name":"latest.json.minisig", "browser_download_url":format!("{}.minisig", assets.url), "size":NEW_SIG.len()},
        {"name":binary.name, "browser_download_url":binary.url, "size":binary.size}
    ]});
    let check = Check::new("0.1.0-alpha.5", None, None, None).unwrap();
    let http = Http::new(Some(&assets.url)).unwrap();
    let discovery = Server::start(vec![(
        "/latest.json".into(),
        200,
        serde_json::to_vec(&vec![release.clone()]).unwrap(),
    )]);
    assert!(check
        .fetch_from(KEY.trim(), &http, &discovery.url)
        .unwrap()
        .notification
        .is_some());
    assert_eq!(
        assets.hits.load(AtomicOrdering::SeqCst),
        2,
        "no binary download"
    );
    release["assets"].as_array_mut().unwrap().pop();
    let discovery = Server::start(vec![(
        "/latest.json".into(),
        200,
        serde_json::to_vec(&vec![release.clone()]).unwrap(),
    )]);
    assert!(check.fetch_from(KEY.trim(), &http, &discovery.url).is_err());
    release["assets"].as_array_mut().unwrap().remove(1);
    let discovery = Server::start(vec![(
        "/latest.json".into(),
        200,
        serde_json::to_vec(&vec![release]).unwrap(),
    )]);
    let hits = assets.hits.load(AtomicOrdering::SeqCst);
    assert!(check.fetch_from(KEY.trim(), &http, &discovery.url).is_err());
    assert_eq!(
        assets.hits.load(AtomicOrdering::SeqCst),
        hits,
        "missing signature fails before downloads"
    );
}

#[test]
fn http_not_modified_is_silent() {
    let server = Server::start(vec![("/latest.json".into(), 304, vec![])]);
    let check = Check::new("0.1.0-alpha.5", None, None, Some("\"previous\"".into())).unwrap();
    let http = Http::new(Some(&server.url)).unwrap();
    let outcome = check.fetch_from(KEY.trim(), &http, &server.url).unwrap();
    assert!(outcome.notification.is_none());
    assert_eq!(outcome.etag.as_deref(), Some("\"previous\""));
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 1);
}

#[test]
fn signed_fixtures_verify_without_network_and_tampering_fails() {
    let http = Http::new(None).unwrap();
    for (body, signature, expected_order) in [
        (NEW, NEW_SIG, Ordering::Greater),
        (SAME, SAME_SIG, Ordering::Equal),
        (OLD, OLD_SIG, Ordering::Less),
    ] {
        let (_, version) = Manifest::verify_parse(body, signature, KEY.trim(), &http).unwrap();
        assert_eq!(
            version.cmp(&Version::parse("0.1.0-alpha.5").unwrap()),
            expected_order
        );
    }
    let mut altered = NEW.to_vec();
    altered.push(b' ');
    assert!(Manifest::verify_parse(&altered, NEW_SIG, KEY.trim(), &http).is_err());
    assert!(Manifest::verify_parse(NEW, SAME_SIG, KEY.trim(), &http).is_err());
    assert!(Manifest::verify_parse(NEW, b"", KEY.trim(), &http).is_err());
    assert!(Manifest::verify_parse(NEW, NEW_SIG, UPDATE_PUBLIC_KEY, &http).is_err());
    let error = Manifest::verify_parse(b"not json", NEW_SIG, KEY.trim(), &http)
        .err()
        .unwrap();
    assert!(error.downcast_ref::<minisign_verify::Error>().is_some());
    let error = Manifest::verify_parse(
        include_bytes!("../../tests/fixtures/update/invalid.json"),
        include_bytes!("../../tests/fixtures/update/invalid.json.minisig"),
        KEY.trim(),
        &http,
    )
    .err()
    .unwrap();
    assert!(
        error.downcast_ref::<serde_json::Error>().is_some(),
        "valid signature reaches JSON parser"
    );
}

#[test]
fn daily_cadence_and_jitter_boundaries_without_network() {
    let mut last = None;
    assert!(reserve_attempt(&mut last, 100_000, 42));
    assert!(!reserve_attempt(&mut last, 100_000, 42));
    assert!(!reserve_attempt(&mut last, 99_999, 42));
    assert!(!reserve_attempt(&mut last, 100_000 + 86400, 42));
    assert!(reserve_attempt(&mut last, 100_000 + 86442, 42));
    assert!((0..=21600).contains(&daily_jitter()));
    assert_eq!(daily_jitter(), daily_jitter());
}

#[test]
fn http_redirects_enforce_allowlist_and_limit() {
    let server = Server::start(vec![(
        "/latest.json".into(),
        302,
        b"https://untrusted.invalid/manifest".to_vec(),
    )]);
    assert!(server.check().run_with_key(KEY.trim()).is_none());
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 1);
    let server = Server::start(vec![("/latest.json".into(), 302, b"/latest.json".to_vec())]);
    assert!(server.check().run_with_key(KEY.trim()).is_none());
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 6);
    let server = Server::start(vec![
        ("/latest.json".into(), 302, b"/payload".to_vec()),
        ("/payload".into(), 200, NEW.to_vec()),
        ("/latest.json.minisig".into(), 200, NEW_SIG.to_vec()),
    ]);
    assert!(server
        .check()
        .run_with_key(KEY.trim())
        .unwrap()
        .notification
        .is_some());
    assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 3);
}

#[test]
fn valid_but_wrong_public_key_is_rejected() {
    let http = Http::new(None).unwrap();
    let wrong = include_str!("../../tests/fixtures/update/wrong.pub").trim();
    let error = Manifest::verify_parse(NEW, NEW_SIG, wrong, &http)
        .err()
        .unwrap();
    assert!(error.downcast_ref::<minisign_verify::Error>().is_some());
}

#[test]
fn notification_action_and_persisted_dismissal_without_network() {
    let http = Http::new(None).unwrap();
    let make_notification = |body, sig| {
        let (manifest, _) = Manifest::verify_parse(body, sig, KEY.trim(), &http).unwrap();
        Available {
            current: "0.1.0-alpha.5".into(),
            version: manifest.version,
            notes_url: manifest.notes_url,
        }
        .notification()
    };
    let notification = make_notification(NEW, NEW_SIG);
    assert!(notification.title.contains("0.1.0-alpha.5 → 0.1.0-alpha.6"));
    assert_eq!(notification.action_label.as_deref(), Some("Ver release"));
    assert!(notification.action.is_some());
    let mut queue = crate::notifications::NotificationQueue::default();
    let mut prefs = crate::preferences::Preferences::default();
    assert!(queue.push(notification, &prefs.dismissed_notifications));
    assert!(queue.dismiss("update-0.1.0-alpha.6"));
    prefs
        .dismissed_notifications
        .insert("update-0.1.0-alpha.6".into());
    let root =
        std::env::temp_dir().join(format!("dish-update-pure-dismiss-{}", std::process::id()));
    let path = root.join("state.json");
    crate::fs_security::write_atomic(&path, &serde_json::to_vec(&prefs).unwrap()).unwrap();
    let restored: crate::preferences::Preferences =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(!queue.push(
        make_notification(NEW, NEW_SIG),
        &restored.dismissed_notifications
    ));
    assert!(queue.push(
        make_notification(NEXT, NEXT_SIG),
        &restored.dismissed_notifications
    ));
    std::fs::remove_dir_all(root).unwrap();
}
