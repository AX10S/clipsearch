// Prevents additional console window on Windows in release, AND keeps stdout
// attached in debug builds for `println!`-based debugging.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    clipvault_lib::run();
}
