use crate::asset_manager::Asset;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// A font file loaded from disk as raw bytes.
///
/// Pass the bytes to [`renderer::TextSystem::load_font`] to register the font
/// and get back a font ID for use in text rendering.
pub struct FontAsset {
    pub data: Vec<u8>,
}

impl Asset for FontAsset {
    type Params = PathBuf;

    fn load(path: Self::Params) -> Result<Self> {
        let data = std::fs::read(&path)
            .with_context(|| format!("failed to read font: {}", path.display()))?;
        Ok(Self { data })
    }
}
