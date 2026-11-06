pub mod config;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("errore avvio Vaulti");
}
