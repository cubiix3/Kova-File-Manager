//! Opt-in end-to-end measurements; never used to drive application behavior.
use kova_core::domain::TabId;
use std::{cell::RefCell, collections::HashMap, time::Instant};
thread_local! {
    static REQUESTS: RefCell<HashMap<TabId, (Instant, &'static str)>> = RefCell::default();
    static FRAME: RefCell<Option<(Instant, &'static str)>> = const { RefCell::new(None) };
}
pub fn begin(tab: TabId, kind: &'static str) {
    if std::env::var_os("KOVA_PERF").is_some() {
        REQUESTS.with_borrow_mut(|requests| {
            requests.insert(tab, (Instant::now(), kind));
        });
    }
}
pub fn ready(tab: TabId) {
    if let Some(request) = REQUESTS.with_borrow_mut(|requests| requests.remove(&tab)) {
        tracing::info!(
            kind = request.1,
            elapsed_ms = request.0.elapsed().as_secs_f64() * 1000.,
            "request to UI model"
        );
        FRAME.with_borrow_mut(|frame| *frame = Some(request));
    }
}
pub fn rendered() {
    if let Some((start, kind)) = FRAME.with_borrow_mut(Option::take) {
        tracing::info!(
            kind,
            elapsed_ms = start.elapsed().as_secs_f64() * 1000.,
            "request to rendered result"
        );
    }
}
