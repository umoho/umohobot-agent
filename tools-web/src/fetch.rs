use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;

use crate::safe_client::SafeClient;

#[derive(Debug, thiserror::Error)]
pub enum WebFetchError {
    #[error("Web fetch error: {0}")]
    Client(#[from] crate::safe_client::SafeClientError),
    #[error("At least one URL is required")]
    NoUrls,
    #[error("Invalid selector: {0}")]
    InvalidSelector(String),
}

#[derive(Deserialize)]
pub struct WebFetchArgs {
    pub urls: Vec<String>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub ignore_robots: Option<bool>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

pub struct WebFetchTool;

impl Tool for WebFetchTool {
    const NAME: &'static str = "web_fetch";

    type Error = WebFetchError;
    type Args = WebFetchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "web_fetch".into(),
            description: "Fetch web pages with flexible output control. Supports CSS selector extraction, raw HTML, plain text, or Markdown. Parallel fetch for multiple URLs.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "urls": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "URLs to fetch (at least one, fetched in parallel)"
                    },
                    "selector": {
                        "type": "string",
                        "description": "Optional CSS selector. If set, only content from matching elements is returned. Works with all formats."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["markdown", "text", "html"],
                        "description": "Output format. Default: markdown. 'text' strips all HTML tags, 'html' returns raw HTML."
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
                "required": ["urls"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if args.urls.is_empty() {
            return Err(WebFetchError::NoUrls);
        }

        let timeout = args.timeout_secs.unwrap_or(30);
        let client = SafeClient::new(timeout);

        let format = args.format.as_deref().unwrap_or("markdown");
        let selector_str = args.selector.as_deref();

        let futures: Vec<_> = args
            .urls
            .iter()
            .map(|url| {
                fetch_one(
                    &client,
                    url,
                    selector_str,
                    format,
                    args.ignore_robots.unwrap_or(false),
                )
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        let mut output: Vec<String> = Vec::new();
        for result in results {
            match result {
                Ok(content) => output.push(content),
                Err(e) => output.push(format!("Error: {e}")),
            }
        }

        Ok(output.join("\n\n"))
    }
}

async fn fetch_one(
    client: &SafeClient,
    url: &str,
    selector: Option<&str>,
    format: &str,
    ignore_robots: bool,
) -> Result<String, WebFetchError> {
    let html = client.fetch(url, ignore_robots).await?;

    let document = scraper::Html::parse_document(&html);

    let elements: Vec<scraper::element_ref::ElementRef> = if let Some(sel_str) = selector {
        let sel = scraper::Selector::parse(sel_str)
            .map_err(|e| WebFetchError::InvalidSelector(e.to_string()))?;
        document.select(&sel).collect()
    } else {
        vec![document.root_element()]
    };

    let content = match format {
        "html" => elements
            .iter()
            .map(|e| e.html())
            .collect::<Vec<_>>()
            .join("\n\n"),
        "text" => elements
            .iter()
            .map(|e| e.text().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => {
            let mut parts = Vec::new();
            for elem in &elements {
                let elem_html = elem.inner_html();
                let md = htmd::HtmlToMarkdown::new()
                    .convert(&elem_html)
                    .unwrap_or_else(|_| elem.text().collect::<Vec<_>>().join(" "));
                parts.push(md);
            }
            parts.join("\n\n")
        }
    };

    Ok(format!("─── {url} ───\n\n{content}"))
}
