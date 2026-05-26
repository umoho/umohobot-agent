use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;

use crate::safe_client::SafeClient;

#[derive(Debug, thiserror::Error)]
pub enum WebFindError {
    #[error("Web find error: {0}")]
    Client(#[from] crate::safe_client::SafeClientError),
    #[error("At least one URL is required")]
    NoUrls,
    #[error("No keywords provided")]
    NoKeywords,
}

#[derive(Deserialize)]
pub struct WebFindArgs {
    pub urls: Vec<String>,
    pub keywords: Vec<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub match_mode: Option<String>,
    #[serde(default)]
    pub ignore_robots: Option<bool>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

pub struct WebFindTool;

impl Tool for WebFindTool {
    const NAME: &'static str = "web_find";

    type Error = WebFindError;
    type Args = WebFindArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "web_find".into(),
            description: "Search web pages for keywords and return CSS selector paths to matching elements, with surrounding context. Helps locate where content lives in the DOM before extracting it with web_fetch.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "urls": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "URLs to search (at least one, fetched in parallel)"
                    },
                    "keywords": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Keywords to search for (required)"
                    },
                    "method": {
                        "type": "string",
                        "enum": ["GET", "POST"],
                        "description": "HTTP method. Default: GET."
                    },
                    "body": {
                        "type": "string",
                        "description": "Request body (for POST). Raw string; use content_type to specify format (JSON, form-encoded, etc.)."
                    },
                    "content_type": {
                        "type": "string",
                        "description": "Content-Type header for the body (e.g. application/json, application/x-www-form-urlencoded). Ignored when no body is set."
                    },
                    "match_mode": {
                        "type": "string",
                        "enum": ["exact", "case_insensitive", "regex"],
                        "description": "Matching mode. Default: case_insensitive."
                    },
                    "ignore_robots": {
                        "type": "boolean",
                        "description": "Set to true to bypass robots.txt restrictions. Defaults to false."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Per-URL timeout in seconds. Defaults to 30. Timed-out URLs are skipped."
                    }
                },
                "required": ["urls", "keywords"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if args.urls.is_empty() {
            return Err(WebFindError::NoUrls);
        }
        if args.keywords.is_empty() {
            return Err(WebFindError::NoKeywords);
        }

        let method = match args.method.as_deref().unwrap_or("GET") {
            "POST" => reqwest::Method::POST,
            _ => reqwest::Method::GET,
        };
        let timeout = args.timeout_secs.unwrap_or(30);
        let client = SafeClient::new(timeout);
        let mode = args.match_mode.as_deref().unwrap_or("case_insensitive");
        let body = args.body.clone();
        let content_type = args.content_type.as_deref();

        let futures: Vec<_> = args
            .urls
            .iter()
            .map(|url| {
                find_in_url(
                    &client,
                    url,
                    method.clone(),
                    body.clone(),
                    content_type,
                    &args.keywords,
                    mode,
                    args.ignore_robots.unwrap_or(false),
                )
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        let mut output: Vec<String> = Vec::new();
        for result in results {
            match result {
                Ok(Some(content)) => output.push(content),
                Ok(None) => {}
                Err(e) => output.push(format!("Error: {e}")),
            }
        }

        if output.is_empty() {
            Ok("未找到匹配内容".into())
        } else {
            Ok(output.join("\n\n"))
        }
    }
}

struct Match {
    keyword: String,
    full_path: String,
    short_path: String,
    context: String,
}

async fn find_in_url(
    client: &SafeClient,
    url: &str,
    method: reqwest::Method,
    body: Option<String>,
    content_type: Option<&str>,
    keywords: &[String],
    mode: &str,
    ignore_robots: bool,
) -> Result<Option<String>, WebFindError> {
    let html = client
        .request(url, method, body, content_type, ignore_robots)
        .await?;
    let document = scraper::Html::parse_document(&html);

    let matchers: Vec<(String, Matcher)> = keywords
        .iter()
        .map(|kw| {
            let m = match mode {
                "exact" => Matcher::Substring(kw.clone()),
                "regex" => regex::Regex::new(kw)
                    .map(|r| Matcher::Regex(r))
                    .unwrap_or_else(|_| Matcher::Substring(kw.clone())),
                _ => Matcher::CaseInsensitive(kw.to_lowercase()),
            };
            (kw.clone(), m)
        })
        .collect();

    let mut matches: Vec<Match> = Vec::new();

    for elem in document.root_element().descendent_elements() {
        let text: String = elem.text().collect();
        if text.trim().is_empty() {
            continue;
        }

        for (keyword, matcher) in &matchers {
            if !matcher.matches(&text) {
                continue;
            }

            let paths = crate::selector_path::build_paths(elem);
            let ctx = extract_context(&text, keyword, 200);

            matches.push(Match {
                keyword: keyword.clone(),
                full_path: paths.full,
                short_path: paths.short,
                context: ctx,
            });
        }
    }

    if matches.is_empty() {
        return Ok(None);
    }

    let mut current_kw = String::new();
    let mut output = format!("─── {url} ───");
    for m in &matches {
        if m.keyword != current_kw {
            let count = matches.iter().filter(|m2| m2.keyword == m.keyword).count();
            output += &format!("\n\nkeyword \"{}\" ({mode}) — {count} matches", m.keyword);
            current_kw = m.keyword.clone();
        }
        output += &format!("\n  full:   {}", m.full_path);
        output += &format!("\n  short:  {}", m.short_path);
        output += &format!("\n  context: {}", m.context);
    }

    Ok(Some(output))
}

enum Matcher {
    Substring(String),
    CaseInsensitive(String),
    Regex(regex::Regex),
}

impl Matcher {
    fn matches(&self, text: &str) -> bool {
        match self {
            Matcher::Substring(kw) => text.contains(kw.as_str()),
            Matcher::CaseInsensitive(lower) => text.to_lowercase().contains(lower.as_str()),
            Matcher::Regex(re) => re.is_match(text),
        }
    }
}

fn extract_context(text: &str, keyword: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let total_chars = chars.len();

    if let Some(byte_pos) = text.find(keyword) {
        let pos_in_chars = text[..byte_pos].chars().count();
        let kw_chars = keyword.chars().count();
        let half = max_chars / 2;

        let start_char = pos_in_chars.saturating_sub(half);
        let end_char = (pos_in_chars + kw_chars + half).min(total_chars);

        let prefix = if start_char > 0 { "..." } else { "" };
        let suffix = if end_char < total_chars { "..." } else { "" };
        let snippet: String = chars[start_char..end_char].iter().collect();

        format!("{prefix}{snippet}{suffix}")
    } else {
        let end = max_chars.min(total_chars);
        let snippet: String = chars[..end].iter().collect();
        let suffix = if end < total_chars { "..." } else { "" };
        format!("{snippet}{suffix}")
    }
}
