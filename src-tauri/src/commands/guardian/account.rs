use super::license::cache_license;
use super::types::{Account, CheckoutSession, LicenseStatus};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Checkout,
    RestoreStart,
    RestoreVerify,
    Account,
    Cancel,
}

const UNREACHABLE: &str = "Couldn't reach Pawtrol. Check your connection and try again.";
const TIMED_OUT: &str = "Pawtrol took too long to answer. Try again.";
const SERVER_DOWN: &str = "Pawtrol's server had a hiccup. Try again in a minute.";
const BAD_REPLY: &str = "Got an unexpected reply from Pawtrol. Try again.";
const GENERIC: &str = "Something went wrong. Try again.";
const BAD_EMAIL: &str = "Enter a valid email address.";
const BAD_CODE: &str = "That code didn't work. Check it and try again.";
const EXPIRED_CODE: &str = "That code expired. Send a new one.";
const CODE_FORMAT: &str = "Enter the 6-digit code from your email.";

fn normalize_email(email: &str) -> Result<String, String> {
    let email = email.trim().to_lowercase();
    let valid = match email.split_once('@') {
        Some((user, domain)) => {
            !user.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !email.chars().any(char::is_whitespace)
        }
        None => false,
    };
    if valid {
        Ok(email)
    } else {
        Err(BAD_EMAIL.into())
    }
}

fn normalize_code(code: &str) -> Result<String, String> {
    let digits: String = code.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    if digits.len() == 6 && digits.chars().all(|c| c.is_ascii_digit()) {
        Ok(digits)
    } else {
        Err(CODE_FORMAT.into())
    }
}

fn worker_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_lowercase))
        .unwrap_or_default()
}

fn friendly_error(call: Call, status: u16, body: &str) -> String {
    let msg = worker_message(body);
    let text = match (call, status) {
        (Call::RestoreVerify, _) if msg.contains("expired") => EXPIRED_CODE,
        (Call::RestoreVerify, 429) => "Too many attempts. Send a new code and try again.",
        (Call::RestoreStart, 429) => "Too many codes sent. Try again in an hour.",
        (_, 429) => "Too many tries. Wait a minute and try again.",
        (Call::RestoreVerify, 400 | 401 | 403 | 404) => BAD_CODE,
        (Call::RestoreVerify, 410) => EXPIRED_CODE,
        (Call::Checkout | Call::RestoreStart, 400 | 422) => BAD_EMAIL,
        (Call::Checkout, 409) => "Pawtrol is already active on this Mac.",
        (Call::Cancel, 404) => "No subscription found for this Mac.",
        (Call::Cancel, 409) => "This subscription is already cancelled.",
        (_, s) if s >= 500 => SERVER_DOWN,
        _ => GENERIC,
    };
    text.to_string()
}

fn transport_error(e: reqwest::Error) -> String {
    if e.is_timeout() {
        TIMED_OUT.into()
    } else {
        UNREACHABLE.into()
    }
}

/// Sends a request and returns the raw status and body, with transport failures already made friendly.
async fn send(req: reqwest::RequestBuilder) -> Result<(u16, String), String> {
    let resp = req.timeout(TIMEOUT).send().await.map_err(transport_error)?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(transport_error)?;
    Ok((status, body))
}

fn parse<T: DeserializeOwned>(call: Call, status: u16, body: &str) -> Result<T, String> {
    if !(200..300).contains(&status) {
        return Err(friendly_error(call, status, body));
    }
    serde_json::from_str(body).map_err(|_| BAD_REPLY.to_string())
}

pub(crate) async fn checkout_create_with(
    client: &reqwest::Client,
    base_url: &str,
    device_id: &str,
    email: &str,
) -> Result<CheckoutSession, String> {
    let email = normalize_email(email)?;
    let (status, body) = send(
        client
            .post(format!("{}/checkout/create", base_url))
            .json(&json!({ "device_id": device_id, "email": email })),
    )
    .await?;
    let session: CheckoutSession = parse(Call::Checkout, status, &body)?;
    // Only ever hand a hosted https checkout page to the system opener. The one exception is a
    // debug build talking to a local worker in mock mode, whose fake checkout lives on loopback.
    if !session.short_url.starts_with("https://") {
        if is_local_dev(base_url) && is_loopback_http(&session.short_url) {
            return Ok(session);
        }
        return Err(BAD_REPLY.into());
    }
    Ok(session)
}

