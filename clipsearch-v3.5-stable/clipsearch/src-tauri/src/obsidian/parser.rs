// ─────────────────────────────────────────────────────────────────────────────
// Markdown parser — fast, line-oriented, no AST.
//
// Extracts:
//   title     — first H1, or filename (without .md) if no H1
//   body      — full text (frontmatter stripped) for FTS5 indexing
//   headings  — all H1/H2/H3 heading texts
//   aliases   — from YAML frontmatter `aliases:` field (flow-style only)
//   tags      — from YAML frontmatter `tags:` field + inline #tags in body
//
// We do NOT use pulldown-cmark or similar — that would give us an AST we
// don't need. A single linear scan is ~10x faster and handles the metadata
// we actually care about.
// ─────────────────────────────────────────────────────────────────────────────

use once_cell::sync::Lazy;
use regex::Regex;

static TAG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|\s)#([a-zA-Z][\w\-/]*)").unwrap());

pub struct Parsed {
    pub title: String,
    pub body: String,
    pub headings: Vec<String>,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
}

/// Parse a Markdown document. `rel_path` is used as a fallback for the title.
pub fn parse(text: &str, rel_path: &str) -> Parsed {
    let (frontmatter, body) = split_frontmatter(text);

    // ── Aliases + tags from frontmatter ──────────────────────────────────────
    let mut aliases = Vec::new();
    let mut tags = Vec::new();
    if let Some(fm) = frontmatter {
        for line in fm.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("aliases:") {
                aliases.extend(parse_yaml_list(rest));
            } else if let Some(rest) = trimmed.strip_prefix("tags:") {
                tags.extend(parse_yaml_list(rest));
            }
        }
    }

    // ── Headings (H1–H3) + title (first H1) ──────────────────────────────────
    let mut headings = Vec::new();
    let mut title: Option<String> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        let h = if let Some(after) = trimmed.strip_prefix("# ") {
            Some(("h1", after.trim().to_string()))
        } else if let Some(after) = trimmed.strip_prefix("## ") {
            Some(("h2", after.trim().to_string()))
        } else if let Some(after) = trimmed.strip_prefix("### ") {
            Some(("h3", after.trim().to_string()))
        } else {
            None
        };
        if let Some((level, text)) = h {
            if level == "h1" && title.is_none() {
                title = Some(text.clone());
            }
            headings.push(text);
        }
    }

    // ── Inline #tags in body (e.g. `#project/foo`) ───────────────────────────
    for cap in TAG_RE.captures_iter(&body) {
        if let Some(t) = cap.get(1) {
            tags.push(t.as_str().to_string());
        }
    }
    tags.sort();
    tags.dedup();

    let title = title.unwrap_or_else(|| {
        let p = std::path::Path::new(rel_path);
        p.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(rel_path)
            .to_string()
    });

    Parsed {
        title,
        body: body.to_string(),
        headings,
        aliases,
        tags,
    }
}

/// Parse an Obsidian Canvas file (`.canvas`).
///
/// Canvas files are JSON with this shape:
/// ```json
/// {
///   "nodes": [
///     {"id":"...", "type":"text", "text":"# Heading\nsome text", "width":..., "height":...},
///     {"id":"...", "type":"text", "text":"more text"},
///     {"id":"...", "type":"file", "file":"path/to/note.md"},
///     ...
///   ],
///   "edges": [...]
/// }
/// ```
///
/// We use `serde_json` to parse the JSON properly (handles UTF-8, escapes,
/// nested structures). Extract every `text` field from nodes (concatenated
/// as the body), use the filename as the title, and pull out any `#tags`.
pub fn parse_canvas(text: &str, rel_path: &str) -> Parsed {
    let mut body_parts: Vec<String> = Vec::new();
    let mut headings: Vec<String> = Vec::new();

    // Parse as JSON. If parsing fails (malformed file), fall back to empty.
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text) {
        // Walk the JSON tree and collect every "text" field value.
        collect_text_values(&json, &mut body_parts);

        for value in &body_parts {
            if let Some(first_line) = value.lines().next() {
                let trimmed = first_line.trim();
                if let Some(h) = trimmed.strip_prefix("# ") {
                    headings.push(h.trim().to_string());
                }
            }
        }
    } else {
        tracing::warn!(rel_path, "failed to parse canvas file as JSON");
    }

    let body = body_parts.join("\n\n---\n\n");

    // Extract tags from the body (same regex as the Markdown parser).
    let mut tags: Vec<String> = Vec::new();
    for cap in TAG_RE.captures_iter(&body) {
        if let Some(t) = cap.get(1) {
            tags.push(t.as_str().to_string());
        }
    }
    tags.sort();
    tags.dedup();

    // Title: filename without `.canvas` extension.
    let title = std::path::Path::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel_path)
        .to_string();

    Parsed {
        title,
        body,
        headings,
        aliases: Vec::new(),
        tags,
    }
}

