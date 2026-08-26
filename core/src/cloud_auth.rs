//! Cloud login via Firebase Authentication (Google Identity Toolkit REST API).
//!
//! NEXORA's optional cloud accounts live in the user's own Firebase project, so
//! a person can log in from any machine and the developer manages everyone from
//! the Firebase console. There is **no Firebase SDK** here — just the documented
//! HTTPS REST endpoints, called with the same blocking `ureq` client the rest of
//! core uses.
//!
//! The Firebase Web API key is NOT a secret (Google designs it to ship inside
//! client apps); it only identifies the project. All real security — password
//! hashing, rate limiting, per-user tokens — is enforced on Google's servers.
//!
//! Accounts are keyed by **email + password** (Firebase's identifier). This path
//! requires internet at login time; the offline local lock lives in [`crate::auth`].

use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tiny_http::{Header, Response, Server};

const BASE: &str = "https://identitytoolkit.googleapis.com/v1/accounts";
/// Google OAuth 2.0 endpoints (native-app loopback flow, RFC 8252).
const GOOGLE_AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN: &str = "https://oauth2.googleapis.com/token";

/// A signed-in cloud user (the useful subset of Firebase's auth response).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudUser {
    pub email: String,
    /// Firebase user id (`localId`).
    pub uid: String,
    /// Short-lived (≈1h) ID token (JWT) for authenticated calls.
    pub id_token: String,
    /// Long-lived refresh token.
    pub refresh_token: String,
}

/// Firebase's success payload (a superset; we read what we need).
#[derive(Deserialize)]
struct AuthResp {
    #[serde(rename = "localId")]
    local_id: String,
    email: Option<String>,
    #[serde(rename = "idToken")]
    id_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
}

impl From<AuthResp> for CloudUser {
    fn from(r: AuthResp) -> Self {
        CloudUser {
            email: r.email.unwrap_or_default(),
            uid: r.local_id,
            id_token: r.id_token,
            refresh_token: r.refresh_token,
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build()
}

/// Turn a Firebase error code into a friendly, user-facing message. Firebase
/// sometimes suffixes detail after " : " (e.g. `WEAK_PASSWORD : Password ...`).
pub fn friendly_error(message: &str) -> String {
    let code = message.split(" : ").next().unwrap_or(message).trim();
    match code {
        "EMAIL_EXISTS" => "That email already has an account — try logging in.".into(),
        "EMAIL_NOT_FOUND" | "INVALID_PASSWORD" | "INVALID_LOGIN_CREDENTIALS" => {
            "Incorrect email or password.".into()
        }
        "INVALID_EMAIL" => "That doesn't look like a valid email address.".into(),
        "MISSING_EMAIL" => "Please enter your email.".into(),
        "MISSING_PASSWORD" => "Please enter your password.".into(),
        "WEAK_PASSWORD" => "Password should be at least 6 characters.".into(),
        "USER_DISABLED" => "This account has been disabled.".into(),
        "OPERATION_NOT_ALLOWED" => {
            "Email/password sign-in isn't enabled for this project yet.".into()
        }
        "TOO_MANY_ATTEMPTS_TRY_LATER" => {
            "Too many attempts — please wait a bit and try again.".into()
        }
        "TOKEN_EXPIRED" | "INVALID_ID_TOKEN" => "Your session expired — please log in again.".into(),
        other => format!("Authentication failed ({other})."),
    }
}

/// POST a JSON body to a Firebase accounts endpoint and parse the response,
/// mapping Firebase/transport errors into friendly [`CoreError::Provider`]s.
///
/// Uses `send_string` + `into_string` + `serde_json` (rather than ureq's `json`
/// feature) to match the rest of core, which keeps that feature off.
fn post(url: &str, body: serde_json::Value) -> Result<CloudUser> {
    let body_str = body.to_string();
    let req = agent().post(url).set("Content-Type", "application/json");
    match req.send_string(&body_str) {
        Ok(resp) => {
            let text = resp
                .into_string()
                .map_err(|e| CoreError::Provider(format!("read response: {e}")))?;
            serde_json::from_str::<AuthResp>(&text)
                .map(CloudUser::from)
                .map_err(|e| CoreError::Provider(format!("Unexpected response from server: {e}")))
        }
        Err(ureq::Error::Status(_, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str().map(String::from))
                })
                .unwrap_or_else(|| "unknown error".into());
            Err(CoreError::Provider(friendly_error(&msg)))
        }
        Err(_transport) => Err(CoreError::Provider(
            "Couldn't reach the login server — check your internet connection.".into(),
        )),
    }
}

