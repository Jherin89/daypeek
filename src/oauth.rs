//! OAuth helpers for the bring-your-own-app providers (TickTick, Microsoft To
//! Do, Google Tasks).
//!
//! `daypeek` ships **no** client id or secret. You register your own app with the
//! service, then either paste an access token into an env var, or run
//! `daypeek auth <provider>` once: it opens the consent page, catches the
//! redirect on `http://127.0.0.1:<port>/callback`, and prints the refresh
//! token for *you* to store (env var or Keychain). Nothing is written to disk.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::http::{decode, enc, Http};

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

/// POST a form to a token endpoint.
pub fn token_request(
    http: &Http,
    url: &str,
    form: &[(&str, &str)],
    basic: Option<(&str, &str)>,
) -> Result<TokenResponse> {
    let mut req = http.post(url).set("Accept", "application/json");
    if let Some((user, pass)) = basic {
        let b = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        req = req.set("Authorization", &format!("Basic {b}"));
    }
    match req.send_form(form) {
        Ok(resp) => decode(resp),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let short: String = body.chars().take(200).collect();
            Err(Error::AuthMissing(format!(
                "token endpoint returned HTTP {code}: {short}"
            )))
        }
        Err(e) => Err(e.into()),
    }
}

/// Exchange a refresh token for an access token.
pub fn refresh(
    http: &Http,
    token_url: &str,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
    scope: Option<&str>,
) -> Result<String> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", refresh_token),
    ];
    if let Some(s) = client_secret {
        form.push(("client_secret", s));
    }
    if let Some(s) = scope {
        form.push(("scope", s));
    }
    Ok(token_request(http, token_url, &form, None)?.access_token)
}

/// Static description of a provider's OAuth endpoints.
pub struct Flow {
    pub provider: &'static str,
    pub authorize_url: String,
    pub token_url: String,
    pub scope: &'static str,
    /// Extra query params for the authorize URL.
    pub extra: &'static [(&'static str, &'static str)],
    /// Send client credentials with HTTP Basic (TickTick) instead of the body.
    pub basic_auth: bool,
    pub pkce: bool,
    /// `127.0.0.1` (Google, TickTick) or `localhost` (Microsoft: its portal
    /// only accepts `http://localhost/...` loopback redirects).
    pub redirect_host: &'static str,
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("OS random number generator");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// Run the loopback authorization-code flow and return the token response.
pub fn loopback(
    http: &Http,
    flow: &Flow,
    client_id: &str,
    client_secret: Option<&str>,
    port: u16,
) -> Result<TokenResponse> {
    let redirect = format!("http://{}:{port}/callback", flow.redirect_host);
    let state = random_token(16);
    let verifier = random_token(48);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));

    let mut url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
        flow.authorize_url,
        enc(client_id),
        enc(&redirect),
        enc(flow.scope),
        enc(&state)
    );
    if flow.pkce {
        url.push_str(&format!(
            "&code_challenge={challenge}&code_challenge_method=S256"
        ));
    }
    for (k, v) in flow.extra {
        url.push_str(&format!("&{k}={}", enc(v)));
    }

    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| Error::Config(format!("cannot listen on 127.0.0.1:{port}: {e}")))?;
    eprintln!(
        "Open this URL to authorize daypeek ({}):\n\n  {url}\n",
        flow.provider
    );
    open_browser(&url);
    eprintln!("Waiting for the redirect to {redirect} …");

    // Browsers may probe /favicon.ico first; wait for the real callback.
    let (mut stream, path) = loop {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        let mut request_line = String::new();
        BufReader::new(&stream).read_line(&mut request_line)?;
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .to_string();
        if path.starts_with("/callback") {
            break (stream, path);
        }
        let _ = write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    };
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());

    let ok = get("state").as_deref() == Some(state.as_str()) && get("code").is_some();
    let body = if ok {
        "daypeek: authorized. You can close this tab."
    } else {
        "daypeek: authorization failed. Check the terminal."
    };
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if let Some(err) = get("error") {
        return Err(Error::AuthMissing(format!("authorization denied: {err}")));
    }
    if !ok {
        return Err(Error::AuthMissing("state mismatch or missing code".into()));
    }
    let code = get("code").unwrap_or_default();

    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", &redirect),
        ("scope", flow.scope),
    ];
    if flow.pkce {
        form.push(("code_verifier", &verifier));
    }
    if flow.basic_auth {
        // TickTick documents Basic auth *and* shows the credentials in the body;
        // send both so either reading of the spec works.
        let secret = client_secret.unwrap_or_default();
        form.push(("client_id", client_id));
        form.push(("client_secret", secret));
        token_request(http, &flow.token_url, &form, Some((client_id, secret)))
    } else {
        form.push(("client_id", client_id));
        if let Some(s) = client_secret {
            form.push(("client_secret", s));
        }
        token_request(http, &flow.token_url, &form, None)
    }
}

fn open_browser(url: &str) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
