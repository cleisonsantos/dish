//! Optional Pi updates: discover the npm release, compare it with the
//! installed executable and hand installation to Pi's official installer.
//!
//! Dish never writes Pi's files. The only action here is running the same
//! installer the setup screen already uses.
use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::Deserialize;
use std::cmp::Ordering;
use std::time::Duration;
use url::Url;

pub const PACKAGE: &str = crate::installation::PACKAGE;
pub const REGISTRY: &str = "https://registry.npmjs.org/@earendil-works%2Fpi-coding-agent/latest";
const RESPONSE_LIMIT: u64 = 64 * 1024;
const CHECK_DAILY_SECONDS: i64 = 24 * 60 * 60;

/// SemVer precedence, deliberately excluding build metadata from comparison.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Version {
    core: [String; 3],
    pre: Vec<String>,
}

impl Version {
    pub fn parse(input: &str) -> Result<Self> {
        fn identifiers(value: &str, numeric_rule: bool) -> bool {
            value.split('.').all(|part| {
                !part.is_empty()
                    && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    && !(numeric_rule
                        && part.bytes().all(|byte| byte.is_ascii_digit())
                        && part.len() > 1
                        && part.starts_with('0'))
            })
        }

        let mut parts = input.split('+');
        let main = parts.next().unwrap_or_default();
        if let Some(build) = parts.next() {
            ensure!(identifiers(build, false), "invalid build metadata");
        }
        ensure!(parts.next().is_none(), "invalid version");
        let (core, pre) = main.split_once('-').unwrap_or((main, ""));
        let core: Vec<_> = core.split('.').map(str::to_owned).collect();
        ensure!(
            core.len() == 3
                && core.iter().all(|part| !part.is_empty()
                    && part.bytes().all(|byte| byte.is_ascii_digit())
                    && (part.len() == 1 || !part.starts_with('0'))),
            "invalid version core"
        );
        ensure!(
            !main.contains('-') || identifiers(pre, true),
            "invalid prerelease"
        );
        let core: [String; 3] = core
            .try_into()
            .map_err(|_| anyhow!("invalid version core"))?;
        Ok(Self {
            core,
            pre: if pre.is_empty() {
                vec![]
            } else {
                pre.split('.').map(str::to_owned).collect()
            },
        })
    }
}

