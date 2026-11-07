//! Single sign-on against a real homeserver with an identity provider.
//!
//! Skipped unless `CHATCRAFT_TEST_HOMESERVER` and `CHATCRAFT_TEST_SSO_USER` (the account the
//! identity provider signs everyone in as) are set. `docs/matrix.md` › Testing shows a local
//! Synapse with a test-only CAS provider. `curl -L` stands in for the browser: it follows the
//! homeserver → identity provider → homeserver → ChatCraft redirects.

use std::path::Path;
use std::time::{Duration, Instant};

use chatcraft_engine::{Loopback, Session};
use chatcraft_matrix::sso::{SsoEvent, SsoSignIn};
use chatcraft_matrix::{MatrixConfig, MatrixTransport, SecretStore};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// Runs a desktop SSO sign-in with curl as the browser; returns the login token.
fn sso_token(hs: &str) -> String {
    let mut sso = SsoSignIn::start(hs).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut browser = None;
    loop {
        assert!(Instant::now() < deadline, "no token");
        match sso.poll() {
            Some(SsoEvent::Open(url)) => {
                assert!(url.contains("/login/sso/redirect") && url.contains("127.0.0.1"), "{url}");
                browser =
                    Some(std::thread::spawn(move || std::process::Command::new("curl").args(["-sL", "--max-redirs", "10", &url]).output().unwrap()));
            }
            Some(SsoEvent::Token(t)) => {
                let page = browser.take().unwrap().join().unwrap();
                assert!(String::from_utf8_lossy(&page.stdout).contains("Signed in"), "the browser is told it can close the tab");
                return t;
            }
            Some(SsoEvent::Failed(e)) => panic!("single sign-on failed: {e}"),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn config(hs: &str, user: &str, token: &str, store: &Path) -> MatrixConfig {
    let mut cfg = MatrixConfig::new(hs, user, "", store.to_path_buf());
    cfg.login_token = token.into();
    // Tests keep secrets in files, never in the developer's keychain.
    cfg.secrets = SecretStore::Files;
    cfg
}

#[test]
fn single_sign_on_signs_in_and_is_remembered() {
    let (Some(hs), Some(user)) = (env("CHATCRAFT_TEST_HOMESERVER"), env("CHATCRAFT_TEST_SSO_USER")) else {
        eprintln!("skipped: set CHATCRAFT_TEST_HOMESERVER and CHATCRAFT_TEST_SSO_USER (a homeserver with single sign-on)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("chatcraft-sso-{}", std::process::id()));
    let store = dir.join(&user);
    let token = sso_token(&hs);
    let mut s = Session::connect(Box::new(MatrixTransport::connect(config(&hs, &user, &token, &store)).unwrap()), Loopback::hello(&user));
    assert!(s.wait_ready(Duration::from_secs(60)), "sign-in with the token failed: {:?}", s.error);
    drop(s);

    // The next launch resumes the saved session: no token, no password.
    let mut s = Session::connect(Box::new(MatrixTransport::connect(config(&hs, &user, "", &store)).unwrap()), Loopback::hello(&user));
    assert!(s.wait_ready(Duration::from_secs(60)), "the saved session didn't resume: {:?}", s.error);
    drop(s);

    // A token is single use.
    let again = dir.join("again");
    let mut s = Session::connect(Box::new(MatrixTransport::connect(config(&hs, &user, &token, &again)).unwrap()), Loopback::hello(&user));
    assert!(!s.wait_ready(Duration::from_secs(30)), "a used token signed in again");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn signing_in_as_someone_else_is_refused_and_leaves_nothing_behind() {
    let (Some(hs), Some(user)) = (env("CHATCRAFT_TEST_HOMESERVER"), env("CHATCRAFT_TEST_SSO_USER")) else {
        eprintln!("skipped: set CHATCRAFT_TEST_HOMESERVER and CHATCRAFT_TEST_SSO_USER");
        return;
    };
    let store = std::env::temp_dir().join(format!("chatcraft-sso-other-{}", std::process::id()));
    let token = sso_token(&hs);
    let other = format!("not-{user}");
    let mut s = Session::connect(Box::new(MatrixTransport::connect(config(&hs, &other, &token, &store)).unwrap()), Loopback::hello(&other));
    assert!(!s.wait_ready(Duration::from_secs(30)));
    let why = format!("{:?} {:?}", s.error, s.link_status());
    assert!(why.contains("signed you in as") && why.contains(&user), "{why}");
    assert!(!store.exists(), "the store for the wrong account was removed");
}
