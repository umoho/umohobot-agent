use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum WebFetchError {
    #[error("Web fetch error: {0}")]
    Fetch(#[from] web2llm::Web2llmError),
}

#[derive(Deserialize)]
pub struct WebFetchArgs {
    pub url: String,
    #[serde(default)]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub ignore_robots: Option<bool>,
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
            description: "Fetch a web page and return its text content as clean Markdown. Optionally limit to a token budget.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to fetch"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "Optional maximum number of tokens to return. If the page exceeds this, only the first chunks that fit within the budget are returned along with a note about omitted content."
                    },
                    "ignore_robots": {
                        "type": "boolean",
                        "description": "Optional. Set to true to bypass robots.txt restrictions. Defaults to false (robots.txt is respected)."
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let robots_check = !args.ignore_robots.unwrap_or(false);
        let config = web2llm::Web2llmConfig {
            robots_check,
            ..Default::default()
        };
        let fetcher = web2llm::Web2llm::new(config)?;
        let result = fetcher.fetch(&args.url).await?;

        match args.max_tokens {
            None => Ok(result.markdown()),
            Some(budget) => {
                let total = result.total_tokens();
                if total <= budget {
                    return Ok(result.markdown());
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
                    "\n\n---\n*Page truncated: returned ~{used} tokens across {}/{} chunks (budget: {budget}). {omitted} chunk(s) omitted (of {total} total tokens). Use a larger `max_tokens` or fetch a different section if needed.*",
                    included.len(),
                    result.chunks.len(),
                );

                Ok(format!("{}\n\n{}", result.title, included.join("\n\n")) + &summary)
            }
        }
    }
}
