//! Interface tests: the real app, headless, driven through egui_kittest.

use chatcraft_engine::{Pane, Session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::ChatApp;

fn harness() -> Harness<'static, ChatApp> {
    let app = ChatApp::new(Session::demo());
    let mut h = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(|ui, app: &mut ChatApp| app.ui(ui), app);
    h.run_steps(4);
    h
}

#[test]
fn renders_the_demo_workspace() {
    let mut h = harness();
    h.run_steps(2);
    for text in ["Kestrel Labs", "launch-plan", "Welcome to the new workspace", "Add channels", "engineering (1 mentions)"] {
        assert!(h.query_all_by_label_contains(text).next().is_some(), "{text} is on screen");
    }
}

#[test]
fn typing_and_enter_sends_a_message() {
    let mut h = harness();
    h.state_mut().session.focused = true;
    let c = h.state().session.view.channel.unwrap();
    let before = h.state().session.state.timeline(c).len();
    h.state_mut().st.focus_composer = true;
    h.run_steps(2);
    h.event(egui::Event::Text("Hello from the test".into()));
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let tl = h.state().session.state.timeline(c);
    assert_eq!(tl.len(), before + 1, "Enter sends");
    assert_eq!(tl.last().unwrap().text, "Hello from the test");
    assert!(!h.state().session.drafts.contains_key(&c), "the draft is cleared after sending");
}

#[test]
fn ui_commands_open_dialogs_and_panes() {
    let mut h = harness();
    h.state_mut().run("ui.quickSwitcher", &json!({})).unwrap();
    h.run_steps(2);
    assert!(h.query_all_by_label_contains("Jump to").next().is_some());
    h.state_mut().run("ui.closeDialog", &json!({})).unwrap();
    let m = h.state().session.state.messages.values().find(|m| m.reply_count > 0).unwrap().id;
    h.state_mut().run("thread.open", &json!({ "message": m.0 })).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().session.view.pane, Pane::Thread);
    assert!(h.query_all_by_label_contains("Also send to").next().is_some());
    h.state_mut().run("ui.toggleTheme", &json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().tokens.mode, crate::Mode::Dark);
    for id in ["ui.newChannel", "ui.browseChannels", "ui.newMessage", "ui.people", "ui.setStatus", "ui.shortcuts", "ui.recovery", "ui.devices"] {
        h.state_mut().run(id, &json!({})).unwrap();
        h.run_steps(2);
        h.state_mut().run("ui.closeDialog", &json!({})).unwrap();
    }
    for id in ["nav.activity", "nav.saved", "nav.details", "search.messages"] {
        let _ = h.state_mut().run(id, &json!({ "query": "launch" }));
        h.run_steps(2);
    }
    assert!(h.state_mut().run("ui.nope", &json!({})).is_err());
}

/// Every emoji ChatCraft offers must be drawable with the fonts it ships (no tofu). kittest's
/// missing-glyph policy panics, so each glyph is laid out in its own harness.
#[test]
fn every_offered_emoji_has_a_glyph() {
    let mut missing = Vec::new();
    for (name, glyph) in [("letter", "A")].iter().chain(chatcraft_engine::text::emoji::ALL) {
        let ok = std::panic::catch_unwind(|| {
            let mut h = Harness::builder().with_size(egui::vec2(200.0, 60.0)).build_ui(|ui| {
                ui.label(*glyph);
            });
            h.run_steps(1);
        })
        .is_ok();
        if !ok {
            missing.push(format!("{name} {glyph} {:?}", glyph.chars().map(|c| format!("U+{:04X}", c as u32)).collect::<Vec<_>>()));
        }
    }
    assert!(missing.is_empty(), "no glyph for:\n{}", missing.join("\n"));
}

/// `sas::UNDRAWABLE` lists exactly the SAS emoji the bundled fonts can't draw.
#[test]
fn sas_glyphs_are_known() {
    let mut missing = Vec::new();
    for g in crate::sas::TABLE {
        let ok = std::panic::catch_unwind(|| {
            let mut h = Harness::builder().with_size(egui::vec2(120.0, 60.0)).build_ui(|ui| {
                ui.label(g);
            });
            h.run_steps(1);
        })
        .is_ok();
        if !ok {
            missing.push(g);
        }
    }
    assert_eq!(missing, crate::sas::UNDRAWABLE.to_vec(), "update sas::UNDRAWABLE");
}

mod shell {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use chatcraft_engine::{Loopback, Session};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use crate::{Credentials, Shell, SsoPoll, Start};

