use std::collections::HashMap;

use chrono::{FixedOffset, Utc};
use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;

use crate::Error;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeNowArgs {
    pub timezone: Option<String>,
    pub format: Option<String>,
}

pub struct TimeNowTool;

impl Tool for TimeNowTool {
    const NAME: &'static str = "time_now";

    type Error = Error;
    type Args = TimeNowArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Get the current server time. Supports timezone conversion and custom strftime format. \
                Use format=\"%s\" for unix timestamp. Default format: \"%A, %Y-%m-%d %H:%M:%S\" (with weekday). \
                Common formats: %Y(year), %m(month), %d(day), %H(24h), %I(12h), %M(minute), %S(second), %p(AM/PM), %z(timezone offset), %Z(timezone name).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "timezone": {
                        "type": "string",
                        "description": "Timezone name like \"Asia/Shanghai\", \"America/New_York\", \"UTC\". Default: UTC"
                    },
                    "format": {
                        "type": "string",
                        "description": "strftime format string. Use \"%s\" for unix timestamp. Default: \"%A, %Y-%m-%d %H:%M:%S\""
                    }
                }
            }),
        }
    }

    async fn call(&self, args: TimeNowArgs) -> Result<String, Error> {
        let format = args
            .format
            .unwrap_or_else(|| "%A, %Y-%m-%d %H:%M:%S".into());

        let dt = match &args.timezone {
            Some(tz) if tz.to_uppercase() != "UTC" => {
                let offset = parse_timezone(tz)?;
                Utc::now()
                    .with_timezone(&offset)
                    .format(&format)
                    .to_string()
            }
            _ => Utc::now().format(&format).to_string(),
        };

        Ok(dt)
    }
}

fn parse_timezone(tz: &str) -> Result<FixedOffset, Error> {
    let offsets: HashMap<&str, i32> = [
        ("UTC", 0),
        ("GMT", 0),
        ("EST", -5 * 3600),
        ("EDT", -4 * 3600),
        ("CST", -6 * 3600),
        ("CDT", -5 * 3600),
        ("MST", -7 * 3600),
        ("MDT", -6 * 3600),
        ("PST", -8 * 3600),
        ("PDT", -7 * 3600),
    ]
    .iter()
    .copied()
    .collect();

    if let Some(&secs) = offsets.get(tz.to_uppercase().as_str()) {
        return FixedOffset::east_opt(secs)
            .ok_or_else(|| Error::ParseError(format!("invalid timezone offset: {secs}")));
    }

    if let Some((sign, hour, minute)) = parse_utc_offset(tz) {
        let secs = hour * 3600 + minute * 60;
        let secs = if sign { secs } else { -secs };
        return FixedOffset::east_opt(secs)
            .ok_or_else(|| Error::ParseError(format!("invalid timezone offset: {tz}")));
    }

    Err(Error::ParseError(format!(
        "unknown timezone: '{tz}'. Try a UTC offset like 'UTC+8', 'UTC-5', 'UTC+05:30', \
        or an abbreviation: EST, PST, CST, UTC, GMT"
    )))
}

fn parse_utc_offset(s: &str) -> Option<(bool, i32, i32)> {
    let s = s.trim().to_uppercase();
    if !s.starts_with("UTC") && !s.starts_with("GMT") {
        return None;
    }
    let rest = &s[3..];
    if rest.is_empty() {
        return Some((true, 0, 0));
    }
    let (sign_str, rest) = if rest.starts_with('+') {
        ("+", &rest[1..])
    } else if rest.starts_with('-') {
        ("-", &rest[1..])
    } else {
        return None;
    };
    let sign = sign_str == "+";

    if let Some((h, m)) = rest.split_once(':') {
        let hour: i32 = h.parse().ok()?;
        let minute: i32 = m.parse().ok()?;
        Some((sign, hour, minute))
    } else if rest.len() >= 4 {
        let hour: i32 = rest[..2].parse().ok()?;
        let minute: i32 = rest[2..4].parse().ok()?;
        Some((sign, hour, minute))
    } else {
        let hour: i32 = rest.parse().ok()?;
        Some((sign, hour, 0))
    }
}
