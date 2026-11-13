//! Block Stack 웹 클라이언트 (Leptos CSR → WASM).

mod app;
mod arcade;
mod runtime;
mod render;
mod settings;
mod sound;

fn main() {
    leptos::mount::mount_to_body(app::App);
}
