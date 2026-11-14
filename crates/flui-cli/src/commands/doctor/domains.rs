//! `flui doctor --domains`: whether each associated domain the application
//! declares (ADR-0307 §4) serves the file that lets its `https` links open
//! the application.
//!
//! The files live with the domain's owner, so the CLI never writes them;
//! the check fetches them, which is why it runs only when asked. An Apple
//! platform reads `https://<domain>/.well-known/apple-app-site-association`
//! for the domains of the application's Apple targets, Android reads
//! `https://<domain>/.well-known/assetlinks.json` for those of its Android
//! targets. Neither follows a redirect, so neither does the check. The fetch
//! is `curl`, which the platforms the CLI runs on carry; the reading of an
//! answer is apart from it, so the tests need no network.
//!
//! Check ids, one per domain and file, the file's address in the event's
//! `url`: `project.apple-app-site-association` and `project.assetlinks`;
//! `project.domains` when the domains cannot be read.

use std::io::Read as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::{Check, Fix, Status};
use crate::config::FluiConfig;
use crate::declarations::Declarations;
use crate::declarations::verify::ManifestKind;

const SECTION: &str = "project";

/// The largest file read: Apple's limit for `apple-app-site-association`,
/// and more than an `assetlinks.json` of a few hundred statements needs.
const MAX_BODY: u64 = 128 * 1024;

/// How many of the names a file gives a failing check lists.
const LISTED: usize = 5;

/// The budget of one fetch, connection included.
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// The relation an `assetlinks.json` statement grants for verified links.
const HANDLE_ALL_URLS: &str = "delegate_permission/common.handle_all_urls";

/// The note on a passing `assetlinks.json`.
const FINGERPRINTS_NOT_COMPARED: &str = "the fingerprints are not compared with the certificate \
                                         the release is signed with";

/// The note on a passing `apple-app-site-association`.
const TEAM_NOT_COMPARED: &str = "the team identifier is not compared with the one the release is \
                                 signed by";

/// An HTTP answer, as the check reads it.
#[derive(Debug)]
pub(super) struct Answer {
    /// The status code.
    pub(super) status: u16,
    /// The `Content-Type`, empty when the server sent none.
    pub(super) content_type: String,
    /// At most [`MAX_BODY`] bytes of the body.
    pub(super) body: Vec<u8>,
}

/// Fetches a URL without following a redirect, or says why it could not.
pub(super) type Fetch<'a> = &'a dyn Fn(&str) -> Result<Answer, String>;

/// The checks of each associated domain the project at `dir` declares,
/// fetched with `fetch`.
pub(super) fn domain_checks(dir: &Path, fetch: Fetch<'_>) -> Vec<Check> {
    const ID: &str = "project.domains";
    const TITLE: &str = "Associated domains";
    let failing = |detail: String| {
        vec![Check::failing(
            ID,
            SECTION,
            TITLE,
            Status::Error,
            detail,
            None,
        )]
    };
    let config = match FluiConfig::load_from(&dir.join("flui.toml")) {
        Ok(config) => config,
        Err(error) => return failing(error.to_string()),
    };
    let domains = |kind: ManifestKind| {
        Declarations::collect(dir, kind.targets()).map(|declarations| {
            declarations
                .associated_domains()
                .map(|(domain, _)| domain.to_string())
                .collect::<Vec<String>>()
        })
    };
    let (apple, android) = match (
        domains(ManifestKind::AppleInfo),
        domains(ManifestKind::Android),
    ) {
        (Ok(apple), Ok(android)) => (apple, android),
        (Err(error), _) | (_, Err(error)) => return failing(error.to_string()),
    };
    if apple.is_empty() && android.is_empty() {
        return vec![Check::ok(
            ID,
            SECTION,
            TITLE,
            "the application declares no associated domain".to_owned(),
        )];
    }
    let bundle = config.app.id.apple_bundle_id();
    let package = config.app.id.android_package();
    let apple_checks = apple.iter().map(|domain| {
        let url = format!("https://{domain}/.well-known/apple-app-site-association");
        let verdict = fetch(&url).and_then(|answer| apple_association(&answer, &bundle));
        domain_check(
            "project.apple-app-site-association",
            "Apple app site association",
            url,
            verdict,
            TEAM_NOT_COMPARED,
        )
    });
    let android_checks = android.iter().map(|domain| {
        let url = format!("https://{domain}/.well-known/assetlinks.json");
        let verdict = fetch(&url).and_then(|answer| asset_links(&answer, package));
        domain_check(
            "project.assetlinks",
            "Android asset links",
            url,
            verdict,
            FINGERPRINTS_NOT_COMPARED,
        )
    });
    apple_checks.chain(android_checks).collect()
}