fn is_loopback_http(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")
}

pub(crate) fn is_local_dev(base_url: &str) -> bool {
    cfg!(debug_assertions) && is_loopback_http(&format!("{}/", base_url.trim_end_matches('/')))
}

pub(crate) async fn restore_start_with(
    client: &reqwest::Client,
    base_url: &str,
    email: &str,
) -> Result<(), String> {
    let email = normalize_email(email)?;
    let (status, body) = send(
        client
            .post(format!("{}/restore/start", base_url))
            .json(&json!({ "email": email })),
    )
    .await?;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(friendly_error(Call::RestoreStart, status, &body))
    }
}

pub(crate) async fn restore_verify_with(
    client: &reqwest::Client,
    base_url: &str,
    device_id: &str,
    email: &str,
    code: &str,
    data_dir: &Path,
) -> Result<LicenseStatus, String> {
    let email = normalize_email(email)?;
    let code = normalize_code(code)?;
    let (status, body) = send(
        client
            .post(format!("{}/restore/verify", base_url))
            .json(&json!({ "email": email, "code": code, "device_id": device_id })),
    )
    .await?;
    let license: LicenseStatus = parse(Call::RestoreVerify, status, &body)?;
    // An inactive reply means "no subscription for that email"; it says nothing about this Mac's own license.
    if license.active {
        cache_license(data_dir, &license);
    }
    Ok(license)
}

pub(crate) async fn account_with(
    client: &reqwest::Client,
    base_url: &str,
    device_id: &str,
) -> Result<Option<Account>, String> {
    let (status, body) = send(
        client
            .get(format!("{}/account", base_url))
            .query(&[("device_id", device_id)]),
    )
    .await?;
    if status == 404 {
        return Ok(None);
    }
    parse(Call::Account, status, &body).map(Some)
}

