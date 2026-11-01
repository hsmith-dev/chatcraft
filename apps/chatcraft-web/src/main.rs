//! ChatCraft in the browser: the same `ChatApp` as the desktop, through eframe's web runner.
//! M0 runs the demo workspace in-page; connecting to a server over the browser's WebSocket is a
//! tracked gap (`docs/gaps.md`). Build with `trunk build --release` from this directory.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web {
    use chatcraft_engine::Session;
    use chatcraft_ui_egui::ChatApp;
    use wasm_bindgen::JsCast;

    struct App(ChatApp);

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.0.ui(ui);
        }
    }

    pub fn start() {
        wasm_bindgen_futures::spawn_local(async {
            let Some(canvas) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("chatcraft_canvas"))
                .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                log::error!("no #chatcraft_canvas");
                return;
            };
            // getTimezoneOffset is minutes *behind* UTC.
            let offset = -(js_sys::Date::new_0().get_timezone_offset() as i32);
            let r = eframe::WebRunner::new()
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(move |_cc| {
                        let mut app = ChatApp::new(Session::demo());
                        app.utc_offset_minutes = offset;
                        Ok(Box::new(App(app)))
                    }),
                )
                .await;
            if let Err(e) = r {
                log::error!("ChatCraft failed to start: {e:?}");
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("chatcraft-web only runs in the browser: build it with `trunk build --release` in apps/chatcraft-web");
}
