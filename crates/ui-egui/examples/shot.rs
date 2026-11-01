//! Headless screenshot of the real app (no window, no focus stealing).
//!
//! `cargo run -p chatcraft-ui-egui --example shot -- out.png [--dark] [--size 1440x900]
//!  [--cmd 'thread.open={"message":12}'] ...`
//!
//! Commands run in order before the shot (engine ids or `ui.*` ids), so any state can be shown.

use chatcraft_engine::Session;
use chatcraft_ui_egui::ChatApp;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| "chatcraft.png".into());
    let mut app = ChatApp::new(Session::demo());
    app.session.focused = true;
    let (mut w, mut h) = (1440.0, 900.0);
    let mut cmds = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dark" => app.set_dark(true),
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
    match harness.render() {
        Ok(img) => match img.save(&out) {
            Ok(()) => println!("{out}"),
            Err(e) => eprintln!("save {out}: {e}"),
        },
        Err(e) => eprintln!("render: {e}"),
    }
}