fn number_cmp(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.core.iter().zip(&other.core) {
            let order = number_cmp(a, b);
            if order != Ordering::Equal {
                return order;
            }
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        for (a, b) in self.pre.iter().zip(&other.pre) {
            let numeric = |value: &str| value.bytes().all(|byte| byte.is_ascii_digit());
            let order = match (numeric(a), numeric(b)) {
                (true, true) => number_cmp(a, b),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => a.cmp(b),
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        self.pre.len().cmp(&other.pre.len())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The version printed by a Pi executable, if it exposes one.
pub fn parse_installed(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|word| {
        let candidate = word.trim_matches(|c: char| {
            !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        });
        let candidate = candidate.strip_prefix('v').unwrap_or(candidate);
        Version::parse(candidate).ok().map(|_| candidate.to_string())
    })
}

/// Ask the selected Pi for its version. Network and filesystem access stay out.
pub fn installed_version(program: &str) -> Option<String> {
    let mut command = std::process::Command::new(program);
    command.arg("--version");
    let output = crate::process::run(
        command,
        Duration::from_secs(10),
        64 * 1024,
        None,
        None,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_installed(&String::from_utf8_lossy(&output.stdout))
}

pub fn is_newer(installed: &str, latest: &str) -> bool {
    match (Version::parse(installed), Version::parse(latest)) {
        (Ok(installed), Ok(latest)) => latest > installed,
        _ => false,
    }
}

/// One daily attempt is enough; callers persist the timestamp before dispatch.
pub fn check_due(last: Option<i64>, now: i64, enabled: bool) -> bool {
    enabled && last.is_none_or(|last| now.saturating_sub(last) >= CHECK_DAILY_SECONDS)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_secs() as i64)
        .unwrap_or_default()
}

pub fn display_version(version: Option<&str>) -> String {
    version.unwrap_or("desconhecida").to_string()
}

pub fn format_last_check(last: Option<i64>) -> String {
    use chrono::TimeZone;
    match last {
        None => "nunca".to_string(),
        Some(seconds) => chrono::Local
            .timestamp_opt(seconds, 0)
            .single()
            .map(|time| time.format("%d/%m/%Y às %H:%M").to_string())
            .unwrap_or_else(|| "desconhecida".to_string()),
    }
}

/// Content for the notification banner. The Workspace attaches the action
/// because opening the installer needs the workspace entity.
pub struct Available {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub version: String,
}

pub fn available(installed: Option<&str>, latest: &str) -> Option<Available> {
    let installed = installed?;
    if !is_newer(installed, latest) {
        return None;
    }
    Some(Available {
        id: format!("pi-update-{latest}"),
        title: format!("Pi {latest} disponível"),
        detail: format!("Instalado: {installed}. O instalador oficial abre num terminal."),
        version: latest.to_string(),
    })
}

pub fn registry_url() -> String {
    std::env::var("DISH_PI_REGISTRY_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| REGISTRY.to_string())
}

pub fn fetch_latest() -> Result<String> {
    fetch_latest_from(&registry_url())
}

fn fetch_latest_from(input: &str) -> Result<String> {
    let url = Url::parse(input)?;
    validate_url(&url)?;
    let config = ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(15)))
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_resolve(Some(Duration::from_secs(5)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(url.as_str())
        .header("User-Agent", concat!("dish/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/json")
        .call()
        .map_err(|error| anyhow!("registry request failed: {error}"))?;
    ensure!(
        response.status().as_u16() == 200,
        "unexpected registry status"
    );
    let bytes = response
        .body_mut()
        .with_config()
        .limit(RESPONSE_LIMIT)
        .read_to_vec()
        .context("reading registry response")?;
    parse_registry_json(&bytes)
}

fn validate_url(url: &Url) -> Result<()> {
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "invalid registry URL"
    );
    let host = url.host_str().ok_or_else(|| anyhow!("missing registry host"))?;
    let loopback = host
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback());
    match url.scheme() {
        "https" => ensure!(
            host == "registry.npmjs.org" || loopback,
            "unexpected registry host"
        ),
        "http" => ensure!(loopback, "HTTP is only allowed on loopback"),
        _ => bail!("registry requires HTTP or HTTPS"),
    }
    Ok(())
}

#[derive(Deserialize)]
struct RegistryPackage {
    name: String,
    version: String,
}

fn parse_registry_json(bytes: &[u8]) -> Result<String> {
    let package: RegistryPackage = serde_json::from_slice(bytes)?;
    ensure!(package.name == PACKAGE, "unexpected package in registry");
    Version::parse(&package.version)?;
    Ok(package.version)
}

/// Where a Pi update can be installed from. Kept explicit so the settings row
/// can explain why a non-Linux system cannot use the official installer.
pub fn installer_supported() -> bool {
    cfg!(target_os = "linux")
}

/// Best-effort log line, without URLs or credentials.
pub fn log_check_failed(automatic: bool) {
    crate::logging::warn(format_args!(
        "Dish: Pi update check failed ({})",
        if automatic { "automatic" } else { "manual" }
    ));
}

/// Log before handing control to the terminal installer.
pub fn log_install_start(installed: Option<&str>, automatic: bool) {
    crate::logging::warn(format_args!(
        "Dish: opening official Pi installer; installed={} automatic={automatic}",
        display_version(installed)
    ));
}

pub fn log_install_result(before: Option<&str>, after: Option<&str>) {
    crate::logging::warn(format_args!(
        "Dish: Pi version after installer: before={} after={}",
        display_version(before),
        display_version(after)
    ));
}

/// Run Pi's official installer through the existing setup path.
pub fn launch_installer() -> Result<()> {
    crate::installation::launch_official_installer()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn server(status: u16, body: &[u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = body.to_vec();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            let header = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
        });
        format!("http://{address}/latest")
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
            "1.4.0",
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
    fn installed_output_accepts_prefixes_and_ignores_noise() {
        assert_eq!(parse_installed("1.4.0\n").as_deref(), Some("1.4.0"));
        assert_eq!(parse_installed("pi 1.4.0").as_deref(), Some("1.4.0"));
        assert_eq!(parse_installed("Pi v1.4.0-beta.2").as_deref(), Some("1.4.0-beta.2"));
        assert_eq!(parse_installed("not a version"), None);
        assert_eq!(parse_installed("1.4"), None);
    }

    #[test]
    fn newer_versions_offer_an_update_and_equal_ones_do_not() {
        assert!(is_newer("1.4.0", "1.4.1"));
        assert!(is_newer("1.4.0-beta.1", "1.4.0"));
        assert!(!is_newer("1.4.0", "1.4.0"));
        assert!(!is_newer("1.4.1", "1.4.0"));
        assert!(!is_newer("unknown", "1.4.1"));
        let offered = available(Some("1.4.0"), "1.4.1").unwrap();
        assert_eq!(offered.id, "pi-update-1.4.1");
        assert_eq!(offered.title, "Pi 1.4.1 disponível");
        assert!(offered.detail.contains("Instalado: 1.4.0"));
        assert!(available(Some("1.4.0"), "1.4.0").is_none());
        assert!(available(None, "1.4.1").is_none());
    }

    #[test]
    fn cadence_allows_one_attempt_per_day_and_disabled_checks_never_run() {
        let now = 1_700_000_000;
        assert!(check_due(None, now, true));
        assert!(!check_due(Some(now - 86_399), now, true));
        assert!(check_due(Some(now - 86_400), now, true));
        assert!(!check_due(Some(now + 1_000), now, true));
        assert!(!check_due(None, now, false));
        assert!(!check_due(Some(now - 86_400), now, false));
    }

    #[test]
    fn registry_json_requires_the_configured_package() {
        let body = br#"{"name":"@earendil-works/pi-coding-agent","version":"1.4.0"}"#;
        assert_eq!(parse_registry_json(body).unwrap(), "1.4.0");
        let wrong = br#"{"name":"some-other-package","version":"1.4.0"}"#;
        assert!(parse_registry_json(wrong).is_err());
        let invalid = br#"{"name":"@earendil-works/pi-coding-agent","version":"latest"}"#;
        assert!(parse_registry_json(invalid).is_err());
    }

    #[test]
    fn registry_url_only_accepts_https_or_loopback_http() {
        assert!(validate_url(&Url::parse(REGISTRY).unwrap()).is_ok());
        assert!(validate_url(&Url::parse("http://127.0.0.1:9/latest").unwrap()).is_ok());
        assert!(validate_url(&Url::parse("http://example.com/latest").unwrap()).is_err());
        assert!(validate_url(&Url::parse("https://example.com/latest").unwrap()).is_err());
        assert!(validate_url(&Url::parse("ftp://registry.npmjs.org/latest").unwrap()).is_err());
    }

    #[test]
    fn local_registry_response_is_parsed_and_errors_fail_closed() {
        let url = server(
            200,
            br#"{"name":"@earendil-works/pi-coding-agent","version":"1.4.1"}"#,
        );
        assert_eq!(fetch_latest_from(&url).unwrap(), "1.4.1");

        let url = server(404, b"not found");
        assert!(fetch_latest_from(&url).is_err());

        let url = server(
            200,
            br#"{"name":"wrong-package","version":"1.4.1"}"#,
        );
        assert!(fetch_latest_from(&url).is_err());
    }

    #[test]
    fn production_paths_are_exposed_for_the_ui() {
        assert_eq!(display_version(None), "desconhecida");
        assert_eq!(display_version(Some("1.4.0")), "1.4.0");
        assert_eq!(format_last_check(None), "nunca");
        assert!(!format_last_check(Some(0)).is_empty());
        assert_eq!(PACKAGE, "@earendil-works/pi-coding-agent");
    }
}
