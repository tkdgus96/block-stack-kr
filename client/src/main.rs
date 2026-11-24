//! Block Stack 웹 클라이언트 (Leptos CSR → WASM).

mod api;
mod app;
mod fullreplay;
mod atmosphere;
mod coop;
mod practice;
mod profile;
mod replay;
mod arcade;
mod battle;
mod combat;
mod records;
mod room_controls;
mod mobile_settings;
mod runtime;
mod render;
mod settings;
mod sound;
mod touch;

fn main() {
    leptos::mount::mount_to_body(app::App);
}