/// One domain's check: `verdict` is what passing found, or what failed.
fn domain_check(
    id: &'static str,
    title: &'static str,
    url: String,
    verdict: Result<String, String>,
    passing_note: &'static str,
) -> Check {
    match verdict {
        Ok(detail) => Check::ok(id, SECTION, title, detail)
            .with_note(passing_note)
            .with_url(url),
        Err(detail) => Check::failing(
            id,
            SECTION,
            title,
            Status::Error,
            detail,
            Some(Fix::manual(format!(
                "serve the file at {url} with status 200, without a redirect"
            ))),
        )
        .with_url(url),
    }
}

/// The body of `answer` when it is a file served as the platforms take it:
/// status 200, not a redirect, and of type `application/json` when
/// `json_type` asks for it.
fn served(answer: &Answer, json_type: bool) -> Result<&[u8], String> {
    match answer.status {
        200 => {}
        300..=399 => {
            return Err(format!(
                "answers with a redirect (HTTP {}), which the platform does not follow",
                answer.status
            ));
        }
        status => return Err(format!("answers HTTP {status}")),
    }
    let media_type = answer
        .content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    if json_type && !media_type.eq_ignore_ascii_case("application/json") {
        return Err(format!(
            "is served as `{}`, not `application/json`",
            if media_type.is_empty() {
                "no type"
            } else {
                media_type
            }
        ));
    }
    Ok(&answer.body)
}

/// What an `apple-app-site-association` grants `bundle`: an `applinks`
/// entry naming `<team>.<bundle>` with the components or paths it opens.
fn apple_association(answer: &Answer, bundle: &str) -> Result<String, String> {
    let body = served(answer, false)?;
    let file: serde_json::Value =
        serde_json::from_slice(body).map_err(|error| format!("is not JSON: {error}"))?;
    let details = file
        .get("applinks")
        .and_then(|applinks| applinks.get("details"))
        .and_then(serde_json::Value::as_array)
        .ok_or("has no `applinks.details` array")?;
    let mut named = Vec::new();
    for detail in details {
        // `appIDs`, or the single `appID` of the format before iOS 13.
        let ids: Vec<&str> = match detail.get("appIDs") {
            Some(ids) => strings(Some(ids)),
            None => detail
                .get("appID")
                .and_then(serde_json::Value::as_str)
                .into_iter()
                .collect(),
        };
        let Some(id) = ids.iter().copied().find(|id| names_bundle(id, bundle)) else {
            named.extend(ids);
            continue;
        };
        let opens = |key: &str| {
            detail
                .get(key)
                .and_then(serde_json::Value::as_array)
                .is_some_and(|entries| !entries.is_empty())
        };
        if !opens("components") && !opens("paths") {
            return Err(format!(
                "names {id} with no `components` or `paths`, so no link opens the application"
            ));
        }
        return Ok(format!("opens the application's links for {id}"));
    }
    Err(if named.is_empty() {
        format!("names no application; it needs `<team>.{bundle}` in an `appIDs` entry")
    } else {
        format!(
            "does not name `<team>.{bundle}`; it names {}",
            listed(&named)
        )
    })
}

/// Whether the application identifier `id` is `bundle` under a team
/// identifier: ten ASCII capitals and digits, a dot, the bundle identifier.
fn names_bundle(id: &str, bundle: &str) -> bool {
    id.split_once('.').is_some_and(|(team, rest)| {
        team.len() == 10
            && team
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            && rest == bundle
    })
}

