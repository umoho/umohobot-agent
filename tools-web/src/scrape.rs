use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum WebScrapeError {
    #[error("Web scrape error: {0}")]
    Fetch(#[from] web2llm::Web2llmError),
    #[error("Timeout error: {0}")]
    Timeout(String),
}

#[derive(Deserialize)]
pub struct WebScrapeArgs {
    pub urls: Vec<String>,
    #[serde(default)]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub ignore_robots: Option<bool>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

pub struct WebScrapeTool;

impl Tool for WebScrapeTool {
    const NAME: &'static str = "web_scrape";

    type Error = WebScrapeError;
    type Args = WebScrapeArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "web_scrape".into(),
            description: "Fetch web pages and return clean, scored Markdown content. Uses automatic content extraction that removes navigation, ads, and footers. Supports parallel fetching of multiple URLs.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "urls": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "URLs to fetch (at least one, fetched in parallel)"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "Optional maximum number of tokens to return per page. If exceeded, only the first chunks within budget are returned."
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
        let mut results: Vec<String> = Vec::new();
        let timeout = std::time::Duration::from_secs(args.timeout_secs.unwrap_or(30));

        for url in &args.urls {
            let result = tokio::time::timeout(
                timeout,
                fetch_single(url, args.max_tokens, args.ignore_robots),
            )
            .await;

            match result {
                Ok(Ok(content)) => results.push(content),
                Ok(Err(e)) => results.push(format!("─── {url} ───\nError: {e}")),
                Err(_) => results.push(format!(
                    "─── {url} ───\n*Request timed out after {}s, content omitted*",
                    timeout.as_secs()
                )),
            }
        }

        Ok(results.join("\n\n"))
    }
}

async fn fetch_single(
    url: &str,
    max_tokens: Option<usize>,
    ignore_robots: Option<bool>,
) -> Result<String, WebScrapeError> {
    let robots_check = !ignore_robots.unwrap_or(false);
    let config = web2llm::Web2llmConfig {
        robots_check,
        fetch_mode: web2llm::FetchMode::Static,
        ..Default::default()
    };
    let fetcher = web2llm::Web2llm::new(config)?;
    let result = fetcher.fetch(url).await?;

    match max_tokens {
        None => Ok(format!("─── {url} ───\n\n{}", result.markdown())),
        Some(budget) => {
            let total = result.total_tokens();
            if total <= budget {
                return Ok(format!("─── {url} ───\n\n{}", result.markdown()));
            }

            let mut used: usize = 0;
            let mut included: Vec<&str> = Vec::new();
            for chunk in &result.chunks {
                if used + chunk.tokens > budget && !included.is_empty() {
                    break;
                }
                used += chunk.tokens;
                included.push(&chunk.content);
            }

            let omitted = result.chunks.len() - included.len();
            let summary = format!(
                "\n\n---\n*Page truncated: returned ~{used} tokens across {}/{} chunks (budget: {budget}). {omitted} chunk(s) omitted (of {total} total tokens).*",
                included.len(),
                result.chunks.len(),
            );

            Ok(format!(
                "─── {url} ───\n\n{}\n\n{}",
                result.title,
                included.join("\n\n")
            ) + &summary)
        }
    }
}
