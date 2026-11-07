pub mod config;
pub mod nav;

use std::sync::Mutex;
use std::time::Duration;
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow};
use tauri_plugin_opener::OpenerExt;
use url::Url;

pub async fn healthz_ok(base: &str) -> bool {
    let built = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build();
    let cli = match built {
        Ok(c) => c,
        Err(_) => return false,
    };
    let url = format!("{}/healthz", base.trim_end_matches('/'));
    matches!(cli.get(url).send().await, Ok(r) if r.status().is_success())
}

fn navigate(w: &WebviewWindow, target: &str) -> Result<(), String> {
    let u = Url::parse(target).map_err(|e| e.to_string())?;
    w.navigate(u).map_err(|e| e.to_string())
}

fn window_of(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "finestra assente".into())
}

fn current_server(app: &AppHandle) -> String {
    app.state::<Mutex<String>>()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

fn set_server(app: &AppHandle, s: &str) {
    if let Ok(mut g) = app.state::<Mutex<String>>().lock() {
        *g = s.to_string();
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::<String>::new(String::new()))
        .invoke_handler(tauri::generate_handler![
            save_and_connect,
            retry_connect,
            open_config,
        ])
        .setup(|app| {
            let dir = app.path().app_config_dir().expect("config dir");
            let cfg = config::load_from(&dir);
            let start: WebviewUrl = match &cfg {
                Some(c) if tauri::async_runtime::block_on(healthz_ok(&c.server_url)) => {
                    set_server(app.handle(), &c.server_url);
                    let u = Url::parse(&c.server_url).expect("url di config valida");
                    WebviewUrl::External(u)
                }
                Some(c) => {
                    set_server(app.handle(), &c.server_url);
                    WebviewUrl::App("error.html".into())
                }
                None => WebviewUrl::App("index.html".into()),
            };
            let apph = app.handle().clone();
            WebviewWindowBuilder::new(app, "main", start)
                .title("Vaulti")
                .disable_drag_drop_handler()
                .on_navigation(move |url| {
                    if nav::allow(url, &current_server(&apph)) {
                        return true;
                    }
                    let ext = url.to_string();
                    // E0507: la chiusura esterna è Fn, il clone serve a ogni chiamata
                    let apph = apph.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = apph.opener().open_url(ext, None::<&str>) {
                            eprintln!("open_url fallito: {e}");
                        }
                    });
                    false
                })
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("errore avvio Vaulti");
}

#[tauri::command]
fn save_and_connect(app: AppHandle, url: String) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    // rispetta l'autostart già salvato: la riscrittura cambia solo l'URL
    let autostart = config::load_from(&dir)
        .map(|p| p.autostart)
        .unwrap_or(false);
    let c = config::Config {
        server_url: config::validate_url(&url)?,
        autostart,
    };
    config::save_to(&dir, &c).map_err(|e| e.to_string())?;
    set_server(&app, &c.server_url);
    if !tauri::async_runtime::block_on(healthz_ok(&c.server_url)) {
        return Err("server irraggiungibile".into());
    }
    let w = window_of(&app)?;
    navigate(&w, &c.server_url)
}

#[tauri::command]
fn retry_connect(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let c = config::load_from(&dir).ok_or("config assente")?;
    if !tauri::async_runtime::block_on(healthz_ok(&c.server_url)) {
        return Err("server irraggiungibile".into());
    }
    set_server(&app, &c.server_url);
    let w = window_of(&app)?;
    navigate(&w, &c.server_url)
}

#[tauri::command]
fn open_config(app: AppHandle) -> Result<(), String> {
    let w = window_of(&app)?;
    navigate(&w, &nav::local_url("index.html"))
}