/// What an `assetlinks.json` grants `package`: a statement letting the
/// Android application `package` handle all URLs, with the fingerprints of
/// the certificates it is signed with.
fn asset_links(answer: &Answer, package: &str) -> Result<String, String> {
    let body = served(answer, true)?;
    let file: serde_json::Value =
        serde_json::from_slice(body).map_err(|error| format!("is not JSON: {error}"))?;
    let statements = file.as_array().ok_or("is not a JSON array of statements")?;
    let mut packages = Vec::new();
    for statement in statements {
        let grants = statement
            .get("relation")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|relations| {
                relations
                    .iter()
                    .any(|relation| relation.as_str() == Some(HANDLE_ALL_URLS))
            });
        let Some(target) = statement.get("target") else {
            continue;
        };
        if !grants
            || target.get("namespace").and_then(serde_json::Value::as_str) != Some("android_app")
        {
            continue;
        }
        let Some(name) = target
            .get("package_name")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        if name != package {
            packages.push(name);
            continue;
        }
        let fingerprints = strings(target.get("sha256_cert_fingerprints"));
        if fingerprints.is_empty() {
            return Err(format!(
                "grants {package} with no `sha256_cert_fingerprints`, so Android verifies no \
                 build of it"
            ));
        }
        if let Some(malformed) = fingerprints
            .iter()
            .find(|fingerprint| !is_sha256_fingerprint(fingerprint))
        {
            return Err(format!(
                "grants {package} a fingerprint `{}` that is not 32 colon-separated hex \
                 bytes",
                malformed.escape_debug()
            ));
        }
        return Ok(format!(
            "lets {package} handle its links, signed by {} certificate(s)",
            fingerprints.len()
        ));
    }
    Err(if packages.is_empty() {
        format!(
            "has no `{HANDLE_ALL_URLS}` statement for an Android application; it needs one for {package}"
        )
    } else {
        format!(
            "does not grant {package} `{HANDLE_ALL_URLS}`; it grants {}",
            listed(&packages)
        )
    })
}

/// At most the first [`LISTED`] of `names` a server sent, each escaped so a
/// control character reaches the terminal as text.
fn listed(names: &[&str]) -> String {
    let mut shown: Vec<String> = names
        .iter()
        .take(LISTED)
        .map(|name| name.escape_debug().to_string())
        .collect();
    if names.len() > LISTED {
        shown.push(format!("and {} more", names.len() - LISTED));
    }
    shown.join(", ")
}

/// The strings of the JSON array `value`, none when it is not one.
fn strings(value: Option<&serde_json::Value>) -> Vec<&str> {
    value
        .and_then(serde_json::Value::as_array)
        .map_or_default(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect()
        })
}

/// Whether `fingerprint` is a SHA-256 certificate fingerprint as
/// `keytool` prints it: 32 hex bytes joined by colons.
fn is_sha256_fingerprint(fingerprint: &str) -> bool {
    let bytes: Vec<&str> = fingerprint.split(':').collect();
    bytes.len() == 32
        && bytes
            .iter()
            .all(|byte| byte.len() == 2 && byte.bytes().all(|digit| digit.is_ascii_hexdigit()))
}

