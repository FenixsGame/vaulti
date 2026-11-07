// nessuna finestra console nei build release (guscio NSIS)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    vaulti_desktop::run();
}