pub(crate) async fn cancel_subscription_with(
    client: &reqwest::Client,
    base_url: &str,
    device_id: &str,
) -> Result<Account, String> {
    let (status, body) = send(
        client
            .post(format!("{}/subscription/cancel", base_url))
            .json(&json!({ "device_id": device_id })),
    )
    .await?;
    parse(Call::Cancel, status, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::guardian::license::cached_license_active_in;
    use crate::commands::guardian::test_support::{
        block_on, client, dead_url, serve_once, CapturedRequest, TestDir,
    };

    fn body_json(req: &CapturedRequest) -> serde_json::Value {
        serde_json::from_str(&req.body).unwrap()
    }

    const ACCOUNT: &str = r#"{"email":"j***@gmail.com","status":"active","current_end":1932854400,"cancel_at_period_end":false,"devices_count":2}"#;

    #[test]
    fn checkout_posts_device_and_normalized_email() {
        let (base, server) = serve_once(
            200,
            r#"{"short_url":"https://rzp.io/i/abc123","subscription_id":"sub_1"}"#,
        );
        let session =
            block_on(checkout_create_with(&client(), &base, "dev-1", "  Me@Example.COM ")).unwrap();
        let req = server.join().unwrap();

        assert_eq!(session.short_url, "https://rzp.io/i/abc123");
        assert_eq!(req.request_line, "POST /checkout/create HTTP/1.1");
        assert!(req.header("content-type").unwrap().contains("application/json"));
        assert_eq!(
            body_json(&req),
            json!({ "device_id": "dev-1", "email": "me@example.com" })
        );
    }

    #[test]
    fn checkout_rejects_invalid_email_without_a_request() {
        for email in ["", "me", "me@", "@x.com", "me@x", "me@@x.com", "m e@x.com", "me@x."] {
            let err = block_on(checkout_create_with(&client(), &dead_url(), "d", email)).unwrap_err();
            assert_eq!(err, BAD_EMAIL, "{email}");
        }
    }

    #[test]
    fn checkout_refuses_non_https_urls() {
        let (base, server) = serve_once(200, r#"{"short_url":"file:///etc/passwd"}"#);
        let err = block_on(checkout_create_with(&client(), &base, "d", "a@b.co")).unwrap_err();
        server.join().unwrap();
        assert_eq!(err, BAD_REPLY);
    }

    #[test]
    fn checkout_maps_http_errors() {
        let cases = [
            (400, r#"{"error":"Invalid email"}"#, BAD_EMAIL),
            (429, r#"{"error":"slow down"}"#, "Too many tries. Wait a minute and try again."),
            (502, r#"{"error":"Razorpay error"}"#, SERVER_DOWN),
            (418, "teapot", GENERIC),
        ];
        for (status, body, want) in cases {
            let (base, server) = serve_once(status, body);
            let err = block_on(checkout_create_with(&client(), &base, "d", "a@b.co")).unwrap_err();
            server.join().unwrap();
            assert_eq!(err, want, "{status}");
        }
    }

    #[test]
    fn unreachable_worker_is_a_connection_message() {
        let err = block_on(checkout_create_with(&client(), &dead_url(), "d", "a@b.co")).unwrap_err();
        assert_eq!(err, UNREACHABLE);
        let err = block_on(account_with(&client(), &dead_url(), "d")).unwrap_err();
        assert_eq!(err, UNREACHABLE);
    }

    #[test]
    fn malformed_success_body_is_an_unexpected_reply() {
        let (base, server) = serve_once(200, "<html>");
        let err = block_on(checkout_create_with(&client(), &base, "d", "a@b.co")).unwrap_err();
        server.join().unwrap();
        assert_eq!(err, BAD_REPLY);
    }

    #[test]
    fn restore_start_posts_email_only() {
        let (base, server) = serve_once(200, "{}");
        block_on(restore_start_with(&client(), &base, " A@B.co ")).unwrap();
        let req = server.join().unwrap();
        assert_eq!(req.request_line, "POST /restore/start HTTP/1.1");
        assert_eq!(body_json(&req), json!({ "email": "a@b.co" }));
    }

    #[test]
    fn restore_start_rate_limit_is_friendly() {
        let (base, server) = serve_once(429, r#"{"error":"Rate limited"}"#);
        let err = block_on(restore_start_with(&client(), &base, "a@b.co")).unwrap_err();
        server.join().unwrap();
        assert_eq!(err, "Too many codes sent. Try again in an hour.");
    }

    #[test]
    fn restore_verify_sends_code_and_device_and_caches_license() {
        let dir = TestDir::new("restore-ok");
        assert!(!cached_license_active_in(dir.path(), 0));
        let (base, server) = serve_once(200, r#"{"active":true,"expires":1932854400}"#);
        let status = block_on(restore_verify_with(
            &client(),
            &base,
            "dev-7",
            "a@b.co",
            " 123 456 ",
            dir.path(),
        ))
        .unwrap();
        let req = server.join().unwrap();

        assert!(status.active);
        assert_eq!(status.expires, Some(1_932_854_400));
        assert_eq!(req.request_line, "POST /restore/verify HTTP/1.1");
        assert_eq!(
            body_json(&req),
            json!({ "email": "a@b.co", "code": "123456", "device_id": "dev-7" })
        );
        assert!(cached_license_active_in(dir.path(), 1_900_000_000));
    }

    #[test]
    fn restore_verify_inactive_reply_keeps_existing_cache() {
        let dir = TestDir::new("restore-inactive");
        cache_license(
            dir.path(),
            &LicenseStatus {
                active: true,
                expires: Some(u64::MAX),
            },
        );
        let (base, server) = serve_once(200, r#"{"active":false,"expires":null}"#);
        let status =
            block_on(restore_verify_with(&client(), &base, "d", "a@b.co", "123456", dir.path()))
                .unwrap();
        server.join().unwrap();
        assert!(!status.active);
        assert!(cached_license_active_in(dir.path(), 0));
    }

    #[test]
    fn restore_verify_maps_code_errors_and_leaves_cache_alone() {
        let cases = [
            (400, r#"{"error":"Invalid code"}"#, BAD_CODE),
            (401, r#"{"error":"Invalid code"}"#, BAD_CODE),
            (400, r#"{"error":"Code expired"}"#, EXPIRED_CODE),
            (410, "{}", EXPIRED_CODE),
            (429, r#"{"error":"Too many attempts"}"#, "Too many attempts. Send a new code and try again."),
            (500, "{}", SERVER_DOWN),
        ];
        for (status, body, want) in cases {
            let dir = TestDir::new("restore-err");
            let (base, server) = serve_once(status, body);
            let err =
                block_on(restore_verify_with(&client(), &base, "d", "a@b.co", "123456", dir.path()))
                    .unwrap_err();
            server.join().unwrap();
            assert_eq!(err, want, "{status} {body}");
            assert!(!cached_license_active_in(dir.path(), 0));
        }
    }

    #[test]
    fn restore_verify_rejects_malformed_codes_locally() {
        let dir = TestDir::new("restore-format");
        for code in ["", "12345", "1234567", "12a456"] {
            let err =
                block_on(restore_verify_with(&client(), &dead_url(), "d", "a@b.co", code, dir.path()))
                    .unwrap_err();
            assert_eq!(err, CODE_FORMAT, "{code}");
        }
    }

    #[test]
    fn account_gets_by_device_and_parses() {
        let (base, server) = serve_once(200, ACCOUNT);
        let account = block_on(account_with(&client(), &base, "dev 1&x")).unwrap().unwrap();
        let req = server.join().unwrap();
        assert_eq!(req.request_line, "GET /account?device_id=dev+1%26x HTTP/1.1");
        assert_eq!(
            account,
            Account {
                email: "j***@gmail.com".into(),
                status: "active".into(),
                current_end: Some(1_932_854_400),
                cancel_at_period_end: false,
                devices_count: 2,
            }
        );
    }

    #[test]
    fn account_404_is_none_and_errors_are_friendly() {
        let (base, server) = serve_once(404, r#"{"error":"Not found"}"#);
        assert_eq!(block_on(account_with(&client(), &base, "d")).unwrap(), None);
        server.join().unwrap();

        let (base, server) = serve_once(503, "{}");
        assert_eq!(block_on(account_with(&client(), &base, "d")).unwrap_err(), SERVER_DOWN);
        server.join().unwrap();
    }

    #[test]
    fn account_tolerates_missing_optional_fields() {
        let (base, server) = serve_once(200, r#"{"email":"a***@b.co","status":"created"}"#);
        let account = block_on(account_with(&client(), &base, "d")).unwrap().unwrap();
        server.join().unwrap();
        assert_eq!(account.current_end, None);
        assert!(!account.cancel_at_period_end);
        assert_eq!(account.devices_count, 0);
    }

    #[test]
    fn cancel_posts_device_and_returns_account() {
        let body = ACCOUNT.replace(r#""cancel_at_period_end":false"#, r#""cancel_at_period_end":true"#);
        let (base, server) = serve_once(200, &body);
        let account = block_on(cancel_subscription_with(&client(), &base, "dev-1")).unwrap();
        let req = server.join().unwrap();
        assert_eq!(req.request_line, "POST /subscription/cancel HTTP/1.1");
        assert_eq!(body_json(&req), json!({ "device_id": "dev-1" }));
        assert!(account.cancel_at_period_end);
    }

    #[test]
    fn cancel_maps_errors() {
        let (base, server) = serve_once(404, r#"{"error":"No subscription"}"#);
        let err = block_on(cancel_subscription_with(&client(), &base, "d")).unwrap_err();
        server.join().unwrap();
        assert_eq!(err, "No subscription found for this Mac.");
    }
}