/// Recursively walk a JSON value and collect every `"text"` field's string
/// value into `out`.
fn collect_text_values(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if key == "text" {
                    if let Some(s) = val.as_str() {
                        out.push(s.to_string());
                    }
                }
                // Recurse into all values (including "text" itself if it's
                // an object/array — unusual but safe).
                collect_text_values(val, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_text_values(item, out);
            }
        }
        _ => {}
    }
}

/// Split off YAML frontmatter (`---\n...\n---`) from the body.
/// Returns `(Some(frontmatter), body)` or `(None, full_text)`.
fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    // The opening fence must be `---` on its own line.
    let after_open = if let Some(rest) = text.strip_prefix("---\r\n") {
        rest
    } else if let Some(rest) = text.strip_prefix("---\n") {
        rest
    } else {
        return (None, text);
    };

    // Find the next `---` that is at the start of a line. We scan for `\n---`
    // and verify what's after.
    let bytes = after_open.as_bytes();
    let mut search_from = 0usize;
    while search_from < bytes.len() {
        let slice = &after_open[search_from..];
        let Some(rel_pos) = slice.find("---") else {
            break;
        };
        let abs_pos = search_from + rel_pos;

        // Must be at the start of a line.
        let at_line_start = abs_pos == 0 || bytes.get(abs_pos - 1) == Some(&b'\n');
        if at_line_start {
            let after = &after_open[abs_pos + 3..];
            let ends_correctly = after.is_empty()
                || after.starts_with('\n')
                || after.starts_with("\r\n");
            if ends_correctly {
                let fm = &after_open[..abs_pos];
                let skip = if after.starts_with("\r\n") {
                    3 + 2
                } else if after.starts_with('\n') {
                    3 + 1
                } else {
                    3
                };
                let body = &after_open[abs_pos + skip..];
                return (Some(fm), body);
            }
        }
        search_from = abs_pos + 3;
    }

    // No closing fence — treat the whole thing as body (malformed frontmatter).
    (None, text)
}

/// Parse a YAML list value. Supports flow style (`[a, b, c]`) and a single
/// scalar value. Block-style lists are not parsed here — the frontmatter
/// scanner only reads the value on the same line as the key.
fn parse_yaml_list(value: &str) -> Vec<String> {
    let v = value.trim();
    if v.is_empty() {
        return Vec::new();
    }
    if let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return inner
            .split(',')
            .map(|s| unquote(s.trim()).to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    vec![unquote(v).to_string()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_h1_title_and_headings() {
        let md = "# Hello\nsome text\n## Sub\nmore\n### Deep\n";
        let p = parse(md, "note.md");
        assert_eq!(p.title, "Hello");
        assert_eq!(p.headings, vec!["Hello", "Sub", "Deep"]);
    }

    #[test]
    fn falls_back_to_filename_when_no_h1() {
        let p = parse("just body", "dir/My Note.md");
        assert_eq!(p.title, "My Note");
    }

    #[test]
    fn extracts_frontmatter_aliases_and_tags_flow_style() {
        let md = "---\naliases: [a, b, \"c\"]\ntags: [work, urgent]\n---\n# Title\nbody";
        let p = parse(md, "n.md");
        assert_eq!(p.aliases, vec!["a", "b", "c"]);
        assert_eq!(p.tags, vec!["urgent", "work"]); // sorted + deduped
    }

    #[test]
    fn extracts_inline_tags_from_body() {
        let md = "# T\nhello #project/foo and #bar world";
        let p = parse(md, "n.md");
        assert!(p.tags.contains(&"bar".to_string()));
        assert!(p.tags.contains(&"project/foo".to_string()));
    }

    #[test]
    fn handles_no_frontmatter() {
        let md = "# Title\nbody";
        let p = parse(md, "n.md");
        assert_eq!(p.title, "Title");
        assert!(p.aliases.is_empty());
    }

    #[test]
    fn handles_crlf_line_endings() {
        let md = "# Title\r\nbody\r\n";
        let p = parse(md, "n.md");
        assert_eq!(p.title, "Title");
    }

    #[test]
    fn canvas_parses_text_nodes() {
        // Using r##"..."## because the JSON contains `"#` which would close
        // a single-hash raw string.
        let json = r##"{"nodes":[{"id":"a","type":"text","text":"# My Canvas\nhello world"},{"id":"b","type":"text","text":"second node with #tag1"}],"edges":[]}"##;
        let p = parse_canvas(json, "My Canvas.canvas");
        assert_eq!(p.title, "My Canvas");
        assert!(p.body.contains("hello world"));
        assert!(p.body.contains("second node"));
        assert!(p.headings.contains(&"My Canvas".to_string()));
        assert!(p.tags.contains(&"tag1".to_string()));
    }

    #[test]
    fn canvas_handles_escaped_quotes() {
        let json = r##"{"nodes":[{"type":"text","text":"He said \"hi\""}]}"##;
        let p = parse_canvas(json, "test.canvas");
        assert!(p.body.contains("He said \"hi\""));
    }

    #[test]
    fn canvas_empty_file() {
        let p = parse_canvas("{}", "Empty.canvas");
        assert_eq!(p.title, "Empty");
        assert!(p.body.is_empty());
    }
}
