use agent::OneOrMany;
use agent::UserContent;
use teloxide::types::{DiceEmoji, Message as TgMessage};

#[derive(serde::Serialize)]
pub(crate) struct MessageMeta {
    pub chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<i64>,
    pub msg_id: i64,
}

pub(crate) fn ser_meta(meta: &MessageMeta) -> String {
    let json = serde_json::to_string(meta).unwrap_or_default();
    format!("\x1E{}\x1E ", json)
}

pub(crate) fn dice_emoji_str(e: &DiceEmoji) -> &'static str {
    match e {
        DiceEmoji::Dice => "\u{1f3b2}",
        DiceEmoji::Darts => "\u{1f3af}",
        DiceEmoji::Bowling => "\u{1f3b3}",
        DiceEmoji::Basketball => "\u{1f3c0}",
        DiceEmoji::Football => "\u{26bd}",
        DiceEmoji::SlotMachine => "\u{1f3b0}",
    }
}

pub(crate) fn build_user_content(
    msg: &TgMessage,
    raw_text: &str,
) -> Result<OneOrMany<UserContent>, Box<dyn std::error::Error + Send + Sync>> {
    let chat_id = msg.chat.id.0;
    let msg_id = msg.id.0 as i64;
    let media_group_id = msg.media_group_id().map(|g| g.0.clone());
    let reply_to = msg.reply_to_message().as_ref().map(|r| r.id.0 as i64);
    let (user_id, username, first_name) = match msg.from.as_ref() {
        Some(u) => {
            let username = u.username.clone().map(|n| format!("@{}", n));
            (Some(u.id.0), username, Some(u.first_name.clone()))
        }
        None => (None, None, None),
    };

    let base = MessageMeta {
        chat_id,
        user_id,
        username,
        first_name,
        r#type: String::new(),
        media_group_id,
        emoji: None,
        value: None,
        width: None,
        height: None,
        duration: None,
        performer: None,
        title: None,
        file_name: None,
        mime_type: None,
        file_id: None,
        reply_to,
        msg_id,
    };

    if let Some(sticker) = msg.sticker() {
        let mut meta = base;
        meta.r#type = "sticker".into();
        meta.file_id = Some(sticker.file.id.to_string());
        meta.emoji = sticker.emoji.clone();
        return Ok(OneOrMany::one(UserContent::text(ser_meta(&meta))));
    }

    if let Some(photos) = msg.photo() {
        if let Some(p) = photos.last() {
            let mut meta = base;
            meta.r#type = "photo".into();
            meta.file_id = Some(p.file.id.to_string());
            meta.width = Some(p.width as u32);
            meta.height = Some(p.height as u32);
            return Ok(OneOrMany::one(UserContent::text(ser_meta(&meta))));
        }
    }

    if let Some(video) = msg.video() {
        let mut meta = base;
        meta.r#type = "video".into();
        meta.file_id = Some(video.file.id.to_string());
        meta.width = Some(video.width);
        meta.height = Some(video.height);
        meta.duration = Some(video.duration.seconds());
        meta.file_name = video.file_name.clone();
        meta.mime_type = video.mime_type.as_ref().map(|m| m.to_string());
        return Ok(OneOrMany::one(UserContent::text(ser_meta(&meta))));
    }

    if let Some(audio) = msg.audio() {
        let mut meta = base;
        meta.r#type = "audio".into();
        meta.file_id = Some(audio.file.id.to_string());
        meta.duration = Some(audio.duration.seconds());
        meta.performer = audio.performer.clone();
        meta.title = audio.title.clone();
        meta.file_name = audio.file_name.clone();
        meta.mime_type = audio.mime_type.as_ref().map(|m| m.to_string());
        return Ok(OneOrMany::one(UserContent::text(ser_meta(&meta))));
    }

    if let Some(doc) = msg.document() {
        let mut meta = base;
        meta.r#type = "document".into();
        meta.file_id = Some(doc.file.id.to_string());
        meta.file_name = doc.file_name.clone();
        meta.mime_type = doc.mime_type.as_ref().map(|m| m.to_string());
        return Ok(OneOrMany::one(UserContent::text(ser_meta(&meta))));
    }

    if let Some(anim) = msg.animation() {
        let mut meta = base;
        meta.r#type = "animation".into();
        meta.file_id = Some(anim.file.id.to_string());
        meta.width = Some(anim.width);
        meta.height = Some(anim.height);
        meta.duration = Some(anim.duration.seconds());
        meta.file_name = anim.file_name.clone();
        meta.mime_type = anim.mime_type.as_ref().map(|m| m.to_string());
        return Ok(OneOrMany::one(UserContent::text(ser_meta(&meta))));
    }

    if let Some(voice) = msg.voice() {
        let mut meta = base;
        meta.r#type = "voice".into();
        meta.file_id = Some(voice.file.id.to_string());
        meta.duration = Some(voice.duration.seconds());
        meta.mime_type = voice.mime_type.as_ref().map(|m| m.to_string());
        return Ok(OneOrMany::one(UserContent::text(ser_meta(&meta))));
    }

    if let Some(dice) = msg.dice() {
        let mut meta = base;
        meta.r#type = "dice".into();
        meta.emoji = Some(dice_emoji_str(&dice.emoji).to_string());
        meta.value = Some(dice.value as i32);
        return Ok(OneOrMany::one(UserContent::text(ser_meta(&meta))));
    }

    if !raw_text.trim().is_empty() {
        let mut meta = base;
        meta.r#type = "text".into();
        return Ok(OneOrMany::one(UserContent::text(format!(
            "{}{}",
            ser_meta(&meta),
            raw_text
        ))));
    }

    let mut meta = base;
    meta.r#type = "unsupported".into();
    Ok(OneOrMany::one(UserContent::text(ser_meta(&meta))))
}
