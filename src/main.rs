//! Desktop entry point. The game lives in the library crate so the same code
//! can be built as an Android `cdylib` (see `lib.rs`'s `#[bevy_main]`).

fn main() {
    bevy_uspsa::main();
}