    fn shell(start: Start) -> Harness<'static, Shell> {
        harness(new_shell(start))
    }

    fn new_shell(start: Start) -> Shell {
        // "Signing in" against the in-process hub: an invalid handle fails like a bad password.
        let connect = Box::new(|c: &Credentials| Ok(Session::connect(Box::new(Loopback::demo()), Loopback::hello(&c.user))));
        Shell::new(start, connect)
    }

    fn harness(s: Shell) -> Harness<'static, Shell> {
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, s: &mut Shell| s.ui(ui), s);
        h.run_steps(3);
        h
    }

    #[test]
    fn sign_in_screen_then_demo() {
        let mut h = shell(Start::SignIn(Credentials::default()));
        assert!(h.query_all_by_label_contains("Sign in with your Matrix account").next().is_some());
        assert!(h.state_mut().app().is_none());
        h.get_by_label("Try the demo workspace instead").click();
        h.run_steps(4);
        assert!(h.state_mut().app().is_some(), "the demo link opens a workspace");
    }

    #[test]
    fn a_failed_sign_in_returns_to_the_form_with_the_error() {
        let mut h = shell(Start::Connect(Credentials { homeserver: "hs".into(), user: "not a valid handle!".into(), ..Credentials::default() }));
        h.run_steps(4);
        assert!(h.state_mut().app().is_none());
        assert!(h.query_all_by_label_contains("handle").next().is_some(), "the error is shown");
    }

    #[test]
    fn signing_in_remembers_and_signing_out_forgets() {
        let remembered = Rc::new(Cell::new(false));
        let forgotten = Rc::new(Cell::new(false));
        let mut s = new_shell(Start::Connect(Credentials {
            homeserver: "hs".into(),
            user: "morgan".into(),
            password: "pw".into(),
            ..Credentials::default()
        }));
        let (r, f) = (Rc::clone(&remembered), Rc::clone(&forgotten));
        s.on_signed_in = Some(Box::new(move |c| r.set(c.password.is_empty())));
        s.on_signed_out = Some(Box::new(move || f.set(true)));
        let mut h = harness(s);
        h.run_steps(4);
        assert!(h.state_mut().app().is_some(), "signed in");
        assert!(remembered.get(), "the account is remembered, without its password");
        // Signing out of the in-process workspace has nothing to tell a server: it just leaves.
        let app = h.state_mut().app().unwrap();
        app.st.signed_out = true;
        h.run_steps(3);
        assert!(forgotten.get());
        assert!(h.state_mut().app().is_none());
        assert!(h.query_all_by_label_contains("Sign in with your Matrix account").next().is_some());
    }

    /// A fake single sign-on: opens a page, then hands over a token (or fails).
    fn sso_shell(outcome: SsoPoll, connected_with: Rc<RefCell<Option<Credentials>>>) -> Shell {
        let seen = Rc::clone(&connected_with);
        let connect = Box::new(move |c: &Credentials| {
            *seen.borrow_mut() = Some(c.clone());
            Ok(Session::connect(Box::new(Loopback::demo()), Loopback::hello(&c.user)))
        });
        let mut s = Shell::new(Start::SignIn(Credentials { homeserver: "hs".into(), user: "morgan".into(), ..Credentials::default() }), connect);
        s.sso = Some(Box::new(move |_c: &Credentials| {
            let mut steps = vec![outcome.clone(), SsoPoll::Waiting, SsoPoll::Open("https://hs/sso".into())];
            Ok(Box::new(move || steps.pop().unwrap_or(SsoPoll::Waiting)) as Box<dyn FnMut() -> SsoPoll>)
        }));
        s
    }

    #[test]
    fn single_sign_on_waits_for_the_browser_then_connects_with_the_token() {
        let connected = Rc::new(RefCell::new(None));
        let mut h = harness(sso_shell(SsoPoll::Token("tok".into()), Rc::clone(&connected)));
        h.get_by_label("Sign in with single sign-on").click();
        h.run_steps(2);
        assert!(h.query_all_by_label_contains("in your browser").next().is_some(), "waiting for the browser");
        h.run_steps(6);
        let c = connected.borrow().clone().unwrap();
        assert_eq!((c.user.as_str(), c.login_token.as_str(), c.password.as_str()), ("morgan", "tok", ""));
        assert!(h.state_mut().app().is_some(), "signed in");
    }

    #[test]
    fn a_failed_single_sign_on_returns_to_the_form() {
        let connected = Rc::new(RefCell::new(None));
        let mut h = harness(sso_shell(SsoPoll::Failed("hs doesn't offer single sign-on".into()), Rc::clone(&connected)));
        h.get_by_label("Sign in with single sign-on").click();
        h.run_steps(8);
        assert!(connected.borrow().is_none());
        assert!(h.query_all_by_label_contains("doesn't offer single sign-on").next().is_some(), "the error is shown");
    }

    #[test]
    fn single_sign_on_can_be_cancelled() {
        let connected = Rc::new(RefCell::new(None));
        let mut s = sso_shell(SsoPoll::Token("tok".into()), Rc::clone(&connected));
        // This sign-on never finishes.
        s.sso = Some(Box::new(|_c: &Credentials| Ok(Box::new(|| SsoPoll::Waiting) as Box<dyn FnMut() -> SsoPoll>)));
        let mut h = harness(s);
        h.get_by_label("Sign in with single sign-on").click();
        h.run_steps(2);
        h.get_by_label("Cancel").click();
        h.run_steps(3);
        assert!(h.query_all_by_label_contains("Sign in with your Matrix account").next().is_some());
        assert!(connected.borrow().is_none());
    }
}
