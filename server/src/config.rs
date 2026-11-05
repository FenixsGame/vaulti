use std::env;

#[derive(Clone)]
pub struct Config {
    pub data_dir: String,
    pub port: u16,
    pub chunk_size: u64,
}

pub fn load() -> Config {
    Config {
        data_dir: env::var("VAULTI_DATA_DIR").unwrap_or_else(|_| "/srv/vaulti".into()),
        port: env::var("VAULTI_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8420),
        chunk_size: env::var("VAULTI_CHUNK_SIZE")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8 * 1024 * 1024),
    }
}
