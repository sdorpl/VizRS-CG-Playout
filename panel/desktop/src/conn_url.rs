//! Adres i token serwera w oknie połączenia – wspólne dla okien pakietu VizRS
//! (ten sam plik w VizRS CG Artist `src/conn_url.rs`, VizRS CG System `source/shell/src/conn_url.rs`,
//! VizRS CG Playout `panel/desktop/src/conn_url.rs`). Źródło: VizRS CG System; kopie odświeża `tools/sync-cg.sh`
//! (Artist) i `panel/scripts/sync-ui.sh` (Playout), `--check` pilnuje zgodności. Reszta `connect.rs` różni się
//! między oknami (lista połączeń, praca lokalna, sprawdzenie serwera) i nie jest synchronizowana.

/// Adres serwera: `http(s)://host[:port][/ścieżka]`, bez końcowego `/`. Sam host → `https://`.
pub fn normalize_url(s: &str) -> Result<String, String> {
    let s = s.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err("podaj adres serwera".into());
    }
    let s = if s.contains("://") {
        s.to_string()
    } else {
        format!("https://{s}")
    };
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .ok_or(format!(
            "adres musi zaczynać się od http:// albo https:// („{s}”)"
        ))?;
    let host = rest.split('/').next().unwrap_or("");
    let ok_char = |c: char| c.is_ascii_alphanumeric() || "-._:[]/~%".contains(c);
    if host.is_empty() || !rest.chars().all(ok_char) || s.len() > 300 {
        return Err(format!("niepoprawny adres serwera „{s}”"));
    }
    Ok(s)
}

/// Znaki tokenu takie jak w serwerze (`--token`).
pub fn valid_token(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 256
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            normalize_url(" https://cg.example.pl/ ").unwrap(),
            "https://cg.example.pl"
        );
        assert_eq!(
            normalize_url("cg.example.pl").unwrap(),
            "https://cg.example.pl"
        );
        assert_eq!(
            normalize_url("http://10.0.0.5:8088").unwrap(),
            "http://10.0.0.5:8088"
        );
        assert!(normalize_url("").is_err());
        assert!(normalize_url("ftp://x").is_err());
        assert!(normalize_url("https://a b").is_err());
        assert!(normalize_url("https://x/'><script>").is_err());
    }

    #[test]
    fn tokens() {
        assert!(valid_token("abc-DEF_1.2"));
        assert!(!valid_token(""));
        assert!(!valid_token("a&b"));
        assert!(!valid_token(&"x".repeat(257)));
    }
}
