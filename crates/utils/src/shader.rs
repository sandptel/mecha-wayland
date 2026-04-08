use crate::asset_manager::Asset;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Where GLSL source comes from — a literal string or a file on disk.
pub enum ShaderSource {
    Inline(String),
    File(PathBuf),
}

impl ShaderSource {
    /// Use a GLSL string directly (e.g. a `const` or `include_str!` result).
    pub fn inline(src: impl Into<String>) -> Self {
        Self::Inline(src.into())
    }

    /// Read GLSL from a file at runtime.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    fn resolve(self) -> Result<String> {
        match self {
            Self::Inline(s) => Ok(s),
            Self::File(p) => std::fs::read_to_string(&p)
                .with_context(|| format!("failed to read shader: {}", p.display())),
        }
    }
}

/// Parameters for loading a [`ShaderAsset`] — one source for the vertex shader
/// and one for the fragment shader.
pub struct ShaderConfig {
    pub vert: ShaderSource,
    pub frag: ShaderSource,
}

impl ShaderConfig {
    /// Load both shaders from files on disk.
    pub fn from_files(vert: impl Into<PathBuf>, frag: impl Into<PathBuf>) -> Self {
        Self {
            vert: ShaderSource::File(vert.into()),
            frag: ShaderSource::File(frag.into()),
        }
    }

    /// Use inline GLSL strings for both shaders.
    pub fn from_inline(vert: impl Into<String>, frag: impl Into<String>) -> Self {
        Self {
            vert: ShaderSource::Inline(vert.into()),
            frag: ShaderSource::Inline(frag.into()),
        }
    }
}

/// A loaded shader program — vertex and fragment GLSL source ready for
/// compilation by the renderer.
///
/// Pass `vert_src` and `frag_src` to
/// [`Renderer::register_with_shader`](renderer::Renderer::register_with_shader).
pub struct ShaderAsset {
    pub vert_src: String,
    pub frag_src: String,
}

impl Asset for ShaderAsset {
    type Params = ShaderConfig;

    fn load(config: Self::Params) -> Result<Self> {
        Ok(Self {
            vert_src: config.vert.resolve()?,
            frag_src: config.frag.resolve()?,
        })
    }
}
