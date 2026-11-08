use std::env;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub port: u16,
    pub chunk_size: u64,
    // esquema+host dell'istanza (es. "https://box.ts.net"): fonte unica della
    // difesa CSRF; vuoto → fallback su Host (mai X-Forwarded-Host, spoofabile)
    pub origin: Option<String>,
}

pub fn load() -> Config {
    Config {
        data_dir: env::var("VAULTI_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| "/srv/vaulti".into()),
        port: env::var("VAULTI_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8420),
        chunk_size: env::var("VAULTI_CHUNK_SIZE")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8 * 1024 * 1024),
        origin: env::var("VAULTI_ORIGIN").ok().filter(|s| !s.is_empty()),
    }
}
