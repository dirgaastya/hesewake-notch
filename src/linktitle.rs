//! Judul halaman untuk tautan di Scrap. Diambil sekali per alamat (hanya https), di thread sendiri, dan disimpan di
//! memori selama aplikasi berjalan. Yang dikirim hanya satu permintaan GET ke alamat yang ditempel pengguna sendiri;
//! bisa dimatikan di Settings.

use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum LinkState {
    Loading,
    Title(String),
    Failed,
}

static CACHE: OnceLock<Mutex<HashMap<String, (LinkState, Instant)>>> = OnceLock::new();
static VERSION: AtomicU32 = AtomicU32::new(0);
const MAX_BYTES: usize = 96 * 1024;
const MAX_ENTRIES: usize = 200;

fn cache() -> &'static Mutex<HashMap<String, (LinkState, Instant)>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Naik tiap kali ada hasil baru (untuk memicu gambar ulang).
pub(crate) fn version() -> u32 {
    VERSION.load(Ordering::Relaxed)
}

pub(crate) fn state(url: &str) -> Option<LinkState> {
    cache().lock().ok()?.get(url).map(|e| e.0.clone())
}

/// Detik sejak status alamat ini terakhir berubah (untuk animasi chip).
pub(crate) fn age(url: &str) -> Option<f32> {
    cache().lock().ok()?.get(url).map(|e| e.1.elapsed().as_secs_f32())
}

/// Mulai mengambil judul `url` bila belum pernah (hanya https).
pub(crate) fn request(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    {
        let Ok(mut c) = cache().lock() else { return };
        if c.contains_key(url) || c.len() >= MAX_ENTRIES {
            return;
        }
        c.insert(url.to_string(), (LinkState::Loading, Instant::now()));
    }
    let url = url.to_string();
    std::thread::spawn(move || {
        let st = match weather::http_get(&url, MAX_BYTES).and_then(|h| extract_title(&h)) {
            Some(t) => LinkState::Title(t),
            None => LinkState::Failed,
        };
        if let Ok(mut c) = cache().lock() {
            c.insert(url, (st, Instant::now()));
        }
        VERSION.fetch_add(1, Ordering::Relaxed);
    });
}

/// Alamat http/https di dalam teks, tanpa tanda baca di ujungnya.
pub(crate) fn urls_in(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .map(|w| w.trim_end_matches(|c: char| ".,;:!?)]}'\"".contains(c)).to_string())
        .filter(|w| w.len() > 8)
        .collect()
}

/// "https://www.example.org/a/b" -> "example.org".
pub(crate) fn domain(url: &str) -> String {
    let s = url.split_once("://").map_or(url, |(_, r)| r);
    let s = s.strip_prefix("www.").unwrap_or(s);
    s.split(['/', '?', '#']).next().unwrap_or(s).to_string()
}

/// Isi `<title>` dari HTML, dibersihkan; None bila kosong atau judul tidak berguna (halaman login, "Just a moment").
pub(crate) fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open = lower[start..].find('>')? + start + 1;
    let end = lower[open..].find("</title")? + open;
    clean_title(&html[open..end])
}

fn clean_title(raw: &str) -> Option<String> {
    let s = raw.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return None;
    }
    let low = s.to_lowercase();
    const USELESS: [&str; 8] = ["just a moment", "attention required", "log in", "login", "sign in", "access denied", "403", "404"];
    if USELESS.iter().any(|p| low.starts_with(p)) {
        return None;
    }
    // halaman penolakan browser / bot ("browser tidak didukung", "enable javascript", dll) bukan judul halaman yang berguna
    const BLOCKED: [&str; 10] = ["browser", "javascript", "enable cookies", "captcha", "robot", "unusual traffic", "not supported", "tidak didukung", "tidak di", "are you human"];
    if BLOCKED.iter().any(|p| low.contains(p)) {
        return None;
    }
    Some(if s.chars().count() > 90 { format!("{}\u{2026}", s.chars().take(89).collect::<String>()) } else { s })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_extracted_and_cleaned() {
        assert_eq!(extract_title("<html><head><TITLE>  Hello &amp;\n World </TITLE></head>").as_deref(), Some("Hello & World"));
        assert_eq!(extract_title("<title lang=en>Docs</title>").as_deref(), Some("Docs"));
        assert_eq!(extract_title("<p>no title</p>"), None);
        assert_eq!(extract_title("<title>  </title>"), None);
    }

    #[test]
    fn useless_titles_are_dropped() {
        assert_eq!(extract_title("<title>Just a moment...</title>"), None);
        assert_eq!(extract_title("<title>Sign in to continue</title>"), None);
        assert!(extract_title(&format!("<title>{}</title>", "x".repeat(200))).unwrap().chars().count() <= 90);
    }

    #[test]
    fn urls_are_found_without_trailing_punctuation() {
        assert_eq!(urls_in("see https://example.org/a, and http://b.co/x). ok"), vec!["https://example.org/a", "http://b.co/x"]);
        assert!(urls_in("nothing here").is_empty());
        assert_eq!(domain("https://www.github.com/dirgaastya/x?y=1"), "github.com");
        assert_eq!(domain("http://localhost:8080/a"), "localhost:8080");
    }
}
