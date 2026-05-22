use agent::{AssistantContent, Message, UserContent};

pub fn format_for_compact(messages: &[Message]) -> String {
    messages
        .iter()
        .filter_map(|msg| match msg {
            Message::User { content } => {
                let texts: Vec<String> = content
                    .iter()
                    .filter_map(|c| match c {
                        UserContent::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect();
                if texts.is_empty() {
                    None
                } else {
                    Some(format!("User: {}", texts.join(" ")))
                }
            }
            Message::Assistant { content, .. } => {
                let lines: Vec<String> = content
                    .iter()
                    .filter_map(|c| match c {
                        AssistantContent::ToolCall(tc) => match tc.function.name.as_str() {
                            "telegram_sendMessage" => tc
                                .function
                                .arguments
                                .get("text")
                                .and_then(|v| v.as_str())
                                .map(|text| format!("Assistant: {text}")),
                            "web_fetch" => tc
                                .function
                                .arguments
                                .get("url")
                                .and_then(|v| v.as_str())
                                .map(|url| format!("Assistant fetched {url}")),
                            name => Some(format!("Assistant called a tool {name}")),
                        },
                        _ => None,
                    })
                    .collect();
                if lines.is_empty() {
                    None
                } else {
                    Some(lines.join("\n"))
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
