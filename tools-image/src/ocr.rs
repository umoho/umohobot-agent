use data_buffer::DataBuffer;
use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("OCR backend error: {0}")]
    Backend(String),
    #[error("Base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("Image processing error: {0}")]
    Image(String),
    #[error("Buffer key not found: {0}")]
    BufferKeyNotFound(String),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum ImageOcrArgs {
    Base64 { image_base64: String },
    Buffered { buffer_key: String },
}

pub trait OcrBackend: Send + Sync {
    fn recognize_text(&self, image_bytes: &[u8]) -> Result<String, OcrError>;
}

pub struct ImageOcrTool {
    backend: Box<dyn OcrBackend>,
    buffer: DataBuffer,
}

impl ImageOcrTool {
    pub fn new(backend: Box<dyn OcrBackend>, buffer: DataBuffer) -> Self {
        Self { backend, buffer }
    }
}

impl Tool for ImageOcrTool {
    const NAME: &'static str = "image_ocr";

    type Error = OcrError;
    type Args = ImageOcrArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "image_ocr".into(),
            description: "Recognize text from an image. Provide the image as a base64 string directly, or reference a buffer key for data previously stored in the shared buffer.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "image_base64": {
                        "type": "string",
                        "description": "Base64-encoded image data"
                    },
                    "buffer_key": {
                        "type": "string",
                        "description": "Key referencing image data previously stored in the shared buffer, obtained from a previous tool call"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let bytes = match args {
            ImageOcrArgs::Base64 { image_base64 } => {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.decode(&image_base64)?
            }
            ImageOcrArgs::Buffered { buffer_key } => self
                .buffer
                .get(&buffer_key)
                .ok_or_else(|| OcrError::BufferKeyNotFound(buffer_key))?,
        };
        self.backend.recognize_text(&bytes)
    }
}

#[cfg(feature = "ocr-backend-ocrs")]
pub mod ocrs;
