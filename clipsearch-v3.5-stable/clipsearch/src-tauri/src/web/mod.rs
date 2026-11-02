// ─────────────────────────────────────────────────────────────────────────────
// Web module — generate web search suggestions + URLs.
//
// No API keys required. We use Google's suggest API (returns JSON) for
// autocomplete suggestions, plus a "search on <provider>" fallback row.
//
// Providers: google, bing, duckduckgo, yandex. The active provider is set in
// settings (`web_search_provider`, default "google").
// ─────────────────────────────────────────────────────────────────────────────

use serde::Serialize;

/// One web search result row.
#[derive(Debug, Clone, Serialize)]
pub struct WebResult {
    pub query: String,
    pub provider: String,
    pub url: String,
    pub display_text: String,
}

/// Build the search URL for the given provider + query.
pub fn build_search_url(provider: &str, query: &str) -> String {
    let q = url_encode(query);
    match provider.to_lowercase().as_str() {
        "bing" => format!("https://www.bing.com/search?q={}", q),
        "duckduckgo" => format!("https://duckduckgo.com/?q={}", q),
        "yandex" => format!("https://yandex.ru/search/?text={}", q),
        _ => format!("https://www.google.com/search?q={}", q),
    }
}

/// Build the ITSM (Naumen) search URL for a query.
///
/// Example: `Сопровождение рабочих мест` →
/// `https://itsm.infra.sbt/sd/operator/#search:%D0%A1%D0%BE%D0%BF%D1%80%D0%BE%D0%B2%D0%BE%D0%B6%D0%B4%D0%B5%D0%BD%D0%B8%D0%B5%20%D1%80%D0%B0%D0%B1%D0%BE%D1%87%D0%B8%D1%85%20%D0%BC%D0%B5%D1%81%D1%82:false`
///
/// Build a Naumen ITSM portal search URL.
/// The URL fragment is `#search:<url-encoded-query>:false`.
/// Note: the fragment uses `%20` for spaces (URL-encoded), and the `:false`
/// suffix is a literal part of the Naumen URL scheme (likely a UI flag).
pub fn build_itsm_url(query: &str) -> String {
    let q = url_encode(query);
    format!("https://itsm.infra.sbt/sd/operator/#search:{}:false", q)
}

/// Build a Sberworks Confluence search URL.
/// Example: https://sberworks.ru/wiki/dosearchsite.action?cql=siteSearch+~+%22QUERY%22&queryString=QUERY
pub fn build_confluence_url(query: &str) -> String {
    let q = url_encode(query);
    format!(
        "https://sberworks.ru/wiki/dosearchsite.action?cql=siteSearch+~+%22{}%22&queryString={}",
        q, q
    )
}

/// Build a Sberworks Jira search URL (full-text JQL search).
/// Example: https://sberworks.ru/jira/issues/?jql=text%20~%20%22QUERY%22
pub fn build_jira_url(query: &str) -> String {
    let q = url_encode(query);
    format!("https://sberworks.ru/jira/issues/?jql=text%20~%20%22{}%22", q)
}

/// Fetch suggestions from Google Suggest (client=firefox returns a JSON array).
/// Returns up to `limit` suggestions. On error, returns an empty vec — the
/// caller falls back to the single "Search on Google" row.
pub async fn fetch_suggestions(query: &str, limit: usize) -> Vec<String> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let url = format!(
        "https://suggestqueries.google.com/complete/search?client=firefox&q={}",
        url_encode(query)
    );

    // Use a short timeout — if the suggest API is slow/unavailable we don't
    // want to delay the search.
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let body = match client.get(&url).send().await {
        Ok(resp) => resp.text().await.unwrap_or_default(),
        Err(_) => return Vec::new(),
    };

    // Google Suggest (client=firefox) returns: ["query", ["sug1","sug2",...]]
    // We parse it as a generic JSON array.
    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = parsed.as_array().and_then(|a| a.get(1)).and_then(|v| v.as_array());
    let Some(arr) = arr else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .take(limit)
        .collect()
}

/// Build the full result set for a query: just a single "Search on <provider>"
/// row. AI mode was removed per user request.
pub async fn search(
    query: &str,
    provider: &str,
    _limit: usize,
) -> Vec<WebResult> {
    if query.trim().is_empty() {
        return Vec::new();
    }

    vec![WebResult {
        query: query.to_string(),
        provider: provider.to_string(),
        url: build_search_url(provider, query),
        display_text: format!("Search \"{}\" on {}", query, capitalize(provider)),
    }]
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Minimal URL-encoder. Keeps alphanumerics + a few safe chars, encodes the
/// rest as `%XX`.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_builder_google() {
        assert_eq!(
            build_search_url("google", "hello world"),
            "https://www.google.com/search?q=hello%20world"
        );
    }

    #[test]
    fn url_builder_yandex() {
        assert_eq!(
            build_search_url("yandex", "привет"),
            "https://yandex.ru/search/?text=%D0%BF%D1%80%D0%B8%D0%B2%D0%B5%D1%82"
        );
    }

    #[test]
    fn url_builder_default_is_google() {
        assert_eq!(
            build_search_url("unknown", "test"),
            "https://www.google.com/search?q=test"
        );
    }

    #[test]
    fn itsm_url_cyrillic() {
        // "Сопровождение рабочих мест" should be URL-encoded.
        let url = build_itsm_url("Сопровождение рабочих мест");
        assert!(url.starts_with("https://itsm.infra.sbt/sd/operator/#search:"));
        assert!(url.ends_with(":false"));
        // Should contain URL-encoded Cyrillic bytes (%D0%A1...).
        assert!(url.contains("%D0%A1"));
        assert!(url.contains("%20")); // spaces encoded
    }

    #[test]
    fn itsm_url_simple_ascii() {
        let url = build_itsm_url("hello");
        assert_eq!(
            url,
            "https://itsm.infra.sbt/sd/operator/#search:hello:false"
        );
    }

    #[test]
    fn empty_query_returns_empty() {
        // Use a runtime to test the async function.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt.block_on(search("", "google", 5));
        assert!(res.is_empty());
    }
}
