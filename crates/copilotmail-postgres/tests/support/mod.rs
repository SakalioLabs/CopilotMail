//! Safety boundary for destructive/constraint-testing integration fixtures.

use url::{Host, Url};

pub fn allowed_database(value: &str, opt_in: bool, ci: bool) -> bool {
    if !opt_in {
        return false;
    }
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if !matches!(url.scheme(), "postgres" | "postgresql")
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host().is_none()
    {
        return false;
    }
    let name = url.path().strip_prefix('/').unwrap_or_default();
    let explicit_test_name = name
        .strip_prefix("copilotmail_test_")
        .is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        });
    let loopback = match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        // postgres is a non-special URL scheme: the URL parser can retain an
        // IPv4 literal as Domain rather than normalizing it to Host::Ipv4.
        Some(Host::Domain(name)) => {
            name == "localhost"
                || name
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        }
        _ => false,
    };
    explicit_test_name || (ci && loopback && name == "copilotmail")
}

pub fn database_url() -> String {
    let value = std::env::var("COPILOTMAIL_DATABASE_URL")
        .expect("an isolated test database URL is required");
    let opt_in = std::env::var("COPILOTMAIL_ALLOW_ISOLATED_DB_TESTS").is_ok_and(|v| v == "1");
    let ci = std::env::var("CI").is_ok_and(|v| v == "true" || v == "1");
    assert!(
        allowed_database(&value, opt_in, ci),
        "refusing database tests: explicit opt-in and an isolated test database are required"
    );
    value
}

#[test]
fn database_guard_rejects_implicit_or_redirectable_production_targets() {
    for (url, opt_in, ci) in [
        ("postgres://user:secret@127.0.0.1/copilotmail", false, true),
        ("postgres://user:secret@127.0.0.1/copilotmail", true, false),
        ("postgres://user:secret@production/copilotmail", true, true),
        (
            "postgres://user:secret@127.0.0.1/copilotmail_test_one?dbname=production",
            true,
            false,
        ),
        (
            "postgres://user:secret@127.0.0.1/copilotmail_test_one?host=production",
            true,
            true,
        ),
        (
            "postgres://user:secret@127.0.0.1/copilotmail_test_one#fragment",
            true,
            false,
        ),
        (
            "postgres://user:secret@127.0.0.1/copilotmail_test_",
            true,
            false,
        ),
        (
            "postgres://user:secret@127.0.0.1/copilotmail_test_%2fproduction",
            true,
            false,
        ),
        ("http://127.0.0.1/copilotmail_test_one", true, false),
    ] {
        assert!(!allowed_database(url, opt_in, ci));
    }
    assert!(allowed_database(
        "postgres://user:secret@127.0.0.1/copilotmail",
        true,
        true
    ));
    assert!(allowed_database(
        "postgresql://user:secret@[::1]/copilotmail",
        true,
        true
    ));
    assert!(allowed_database(
        "postgres://user:secret@isolated-db/copilotmail_test_one",
        true,
        false
    ));
}