/// Create a new cloud account (Firebase `signUp`).
pub fn sign_up(api_key: &str, email: &str, password: &str) -> Result<CloudUser> {
    let url = format!("{BASE}:signUp?key={api_key}");
    post(
        &url,
        serde_json::json!({ "email": email.trim(), "password": password, "returnSecureToken": true }),
    )
}

/// Log in to an existing cloud account (Firebase `signInWithPassword`).
pub fn sign_in(api_key: &str, email: &str, password: &str) -> Result<CloudUser> {
    let url = format!("{BASE}:signInWithPassword?key={api_key}");
    post(
        &url,
        serde_json::json!({ "email": email.trim(), "password": password, "returnSecureToken": true }),
    )
}

/// Change the password for the account behind `id_token` (Firebase `update`).
/// The caller should pass a *fresh* id token (re-authenticate first), since ID
/// tokens expire after ~1 hour.
pub fn change_password(api_key: &str, id_token: &str, new_password: &str) -> Result<CloudUser> {
    let url = format!("{BASE}:update?key={api_key}");
    post(
        &url,
        serde_json::json!({ "idToken": id_token, "password": new_password, "returnSecureToken": true }),
    )
}

/// Send a password-reset email (Firebase `sendOobCode`). Firebase emails the user
/// a secure reset link — the reset itself happens on Google's hosted page, so
/// there is nothing more for NEXORA to do.
///
/// If the email isn't registered we still return `Ok(())` rather than an error,
/// so the UI can't be used to probe which emails have accounts.
pub fn send_password_reset(api_key: &str, email: &str) -> Result<()> {
    let url = format!("{BASE}:sendOobCode?key={api_key}");
    let body = serde_json::json!({ "requestType": "PASSWORD_RESET", "email": email.trim() });
    let req = agent().post(&url).set("Content-Type", "application/json");
    match req.send_string(&body.to_string()) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(_, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let raw = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str().map(String::from))
                })
                .unwrap_or_else(|| "unknown error".into());
            let code = raw.split(" : ").next().unwrap_or(&raw).trim();
            // Don't reveal whether an email is registered — treat "not found" as sent.
            if code == "EMAIL_NOT_FOUND" {
                Ok(())
            } else {
                Err(CoreError::Provider(friendly_error(&raw)))
            }
        }
        Err(_transport) => Err(CoreError::Provider(
            "Couldn't reach the login server — check your internet connection.".into(),
        )),
    }
}

// ===========================================================================
// Sign in with Google (OAuth 2.0 loopback flow for native apps, RFC 8252)
// ===========================================================================

/// Sign in with Google. Opens the system browser to Google's consent screen
/// (Google forbids its login inside embedded webviews), catches the redirect on
/// a localhost server, exchanges the auth code for a Google access token, then
/// hands that to Firebase to mint a NEXORA session.
///
/// `open` is called with the URL to launch in the user's browser — the desktop
/// shell supplies it, so core stays GUI-agnostic.
pub fn login_with_google(
    client_id: &str,
    client_secret: &str,
    api_key: &str,
    open: &dyn Fn(&str),
) -> Result<CloudUser> {
    // A localhost listener on an OS-assigned port receives Google's redirect.
    let server = Server::http("127.0.0.1:0")
        .map_err(|e| CoreError::Provider(format!("start sign-in listener: {e}")))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| CoreError::Provider("no sign-in callback port".into()))?;
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let redirect_enc = format!("http%3A%2F%2F127.0.0.1%3A{port}");

    // Random state ties the redirect to this request (CSRF protection).
    let state: String = {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..16).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
    };

    let auth_url = format!(
        "{GOOGLE_AUTH}?client_id={client_id}&redirect_uri={redirect_enc}\
         &response_type=code&scope=openid%20email%20profile&state={state}&prompt=select_account"
    );
    open(&auth_url);

    let code = wait_for_code(&server, &state)?;
    let access_token = exchange_code(client_id, client_secret, &code, &redirect_uri)?;
    sign_in_with_google(api_key, &access_token, &redirect_uri)
}

