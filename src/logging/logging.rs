use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

pub fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_level(true)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .compact()
        .try_init();
}

pub fn sanitize_for_log(input: &str) -> String {
    let mut output = input.to_string();

    output = redact_bearer_token(&output);
    output = redact_key_value(&output, "api_key");
    output = redact_key_value(&output, "api-key");
    output = redact_key_value(&output, "token");
    output = redact_key_value(&output, "secret");
    output = redact_key_value(&output, "password");

    output
}

fn redact_bearer_token(input: &str) -> String {
    redact_prefix_value(input, "Bearer ")
}

fn redact_key_value(input: &str, key: &str) -> String {
    let patterns = [format!("{key}="), format!("{key}:")];
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while cursor < input.len() {
        let next_match = patterns
            .iter()
            .filter_map(|pattern| {
                input[cursor..]
                    .find(pattern)
                    .map(|relative| (cursor + relative, pattern.as_str()))
            })
            .min_by_key(|(index, _)| *index);

        let Some((start, pattern)) = next_match else {
            output.push_str(&input[cursor..]);
            break;
        };

        let value_start = skip_leading_whitespace(input, start + pattern.len());
        output.push_str(&input[cursor..value_start]);
        let value_end = find_value_end(input, value_start);
        output.push_str("<redacted>");
        cursor = value_end;
    }

    output
}

fn redact_prefix_value(input: &str, prefix: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative) = input[cursor..].find(prefix) {
        let start = cursor + relative;
        let value_start = skip_leading_whitespace(input, start + prefix.len());
        output.push_str(&input[cursor..value_start]);

        let value_end = find_value_end(input, value_start);
        output.push_str("<redacted>");
        cursor = value_end;
    }

    output.push_str(&input[cursor..]);
    output
}

fn find_value_end(input: &str, start: usize) -> usize {
    input[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            if ch.is_whitespace() || matches!(ch, ',' | ';' | ')' | ']' | '}' | '&') {
                Some(start + offset)
            } else {
                None
            }
        })
        .unwrap_or(input.len())
}

fn skip_leading_whitespace(input: &str, start: usize) -> usize {
    input[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            if ch.is_whitespace() {
                None
            } else {
                Some(start + offset)
            }
        })
        .unwrap_or(input.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_for_log_redacts_bearer_tokens() {
        let sanitized = sanitize_for_log("Authorization: Bearer secret-token-value");
        assert_eq!(sanitized, "Authorization: Bearer <redacted>");
    }

    #[test]
    fn sanitize_for_log_redacts_key_values() {
        let sanitized = sanitize_for_log("api_key=sk-123 token: abc password=xyz");
        assert_eq!(
            sanitized,
            "api_key=<redacted> token: <redacted> password=<redacted>"
        );
    }
}