/// Fetch `url` with `curl`, refusing anything but `https`, following no
/// redirect and reading at most [`MAX_BODY`] bytes.
///
/// # Errors
///
/// Why no answer came: `curl` missing, the connection, the deadline, or a
/// body over the bound.
pub(super) fn curl(url: &str) -> Result<Answer, String> {
    let file = tempfile::NamedTempFile::new()
        .map_err(|error| format!("cannot create a file for the answer: {error}"))?;
    let mut command = Command::new("curl");
    command
        .args(["--silent", "--show-error", "--proto", "=https"])
        .args(["--max-time", &FETCH_TIMEOUT.as_secs().to_string()])
        .args(["--max-filesize", &MAX_BODY.to_string()])
        .arg("--output")
        .arg(file.path())
        .args(["--write-out", "%{http_code}\\n%{content_type}"])
        .arg(url);
    let output = crate::proc::output_with_timeout(&mut command, FETCH_TIMEOUT + FETCH_TIMEOUT)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => "cannot be fetched: `curl` is not on PATH".to_owned(),
            _ => format!("cannot be fetched: {error}"),
        })?;
    // curl's exit code for a body over `--max-filesize`.
    const FILE_TOO_LARGE: i32 = 63;
    match output.status.code() {
        Some(0) => {}
        Some(FILE_TOO_LARGE) => return Err(format!("is larger than {} KiB", MAX_BODY / 1024)),
        _ => {
            return Err(format!(
                "cannot be fetched: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    let written = String::from_utf8_lossy(&output.stdout);
    let mut lines = written.lines();
    let status = lines
        .next()
        .and_then(|code| code.trim().parse::<u16>().ok())
        .ok_or_else(|| format!("curl reported no status: {written}"))?;
    let content_type = lines.next().unwrap_or_default().trim().to_owned();
    let mut body = Vec::new();
    file.as_file()
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(|error| format!("cannot read the answer: {error}"))?;
    if u64::try_from(body.len()).unwrap_or(u64::MAX) > MAX_BODY {
        return Err(format!("is larger than {} KiB", MAX_BODY / 1024));
    }
    Ok(Answer {
        status,
        content_type,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::{Answer, apple_association, asset_links};

    fn answer(status: u16, content_type: &str, body: &str) -> Answer {
        Answer {
            status,
            content_type: content_type.to_owned(),
            body: body.as_bytes().to_vec(),
        }
    }

    const FINGERPRINT: &str = "14:6D:E9:83:C5:73:06:50:D8:EE:B9:95:2F:34:FC:64:16:A0:83:42:E6:1D:BE:A8:8A:04:96:B2:3F:CF:44:E5";

    /// An `apple-app-site-association` opens the application's links when
    /// an `applinks` entry names its bundle under a team with what it opens;
    /// a redirect, another application, a malformed team, an entry opening
    /// nothing and a file that is not JSON each fail with the reason.
    #[test]
    fn an_apple_association_names_the_application_with_what_it_opens() {
        let bundle = "com.example.notes";
        let cases: [(&str, Answer, Result<&str, &str>); 8] = [
            (
                "components",
                answer(
                    200,
                    "application/json",
                    r#"{"applinks":{"details":[{"appIDs":["ABCDE12345.com.example.notes"],"components":[{"/":"/note/*"}]}]}}"#,
                ),
                Ok("opens the application's links for ABCDE12345.com.example.notes"),
            ),
            (
                "legacy appID and paths, no content type",
                answer(
                    200,
                    "",
                    r#"{"applinks":{"apps":[],"details":[{"appID":"ABCDE12345.com.example.notes","paths":["/note/*"]}]}}"#,
                ),
                Ok("opens the application's links for ABCDE12345.com.example.notes"),
            ),
            (
                "redirect",
                answer(301, "text/html", ""),
                Err("answers with a redirect (HTTP 301), which the platform does not follow"),
            ),
            (
                "missing",
                answer(404, "text/html", "<html/>"),
                Err("answers HTTP 404"),
            ),
            (
                "another application",
                answer(
                    200,
                    "application/json",
                    r#"{"applinks":{"details":[{"appIDs":["ABCDE12345.com.example.other"],"components":[{"/":"*"}]}]}}"#,
                ),
                Err(
                    "does not name `<team>.com.example.notes`; it names ABCDE12345.com.example.other",
                ),
            ),
            (
                "no team",
                answer(
                    200,
                    "application/json",
                    r#"{"applinks":{"details":[{"appIDs":["com.example.notes"],"components":[{"/":"*"}]}]}}"#,
                ),
                Err("does not name `<team>.com.example.notes`; it names com.example.notes"),
            ),
            (
                "opens nothing",
                answer(
                    200,
                    "application/json",
                    r#"{"applinks":{"details":[{"appIDs":["ABCDE12345.com.example.notes"],"components":[]}]}}"#,
                ),
                Err(
                    "names ABCDE12345.com.example.notes with no `components` or `paths`, so no link opens the application",
                ),
            ),
            (
                "no applinks",
                answer(200, "application/json", r#"{"webcredentials":{"apps":[]}}"#),
                Err("has no `applinks.details` array"),
            ),
        ];
        let mut failures = Vec::new();
        for (name, answer, expected) in cases {
            let verdict = apple_association(&answer, bundle);
            if verdict.as_deref() != expected.map_err(str::to_owned).as_deref() {
                failures.push(format!("{name}: {verdict:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        assert!(
            apple_association(&answer(200, "", "<html>"), bundle)
                .is_err_and(|reason| reason.starts_with("is not JSON")),
        );
        // What the server names is listed escaped, the first few of it.
        let many = r#"{"applinks":{"details":[{"appIDs":["A\u001b[2J","B","C","D","E","F","G"],"components":[{"/":"*"}]}]}}"#;
        assert_eq!(
            apple_association(&answer(200, "", many), bundle),
            Err(
                "does not name `<team>.com.example.notes`; it names A\\u{1b}[2J, B, C, D, E, and 2 \
                 more"
                    .to_owned()
            ),
        );
    }

    /// An `assetlinks.json` lets the application handle its links when a
    /// `handle_all_urls` statement names its package with well-formed
    /// fingerprints, served as `application/json`; another package, another
    /// relation, no or a malformed fingerprint and another type each fail.
    #[test]
    fn asset_links_grant_the_package_with_its_fingerprints() {
        let package = "com.example.notes";
        let statement = |relation: &str, name: &str, fingerprints: &str| {
            format!(
                r#"[{{"relation":["{relation}"],"target":{{"namespace":"android_app","package_name":"{name}","sha256_cert_fingerprints":[{fingerprints}]}}}}]"#
            )
        };
        let all = "delegate_permission/common.handle_all_urls";
        let good = statement(all, package, &format!("\"{FINGERPRINT}\""));
        let cases: [(&str, Answer, Result<&str, &str>); 7] = [
            (
                "granted",
                answer(200, "application/json; charset=utf-8", &good),
                Ok("lets com.example.notes handle its links, signed by 1 certificate(s)"),
            ),
            (
                "another type",
                answer(200, "text/plain", &good),
                Err("is served as `text/plain`, not `application/json`"),
            ),
            (
                "redirect",
                answer(302, "", ""),
                Err("answers with a redirect (HTTP 302), which the platform does not follow"),
            ),
            (
                "another package",
                answer(
                    200,
                    "application/json",
                    &statement(all, "com.example.other", &format!("\"{FINGERPRINT}\"")),
                ),
                Err(
                    "does not grant com.example.notes `delegate_permission/common.handle_all_urls`; it grants com.example.other",
                ),
            ),
            (
                "another relation",
                answer(
                    200,
                    "application/json",
                    &statement(
                        "delegate_permission/common.get_login_creds",
                        package,
                        &format!("\"{FINGERPRINT}\""),
                    ),
                ),
                Err(
                    "has no `delegate_permission/common.handle_all_urls` statement for an Android application; it needs one for com.example.notes",
                ),
            ),
            (
                "no fingerprint",
                answer(200, "application/json", &statement(all, package, "")),
                Err(
                    "grants com.example.notes with no `sha256_cert_fingerprints`, so Android verifies no build of it",
                ),
            ),
            (
                "malformed fingerprint",
                answer(
                    200,
                    "application/json",
                    &statement(all, package, "\"14:6D\""),
                ),
                Err(
                    "grants com.example.notes a fingerprint `14:6D` that is not 32 colon-separated hex bytes",
                ),
            ),
        ];
        let mut failures = Vec::new();
        for (name, answer, expected) in cases {
            let verdict = asset_links(&answer, package);
            if verdict.as_deref() != expected.map_err(str::to_owned).as_deref() {
                failures.push(format!("{name}: {verdict:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