/// Block until Google redirects back with `?code=…&state=…` (or a timeout).
fn wait_for_code(server: &Server, expected_state: &str) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    let page = "<!doctype html><html><body style=\"font-family:system-ui,sans-serif;\
        background:#0b0d10;color:#e2e8f0;text-align:center;padding-top:15vh\">\
        <h1 style=\"color:#e0803a\">NEXORA</h1>\
        <p>You're signed in. You can close this tab and return to the app.</p></body></html>";
    loop {
        if Instant::now() >= deadline {
            return Err(CoreError::Provider(
                "Google sign-in timed out — please try again.".into(),
            ));
        }
        match server.recv_timeout(Duration::from_millis(500)) {
            Ok(Some(req)) => {
                let params = parse_query(&req.url().to_string());
                if let Some(err) = params.get("error") {
                    let _ = req.respond(html_response(page));
                    return Err(CoreError::Provider(format!(
                        "Google sign-in was cancelled ({err})."
                    )));
                }
                if let Some(code) = params.get("code") {
                    let state_ok = params.get("state").map(|s| s == expected_state).unwrap_or(false);
                    let _ = req.respond(html_response(page));
                    if !state_ok {
                        return Err(CoreError::Provider(
                            "Sign-in state mismatch — please try again.".into(),
                        ));
                    }
                    return Ok(code.clone());
                }
                // Some other request (e.g. the browser's favicon probe) — ignore.
                let _ = req.respond(Response::from_string("").with_status_code(204));
            }
            Ok(None) => {} // recv timed out; loop and re-check the deadline
            Err(e) => return Err(CoreError::Provider(format!("sign-in listener error: {e}"))),
        }
    }
}

fn html_response(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let header =
        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).expect("header");
    Response::from_string(body).with_header(header)
}

/// Parse a `path?a=b&c=d` query string into a decoded map.
fn parse_query(url: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some((_, qs)) = url.split_once('?') {
        for pair in qs.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                map.insert(k.to_string(), urldecode(v));
            }
        }
    }
    map
}

/// Minimal percent-decoding — enough for an OAuth `code`/`state` value.
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push(h * 16 + l);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Exchange the OAuth authorization code for a Google access token.
fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<String> {
    match agent().post(GOOGLE_TOKEN).send_form(&[
        ("code", code),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ]) {
        Ok(resp) => {
            let text = resp
                .into_string()
                .map_err(|e| CoreError::Provider(format!("read token response: {e}")))?;
            let v: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| CoreError::Provider(format!("parse token response: {e}")))?;
            v.get("access_token")
                .and_then(|t| t.as_str())
                .map(String::from)
                .ok_or_else(|| CoreError::Provider("Google didn't return an access token.".into()))
        }
        Err(ureq::Error::Status(_, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("error_description")
                        .or_else(|| v.get("error"))
                        .and_then(|m| m.as_str().map(String::from))
                })
                .unwrap_or_else(|| "token exchange failed".into());
            Err(CoreError::Provider(format!("Google sign-in failed: {msg}")))
        }
        Err(_) => Err(CoreError::Provider(
            "Couldn't reach Google — check your internet connection.".into(),
        )),
    }
}

/// Exchange a Google access token for a NEXORA/Firebase session
/// (`accounts:signInWithIdp`). Using the access token (not the id token) means
/// Firebase verifies with Google directly, so no client-id whitelisting is needed.
fn sign_in_with_google(api_key: &str, access_token: &str, redirect_uri: &str) -> Result<CloudUser> {
    let url = format!("{BASE}:signInWithIdp?key={api_key}");
    post(
        &url,
        serde_json::json!({
            "postBody": format!("access_token={access_token}&providerId=google.com"),
            "requestUri": redirect_uri,
            "returnIdpCredential": true,
            "returnSecureToken": true,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urldecode_handles_percent_and_plus() {
        assert_eq!(urldecode("a%2Fb"), "a/b");
        assert_eq!(urldecode("hello+world"), "hello world");
        assert_eq!(urldecode("4%2F0Ab"), "4/0Ab");
        assert_eq!(urldecode("plain"), "plain");
    }

    #[test]
    fn parse_query_extracts_code_and_state() {
        let m = parse_query("/?code=4%2F0Axyz&state=deadbeef&scope=email");
        assert_eq!(m.get("code").map(String::as_str), Some("4/0Axyz"));
        assert_eq!(m.get("state").map(String::as_str), Some("deadbeef"));
    }

    #[test]
    fn friendly_errors_are_mapped() {
        assert_eq!(friendly_error("EMAIL_EXISTS"), "That email already has an account — try logging in.");
        assert_eq!(friendly_error("INVALID_LOGIN_CREDENTIALS"), "Incorrect email or password.");
        assert_eq!(friendly_error("EMAIL_NOT_FOUND"), "Incorrect email or password.");
        // Firebase's suffixed form is normalized to the base code.
        assert_eq!(
            friendly_error("WEAK_PASSWORD : Password should be at least 6 characters"),
            "Password should be at least 6 characters."
        );
        assert_eq!(
            friendly_error("OPERATION_NOT_ALLOWED"),
            "Email/password sign-in isn't enabled for this project yet."
        );
        // Unknown codes still surface something actionable.
        assert!(friendly_error("SOME_NEW_CODE").contains("SOME_NEW_CODE"));
    }
}
