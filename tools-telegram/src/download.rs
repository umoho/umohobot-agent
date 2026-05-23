use crate::ToolError;
use data_buffer::DataBuffer;
use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;
use telegram_host::TelegramHost;
use teloxide::net::Download;
use teloxide::prelude::Requester;
use teloxide::types::FileId;

#[derive(Deserialize)]
pub struct TelegramDownloadArgs {
    pub file_id: String,
}

pub struct TelegramDownloadTool {
    pub host: TelegramHost,
    pub buffer: DataBuffer,
}

impl Tool for TelegramDownloadTool {
    const NAME: &'static str = "telegram_download";

    type Error = ToolError;
    type Args = TelegramDownloadArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_download".into(),
            description: "Download a file from Telegram by file ID and store it in the shared buffer. Returns a buffer key that can be passed to other tools (e.g. image_ocr) for further processing.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file_id": {
                        "type": "string",
                        "description": "Telegram file ID to download"
                    }
                },
                "required": ["file_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let file = self.host.bot().get_file(FileId(args.file_id)).await?;
        let mut buf = Vec::new();
        self.host
            .bot()
            .download_file(&file.path, &mut buf)
            .await
            .map_err(|e| ToolError::Request(e.into()))?;
        let key = self.buffer.store(buf);
        Ok(format!(
            "File downloaded to buffer. Use buffer key \"{}\" with other tools like image_ocr.",
            key
        ))
    }
}
