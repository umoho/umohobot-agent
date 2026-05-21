pub mod ocr;

#[cfg(not(feature = "ocr-backend-ocrs"))]
compile_error!("No OCR backend enabled. Enable one: \"ocr-backend-ocrs\"");
