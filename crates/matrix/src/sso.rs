//! Single sign-on (`m.login.sso`): the homeserver sends the person to its identity provider
//! (Google, GitHub, a company's OIDC or SAML, or a Matrix Authentication Service) and back to
//! ChatCraft with a short-lived, single-use login token, which [`crate::MatrixConfig::login_token`]
//! exchanges for a session.
//!
//! - **Desktop** ([`SsoSignIn::start`]): ChatCraft listens once on `127.0.0.1` at a random port,
//!   the system browser opens the homeserver's SSO page with that address as the redirect, and
//!   the token arrives as `GET /?loginToken=…`.
//! - **Browser**: the page itself goes to [`sso_login_url`] with its own address as the redirect
//!   and picks the token up from its query string when it comes back.

use matrix_sdk::Client;
use matrix_sdk::ruma::api::client::session::get_login_types::v3::LoginType;

/// The homeserver's SSO page, which comes back to `redirect` with `?loginToken=…`.
/// Fails with a readable message when the homeserver doesn't offer single sign-on.
pub async fn sso_login_url(homeserver: &str, redirect: &str) -> Result<String, String> {
    let client = Client::builder().server_name_or_homeserver_url(homeserver).build().await.map_err(|e| format!("{homeserver}: {e}"))?;
    let types = client.matrix_auth().get_login_types().await.map_err(|e| format!("{homeserver}: {e}"))?;
    if !types.flows.iter().any(|f| matches!(f, LoginType::Sso(_))) {
        return Err(format!("{homeserver} doesn't offer single sign-on; use a password"));
    }
    client.matrix_auth().get_sso_login_url(redirect, None).await.map_err(|e| e.to_string())
}

