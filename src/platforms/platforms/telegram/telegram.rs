use crate::{app::ReplyPlan, config::Config};

use super::super::{PlatformKind, PlatformMessage, ReplyHandle};
use teloxide::types::Message;

#[derive(Clone, Debug)]
pub struct TelegramRuntimeConfig {
    pub bot_name: String,
    pub placeholder_text: String,
    pub message_edit_throttle_ms: u64,
}

impl TelegramRuntimeConfig {
    pub fn from_config(config: &Config) -> Self {
        Self {
            bot_name: config.bot_name.clone(),
            placeholder_text: config.placeholder_text.clone(),
            message_edit_throttle_ms: config.message_edit_throttle_ms,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TelegramRuntime {
    config: TelegramRuntimeConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelegramReplyStep {
    SendPlaceholder {
        room_id: String,
        text: String,
    },
    EditPlaceholder {
        room_id: String,
        text: String,
        throttle_ms: u64,
    },
}

#[derive(Clone, Debug)]
pub struct TelegramReplyScript {
    pub platform: PlatformKind,
    pub room_id: String,
    pub placeholder_text: String,
    pub final_text: String,
    pub edit_in_place: bool,
    pub edit_throttle_ms: u64,
    pub steps: Vec<TelegramReplyStep>,
}

#[derive(Clone, Debug)]
pub struct TelegramInboundMessage {
    pub room_id: String,
    pub sender_id: String,
    pub text: String,
    pub reply_to_message_id: Option<String>,
    pub is_mention: bool,
}

impl TelegramRuntime {
    pub fn new(config: TelegramRuntimeConfig) -> Self {
        Self { config }
    }

    pub fn from_config(config: &Config) -> Self {
        Self::new(TelegramRuntimeConfig::from_config(config))
    }

    pub fn describe(&self) -> String {
        format!(
            "telegram(bot={}, edit_throttle_ms={})",
            self.config.bot_name, self.config.message_edit_throttle_ms
        )
    }

    pub fn supports_placeholder_edit_flow(&self) -> bool {
        true
    }

    pub fn placeholder_text(&self) -> &str {
        &self.config.placeholder_text
    }

    pub fn inbound_from_message(&self, message: &Message) -> TelegramInboundMessage {
        let text = message
            .text()
            .or_else(|| message.caption())
            .unwrap_or("")
            .to_string();
        let sender_id = message
            .from
            .as_ref()
            .map(|user| user.id.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let reply_to_message_id = message.reply_to_message().map(|reply| reply.id.to_string());
        let is_mention = text.contains(&format!("@{}", self.config.bot_name));

        TelegramInboundMessage {
            room_id: message.chat.id.to_string(),
            sender_id,
            text,
            reply_to_message_id,
            is_mention,
        }
    }

    pub fn normalize_inbound(&self, inbound: TelegramInboundMessage) -> PlatformMessage {
        PlatformMessage {
            platform: PlatformKind::Telegram,
            room_id: inbound.room_id,
            sender_id: inbound.sender_id,
            text: inbound.text,
            attachments: Vec::new(),
            reply_to_message_id: inbound.reply_to_message_id,
            is_mention: inbound.is_mention,
        }
    }

    pub fn build_reply_script(&self, plan: &ReplyPlan) -> TelegramReplyScript {
        let placeholder_text = if plan.placeholder_text.is_empty() {
            self.config.placeholder_text.clone()
        } else {
            plan.placeholder_text.clone()
        };

        TelegramReplyScript {
            platform: plan.platform.clone(),
            room_id: plan.room_id.clone(),
            placeholder_text: placeholder_text.clone(),
            final_text: plan.final_text.clone(),
            edit_in_place: true,
            edit_throttle_ms: self.config.message_edit_throttle_ms,
            steps: vec![
                TelegramReplyStep::SendPlaceholder {
                    room_id: plan.room_id.clone(),
                    text: placeholder_text,
                },
                TelegramReplyStep::EditPlaceholder {
                    room_id: plan.room_id.clone(),
                    text: plan.final_text.clone(),
                    throttle_ms: self.config.message_edit_throttle_ms,
                },
            ],
        }
    }

    pub fn reply_handle(&self, room_id: String, message_id: String) -> ReplyHandle {
        ReplyHandle {
            platform: PlatformKind::Telegram,
            room_id,
            message_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_reply_script_uses_placeholder_then_edit_flow() {
        let runtime = TelegramRuntime::new(TelegramRuntimeConfig {
            bot_name: "bot".to_string(),
            placeholder_text: "正在处理...".to_string(),
            message_edit_throttle_ms: 750,
        });
        let script = runtime.build_reply_script(&ReplyPlan {
            platform: PlatformKind::Telegram,
            room_id: "room".to_string(),
            placeholder_text: String::new(),
            final_text: "done".to_string(),
            notes: Vec::new(),
        });

        assert!(script.edit_in_place);
        assert_eq!(script.steps.len(), 2);
        assert!(matches!(
            script.steps[0],
            TelegramReplyStep::SendPlaceholder { ref text, .. } if text == "正在处理..."
        ));
        assert!(matches!(
            script.steps[1],
            TelegramReplyStep::EditPlaceholder { ref text, throttle_ms, .. }
                if text == "done" && throttle_ms == 750
        ));
    }
}
