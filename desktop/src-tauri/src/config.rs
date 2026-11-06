use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    #[serde(default)]
    pub autostart: bool,
}

pub fn validate_url(s: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("URL vuoto".into());
    }
    let u = url::Url::parse(t).map_err(|e| format!("URL non valido: {e}"))?;
    if u.scheme() != "https" {
        return Err(format!("solo https (ricevuto '{}://')", u.scheme()));
    }
    if u.host_str().unwrap_or("").is_empty() {
        return Err("host mancante".into());
    }
    Ok(u.to_string())
}

pub fn load_from(dir: &Path) -> Option<Config> {
    let txt = std::fs::read_to_string(dir.join("config.json")).ok()?;
    let c: Config = serde_json::from_str(&txt).ok()?;
    validate_url(&c.server_url).ok()?;
    Some(c)
}

pub fn save_to(dir: &Path, c: &Config) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(c).expect("config serializzabile");
    std::fs::write(dir.join("config.json"), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_valida_https() {
        assert_eq!(
            validate_url("https://nome.ts.net").unwrap(),
            "https://nome.ts.net/"
        );
    }

    #[test]
    fn url_http_rifiutato() {
        assert!(validate_url("http://nome.ts.net").is_err());
    }

    #[test]
    fn url_fuoriserie_rifiutati() {
        for s in ["", "   ", "https://", "ftp://x.ts.net", "nome.ts.net", "//x"] {
            assert!(validate_url(s).is_err(), "avrebbe accettato {s:?}");
        }
    }

    #[test]
    fn config_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        assert!(load_from(d.path()).is_none());
        let c = Config {
            server_url: "https://nome.ts.net/".into(),
            autostart: false,
        };
        save_to(d.path(), &c).unwrap();
        assert_eq!(load_from(d.path()).unwrap().server_url, "https://nome.ts.net/");
    }

    #[test]
    fn config_corrotta_o_url_invalido_none() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("config.json"), "{{{{{").unwrap();
        assert!(load_from(d.path()).is_none());
        let c = Config {
            server_url: "http://nome.ts.net/".into(),
            autostart: false,
        };
        save_to(d.path(), &c).unwrap();
        assert!(load_from(d.path()).is_none());
    }
}
