//! The few runtime services the worker needs, natively (Tokio on a background thread) and in
//! the browser (futures on the page's event loop).

use std::future::Future;
use std::time::Duration;

/// Runs a background task.
#[cfg(not(target_family = "wasm"))]
pub(crate) fn spawn<F: Future<Output = ()> + Send + 'static>(f: F) {
    drop(tokio::spawn(f));
}

/// Runs a background task.
#[cfg(target_family = "wasm")]
pub(crate) fn spawn<F: Future<Output = ()> + 'static>(f: F) {
    wasm_bindgen_futures::spawn_local(f);
}

#[cfg(not(target_family = "wasm"))]
pub(crate) async fn sleep(d: Duration) {
    tokio::time::sleep(d).await;
}

#[cfg(target_family = "wasm")]
pub(crate) async fn sleep(d: Duration) {
    gloo_timers::future::sleep(d).await;
}
