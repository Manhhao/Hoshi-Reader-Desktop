use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::sync::client::{self, GoogleDriveError};
use crate::sync::gdrive::manager;

const SERVICE: &str = "hoshi-reader";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const CLIENT_ID: &str = match option_env!("HOSHI_GOOGLE_CLIENT_ID") {
    Some(id) => id,
    None => "",
};
const CLIENT_SECRET: &str = match option_env!("HOSHI_GOOGLE_CLIENT_SECRET") {
    Some(secret) => secret,
    None => "",
};

static RE_CLIENT_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+-[a-z0-9]+\.apps\.googleusercontent\.com$").unwrap());

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

pub fn init_store() {
    #[cfg(target_os = "macos")]
    keyring_core::set_default_store(apple_native_keyring_store::keychain::Store::new().unwrap());
    #[cfg(windows)]
    keyring_core::set_default_store(windows_native_keyring_store::Store::new().unwrap());
}

fn entry(key: &str) -> keyring_core::Entry {
    keyring_core::Entry::new(SERVICE, key).unwrap()
}

fn get_token(key: &str) -> Option<String> {
    entry(key).get_password().ok()
}

fn save_token(key: &str, value: &str) {
    entry(key).set_password(value).ok();
}

pub fn clear_tokens() {
    for key in ["accessToken", "refreshToken", "clientId"] {
        entry(key).delete_credential().ok();
    }
}

fn not_authenticated() -> String {
    "Not authenticated\nPlease sign in".to_string()
}

pub fn is_authenticated() -> bool {
    get_token("accessToken").is_some() && get_token("refreshToken").is_some()
}

pub fn access_token() -> Result<String, String> {
    get_token("accessToken").ok_or_else(not_authenticated)
}

pub async fn authenticate() -> Result<(), String> {
    if !RE_CLIENT_ID.is_match(CLIENT_ID) {
        return Err("Invalid Client ID format".to_string());
    }

    manager::stop().await;
    let result = authorize().await;
    manager::start();
    result
}

async fn authorize() -> Result<(), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");

    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = Uuid::new_v4().simple().to_string();

    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        urlencoding::encode(CLIENT_ID),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(SCOPE),
        challenge,
        state
    );
    open::that(auth_url).map_err(|e| e.to_string())?;

    let code = tauri::async_runtime::spawn_blocking(move || wait_for_code(listener, &state))
        .await
        .map_err(|e| e.to_string())??;

    let response = post_token_form(
        &[
            ("code", code.as_str()),
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.as_str()),
            ("grant_type", "authorization_code"),
        ],
        Duration::from_secs(60),
    )
    .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Token exchange failed: {}",
            response.status().as_u16()
        ));
    }
    let tokens: TokenResponse = response.json().await.map_err(|e| e.to_string())?;
    manager::reset_connection(false).map_err(|error| error.to_string())?;
    clear_tokens();
    save_token("accessToken", &tokens.access_token);
    if let Some(refresh) = &tokens.refresh_token {
        save_token("refreshToken", refresh);
    }
    Ok(())
}

async fn post_token_form(
    params: &[(&str, &str)],
    timeout: Duration,
) -> Result<reqwest::Response, String> {
    reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .timeout(timeout)
        .form(params)
        .send()
        .await
        .map_err(|e| e.to_string())
}

pub async fn refresh_access_token() -> Result<String, GoogleDriveError> {
    let connection = client::connection_id();

    let Some(refresh_token) = get_token("refreshToken") else {
        return Err(not_authenticated().into());
    };

    let response = client::cancellable(post_token_form(
        &[
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
        ],
        Duration::from_secs(10),
    ))
    .await??;

    client::check_connection(connection)?;
    crate::sync::task::check_cancellation()?;

    if response.status().as_u16() != 200 {
        clear_tokens();
        return Err("Failed to refresh token\nPlease sign in again"
            .to_string()
            .into());
    }
    let tokens: TokenResponse = response.json().await.map_err(|e| e.to_string())?;
    save_token("accessToken", &tokens.access_token);
    Ok(tokens.access_token)
}

fn wait_for_code(listener: TcpListener, state: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).ok();
                let mut line = String::new();
                if BufReader::new(&stream).read_line(&mut line).is_err() {
                    continue;
                }
                let query = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|path| path.split_once('?'))
                    .map(|(_, q)| q.to_string());
                let Some(query) = query else {
                    respond(&mut stream, 404, "");
                    continue;
                };
                let params: HashMap<String, String> = query
                    .split('&')
                    .filter_map(|kv| kv.split_once('='))
                    .map(|(k, v)| {
                        let decoded = urlencoding::decode(v)
                            .map(|d| d.into_owned())
                            .unwrap_or_else(|_| v.to_string());
                        (k.to_string(), decoded)
                    })
                    .collect();
                if params.get("state").map(String::as_str) != Some(state) {
                    respond(&mut stream, 404, "");
                    continue;
                }
                if let Some(code) = params.get("code") {
                    respond(
                        &mut stream,
                        200,
                        "<html><body>You can close this window and return to Hoshi Reader.</body></html>",
                    );
                    return Ok(code.clone());
                }
                let error = params
                    .get("error")
                    .cloned()
                    .unwrap_or_else(|| "access_denied".to_string());
                respond(
                    &mut stream,
                    200,
                    "<html><body>You can close this window.</body></html>",
                );
                return Err(format!("Authorization failed: {error}"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("No callback URL received".to_string());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).ok();
}
