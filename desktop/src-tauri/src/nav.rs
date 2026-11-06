use url::Url;

/// true = navigazione permessa nella webview.
/// Locale dell'app sempre; origin del server se configurato; altro → false.
pub fn allow(cur: &Url, server_url: &str) -> bool {
    if is_local(cur) {
        return true;
    }
    if server_url.is_empty() {
        return false;
    }
    match Url::parse(server_url) {
        Ok(s) => cur.origin() == s.origin(),
        Err(_) => false,
    }
}

/// origin locale della webview su Windows (WebView2) e alias tauri.
pub fn is_local(cur: &Url) -> bool {
    const LOCAL: [(&str, &str); 2] = [
        ("http", "tauri.localhost"),
        ("tauri", "localhost"),
    ];
    LOCAL
        .iter()
        .any(|(s, h)| cur.scheme() == *s && cur.host_str() == Some(*h))
}

/// URL di una pagina locale (frontendDist `../ui`).
pub fn local_url(page: &str) -> String {
    format!("http://tauri.localhost/{page}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn origin_server_passa() {
        let s = "https://nome.ts.net";
        for cur in [
            "https://nome.ts.net/",
            "https://nome.ts.net/app.js",
            "https://nome.ts.net/api/files?folder_id=",
            "https://nome.ts.net:443/api/me",
        ] {
            assert!(allow(&u(cur), s), "avrebbe bloccato {cur}");
        }
    }

    #[test]
    fn origin_simili_bloccati() {
        let s = "https://nome.ts.net";
        for cur in [
            "http://nome.ts.net/",
            "https://evil.example.com/",
            "https://nome.ts.net.attacker.com/",
            "https://attacker.com/?x=https://nome.ts.net",
        ] {
            assert!(!allow(&u(cur), s), "avrebbe permesso {cur}");
        }
    }

    #[test]
    fn pagine_locali_passano() {
        let s = "https://nome.ts.net";
        assert!(allow(&u("http://tauri.localhost/index.html"), s));
        assert!(allow(&u("tauri://localhost/error.html"), s));
    }

    #[test]
    fn senza_server_solo_locale() {
        assert!(!allow(&u("https://nome.ts.net/"), ""));
        assert!(allow(&u("http://tauri.localhost/index.html"), ""));
    }

    #[test]
    fn server_url_invalido_non_panica() {
        assert!(!allow(&u("https://nome.ts.net/"), ":::::"));
    }

    #[test]
    fn local_url_formato() {
        assert_eq!(
            local_url("error.html"),
            "http://tauri.localhost/error.html"
        );
    }
}