/// The `loginToken` in a redirect's query string (`?loginToken=…&…` or the request target
/// `/?loginToken=…`), if there is a usable one.
pub fn login_token_from_query(query: &str) -> Option<String> {
    let q = query.split_once('?').map_or(query, |(_, q)| q);
    let q = q.split('#').next().unwrap_or_default();
    let raw = q.split('&').find_map(|p| p.strip_prefix("loginToken="))?;
    let token = percent_decode(raw)?;
    // Tokens are short opaque strings; anything else is not one.
    (!token.is_empty() && token.len() <= 512 && token.chars().all(|c| c.is_ascii_graphic())).then_some(token)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        match c {
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(not(target_family = "wasm"))]
pub use native::{SsoEvent, SsoSignIn};

#[cfg(not(target_family = "wasm"))]
mod native {
    use std::sync::mpsc;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use super::{login_token_from_query, sso_login_url};

    /// How long the browser has to come back.
    const WAIT: Duration = Duration::from_secs(300);
    /// The most of one request that is read (only the request line matters).
    const MAX_REQUEST: usize = 8 * 1024;

    /// What a desktop SSO sign-in reports, in order: `Open` once, then `Token` or `Failed`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum SsoEvent {
        /// Open this address in the person's browser.
        Open(String),
        /// The login token, for [`crate::MatrixConfig::login_token`].
        Token(String),
        Failed(String),
    }

    /// A desktop SSO sign-in running in the background. Dropping it stops listening (within the
    /// wait limit at the latest).
    pub struct SsoSignIn {
        rx: mpsc::Receiver<SsoEvent>,
    }

    impl SsoSignIn {
        /// Starts listening on a random local port and asks the homeserver for its SSO page.
        pub fn start(homeserver: &str) -> Result<SsoSignIn, String> {
            let (tx, rx) = mpsc::channel();
            let homeserver = homeserver.trim().to_string();
            std::thread::Builder::new()
                .name("chatcraft-sso".into())
                .spawn(move || {
                    let ev = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                        Ok(rt) => rt.block_on(run(&homeserver, &tx)),
                        Err(e) => SsoEvent::Failed(format!("runtime: {e}")),
                    };
                    let _ = tx.send(ev);
                })
                .map_err(|e| e.to_string())?;
            Ok(SsoSignIn { rx })
        }

        /// The next event, if one has arrived.
        pub fn poll(&mut self) -> Option<SsoEvent> {
            match self.rx.try_recv() {
                Ok(ev) => Some(ev),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(SsoEvent::Failed("single sign-on stopped".into())),
            }
        }
    }

    async fn run(homeserver: &str, tx: &mpsc::Sender<SsoEvent>) -> SsoEvent {
        // Loopback only: nothing off this computer can reach the listener.
        let listener = match TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(l) => l,
            Err(e) => return SsoEvent::Failed(format!("couldn't listen for the browser: {e}")),
        };
        let port = match listener.local_addr() {
            Ok(a) => a.port(),
            Err(e) => return SsoEvent::Failed(e.to_string()),
        };
        let url = match sso_login_url(homeserver, &format!("http://127.0.0.1:{port}/")).await {
            Ok(u) => u,
            Err(e) => return SsoEvent::Failed(e),
        };
        if tx.send(SsoEvent::Open(url)).is_err() {
            return SsoEvent::Failed("cancelled".into());
        }
        match tokio::time::timeout(WAIT, wait_for_token(&listener)).await {
            Ok(Some(token)) => SsoEvent::Token(token),
            Ok(None) => SsoEvent::Failed("the browser didn't send a sign-in token".into()),
            Err(_) => SsoEvent::Failed("single sign-on timed out; try again".into()),
        }
    }

    /// Answers requests until one carries a login token (browsers also ask for `/favicon.ico`).
    pub(crate) async fn wait_for_token(listener: &TcpListener) -> Option<String> {
        loop {
            let (stream, _) = listener.accept().await.ok()?;
            if let Ok(Some(token)) = tokio::time::timeout(Duration::from_secs(10), answer(stream)).await {
                return Some(token);
            }
        }
    }

    async fn answer(mut stream: TcpStream) -> Option<String> {
        let mut buf = Vec::with_capacity(1024);
        let mut chunk = [0u8; 1024];
        // Read up to the end of the request line (or the cap).
        while !buf.contains(&b'\n') && buf.len() < MAX_REQUEST {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(chunk.get(..n)?);
        }
        let line = String::from_utf8_lossy(&buf);
        let target = line.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split(' ').next()).unwrap_or_default();
        let token = login_token_from_query(target);
        let (status, body) = match token {
            Some(_) => ("200 OK", PAGE_DONE),
            None => ("404 Not Found", "Not found"),
        };
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.shutdown().await;
        token
    }

    const PAGE_DONE: &str = "<!doctype html><meta charset=utf-8><title>ChatCraft</title>\
<body style=\"font-family:system-ui,sans-serif;max-width:32em;margin:4em auto;padding:0 1em\">\
<h1>Signed in</h1><p>You can close this tab and go back to ChatCraft.</p></body>";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_come_from_the_query_string() {
        assert_eq!(login_token_from_query("/?loginToken=abc123").as_deref(), Some("abc123"));
        assert_eq!(login_token_from_query("?x=1&loginToken=a%2Bb&y=2").as_deref(), Some("a+b"));
        assert_eq!(login_token_from_query("loginToken=t#frag").as_deref(), Some("t"));
        for bad in ["", "/", "/?loginToken=", "/?loginToken=%zz", "/?other=1", "/?loginToken=a%20b", "/?loginToken=%FF"] {
            assert_eq!(login_token_from_query(bad), None, "{bad}");
        }
        assert_eq!(login_token_from_query(&format!("/?loginToken={}", "a".repeat(600))), None);
        // Hostile input never panics.
        for s in ["%", "%4", "?loginToken=%", "?loginToken=%4", "?loginToken=é%"] {
            let _ = login_token_from_query(s);
        }
    }

    #[cfg(not(target_family = "wasm"))]
    #[tokio::test]
    async fn the_local_listener_takes_the_token_and_ignores_other_requests() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = tokio::spawn(async move {
            let mut pages = Vec::new();
            for req in ["GET /favicon.ico HTTP/1.1\r\nHost: x\r\n\r\n", "junk\r\n\r\n", "GET /?loginToken=tok_42 HTTP/1.1\r\nHost: x\r\n\r\n"] {
                let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                s.write_all(req.as_bytes()).await.unwrap();
                let mut page = String::new();
                s.read_to_string(&mut page).await.unwrap();
                pages.push(page);
            }
            pages
        });
        assert_eq!(native::wait_for_token(&listener).await.as_deref(), Some("tok_42"));
        let pages = browser.await.unwrap();
        assert!(pages[0].starts_with("HTTP/1.1 404"));
        assert!(pages[2].starts_with("HTTP/1.1 200") && pages[2].contains("Signed in"));
    }
}
