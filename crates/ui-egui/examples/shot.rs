//! Headless screenshot of the real app (no window, no focus stealing).
//!
//! `cargo run -p chatcraft-ui-egui --example shot -- out.png [--dark] [--size 1440x900]
//!  [--cmd 'thread.open={"message":12}'] ...`
//!
//! Commands run in order before the shot (engine ids or `ui.*` ids), so any state can be shown.
//! `--matrix https://homeserver --user NAME [--password PW]` shows a real Matrix account instead
//! of the demo workspace.

use chatcraft_engine::Session;
use chatcraft_ui_egui::ChatApp;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| "chatcraft.png".into());
    if args.iter().any(|a| a == "--sign-in" || a == "--sso") {
        sign_in_shot(&out, args.iter().any(|a| a == "--dark"), args.iter().any(|a| a == "--sso"));
        return;
    }
    let (mut w, mut h) = (1440.0, 900.0);
    let mut cmds = Vec::new();
    let (mut matrix, mut user, mut password, mut dark) = (None, String::new(), String::new(), false);
    // `--verify-emoji`: show the verification dialog at the emoji step (sample emoji).
    let mut verify_demo = false;
    // `--typing`: Ada and Ravi are typing in the open conversation.
    let mut typing_demo = false;
    // `--seen`: Ada and Ravi have read up to the last message of the open conversation.
    let mut seen_demo = false;
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dark" => dark = true,
            "--verify-emoji" => verify_demo = true,
            "--typing" => typing_demo = true,
            "--seen" => seen_demo = true,
            "--matrix" => matrix = it.next().cloned(),
            "--user" => user = it.next().cloned().unwrap_or_default(),
            "--password" => password = it.next().cloned().unwrap_or_default(),
            "--size" => {
                if let Some((a, b)) = it.next().and_then(|s| s.split_once('x')) {
                    w = a.parse().unwrap_or(w);
                    h = b.parse().unwrap_or(h);
                }
            }
            "--cmd" => {
                if let Some(c) = it.next() {
                    cmds.push(c.clone());
                }
            }
            other => eprintln!("ignored argument {other}"),
        }
    }
    let session = match &matrix {
        Some(hs) => {
            let cfg = chatcraft_matrix::MatrixConfig::new(hs, &user, &password, chatcraft_matrix::default_store_dir(hs, &user));
            let link = match chatcraft_matrix::MatrixTransport::connect(cfg) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("matrix: {e}");
                    return;
                }
            };
            let mut s = Session::connect(Box::new(link), chatcraft_engine::Loopback::hello(&user));
            if !s.wait_ready(std::time::Duration::from_secs(60)) {
                eprintln!("could not sign in: {:?}", s.error);
                return;
            }
            s
        }
        None => Session::demo(),
    };
    let mut app = ChatApp::new(session);
    app.session.focused = true;
    app.set_dark(dark);
    if typing_demo && let Some(c) = app.session.view.channel {
        let users: Vec<_> = ["ada", "ravi"].iter().filter_map(|h| app.session.state.user_by_handle(h)).map(|u| u.id).collect();
        app.session.state.apply_event(&chatcraft_engine::model::Event::Typing { channel: c, users });
    }
    if seen_demo
        && let Some(c) = app.session.view.channel
        && let Some(last) = app.session.state.latest(c)
    {
        for h in ["ada", "ravi"] {
            if let Some(u) = app.session.state.user_by_handle(h).map(|u| u.id) {
                app.session.state.apply_event(&chatcraft_engine::model::Event::Receipt { channel: c, user: u, upto: last });
            }
        }
    }
    if verify_demo && let Some(u) = app.session.state.user_by_handle("ada").map(|u| u.id) {
        let emoji = [("🔧", "Spanner"), ("☂️", "Umbrella"), ("🔑", "Key"), ("🦄", "Unicorn"), ("🐘", "Elephant"), ("⚽", "Ball"), ("🌵", "Cactus")];
        app.session.verifications.push(chatcraft_engine::proto::VerificationUpdate {
            flow: "demo".into(),
            user: u,
            incoming: false,
            stage: chatcraft_engine::proto::VerifyStage::Emoji { emoji: emoji.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() },
        });
    }
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(w, h))
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut ChatApp| app.ui(ui), app);
    harness.run_steps(4);
    for c in cmds {
        let (id, params) = c.split_once('=').unwrap_or((c.as_str(), "{}"));
        let params: serde_json::Value = serde_json::from_str(params).unwrap_or_default();
        if let Err(e) = harness.state_mut().run(id, &params) {
            eprintln!("{id}: {e}");
        }
        harness.run_steps(3);
    }
    harness.run_steps(6);
    // A live account answers over the network: give it a moment.
    if matrix.is_some() {
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            harness.run_steps(1);
        }
    }
    match harness.render() {
        Ok(img) => match img.save(&out) {
            Ok(()) => println!("{out}"),
            Err(e) => eprintln!("save {out}: {e}"),
        },
        Err(e) => eprintln!("render: {e}"),
    }
}

/// `--sign-in`: the sign-in screen (with a sample error when `--dark`, to show both states).
/// `--sso`: waiting for single sign-on in the browser.
fn sign_in_shot(out: &str, dark: bool, sso: bool) {
    use chatcraft_ui_egui::{Credentials, Shell, SsoPoll, Start};
    use egui_kittest::kittest::Queryable;
    let creds = Credentials { homeserver: "matrix.example.org".into(), user: "morgan".into(), ..Credentials::default() };
    let mut shell = Shell::new(Start::SignIn(creds), Box::new(|_: &Credentials| Err("Matrix sign-in failed: invalid username or password".into())));
    shell.dark = dark;
    shell.sso = Some(Box::new(|_: &Credentials| Ok(Box::new(|| SsoPoll::Waiting) as Box<dyn FnMut() -> SsoPoll>)));
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, s: &mut Shell| s.ui(ui), shell);
    h.run_steps(4);
    if sso {
        h.get_by_label("Sign in with single sign-on").click();
        h.run_steps(4);
    }
    match h.render() {
        Ok(img) => match img.save(out) {
            Ok(()) => println!("{out}"),
            Err(e) => eprintln!("save {out}: {e}"),
        },
        Err(e) => eprintln!("render: {e}"),
    }
}
