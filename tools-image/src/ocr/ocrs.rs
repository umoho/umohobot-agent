use std::path::{Path, PathBuf};

use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;

use super::{OcrBackend, OcrError};

fn cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".cache")
        });
    base.join("umohobot").join("ocrs")
}

pub struct OcrsBackend {
    engine: OcrEngine,
}

impl OcrsBackend {
    pub async fn new() -> Result<Self, OcrError> {
        let dir = cache_dir();
        std::fs::create_dir_all(&dir).map_err(|e| OcrError::Backend(e.to_string()))?;

        let detection_path = dir.join("text-detection.rten");
        let recognition_path = dir.join("text-recognition.rten");

        if !detection_path.exists() {
            download_model(
                "https://huggingface.co/robertknight/ocrs/resolve/main/text-detection-ssfbcj81.rten",
                &detection_path,
            )
            .await?;
        }
        if !recognition_path.exists() {
            download_model(
                "https://huggingface.co/robertknight/ocrs/resolve/main/text-rec-checkpoint-s52qdbqt.rten",
                &recognition_path,
            )
            .await?;
        }

        let detection_model =
            Model::load_file(&detection_path).map_err(|e| OcrError::Backend(e.to_string()))?;
        let recognition_model =
            Model::load_file(&recognition_path).map_err(|e| OcrError::Backend(e.to_string()))?;

        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .map_err(|e| OcrError::Backend(e.to_string()))?;

        Ok(Self { engine })
    }
}

impl OcrBackend for OcrsBackend {
    fn recognize_text(&self, image_bytes: &[u8]) -> Result<String, OcrError> {
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| OcrError::Image(e.to_string()))?
            .into_rgb8();
        let (width, height) = img.dimensions();
        let source = ImageSource::from_bytes(img.as_raw(), (width, height))
            .map_err(|e| OcrError::Image(e.to_string()))?;
        let input = self
            .engine
            .prepare_input(source)
            .map_err(|e| OcrError::Backend(e.to_string()))?;
        let text = self
            .engine
            .get_text(&input)
            .map_err(|e| OcrError::Backend(e.to_string()))?;
        Ok(text)
    }
}

async fn download_model(url: &str, path: &Path) -> Result<(), OcrError> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| OcrError::Backend(format!("Failed to download OCR model: {e}")))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| OcrError::Backend(format!("Failed to read OCR model: {e}")))?;
    std::fs::write(path, &bytes)
        .map_err(|e| OcrError::Backend(format!("Failed to write OCR model: {e}")))?;
    Ok(())
}
