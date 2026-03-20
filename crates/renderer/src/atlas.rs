use std::collections::HashMap;

use anyhow::Result;
use etagere::{BucketedAtlasAllocator, size2};
use glow::HasContext;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AtlasKey {
    pub font_id:       u32,
    pub glyph_id:      u32,
    pub font_size_x64: u32, // px * 64 to avoid float hashing
}

#[derive(Clone, Copy, Debug)]
pub struct AtlasTile {
    pub x:         u16,
    pub y:         u16,
    pub w:         u16,
    pub h:         u16,
    pub bearing_x: i16,
    pub bearing_y: i16,
}

pub struct GlyphAtlas {
    pub texture:   glow::Texture,
    allocator:     BucketedAtlasAllocator,
    cache:         HashMap<AtlasKey, AtlasTile>,
    pub width:     u32,
    pub height:    u32,
}

impl GlyphAtlas {
    pub fn new(gl: &glow::Context, size: u32) -> Result<Self> {
        let texture = unsafe {
            let t = gl.create_texture().map_err(|e| anyhow::anyhow!("{e}"))?;
            gl.bind_texture(glow::TEXTURE_2D, Some(t));
            // Allocate zeroed storage for the atlas.
            let zeros = vec![0u8; (size * size) as usize];
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R8 as i32,
                size as i32,
                size as i32,
                0,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(zeros.as_slice())),
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            gl.bind_texture(glow::TEXTURE_2D, None);
            t
        };

        Ok(Self {
            texture,
            allocator: BucketedAtlasAllocator::new(size2(size as i32, size as i32)),
            cache: HashMap::new(),
            width: size,
            height: size,
        })
    }

    pub fn get(&self, key: &AtlasKey) -> Option<AtlasTile> {
        self.cache.get(key).copied()
    }

    /// Insert a glyph bitmap into the atlas. `bitmap` is row-major, top-to-bottom.
    pub fn insert(
        &mut self,
        gl: &glow::Context,
        key: AtlasKey,
        bitmap: &[u8],
        w: u16,
        h: u16,
        bearing_x: i16,
        bearing_y: i16,
    ) -> Result<AtlasTile> {
        // 1px padding prevents UV bleeding at tile borders.
        let alloc = self
            .allocator
            .allocate(size2(w as i32 + 1, h as i32 + 1))
            .ok_or_else(|| anyhow::anyhow!("glyph atlas full"))?;

        let tx = alloc.rectangle.min.x as i32;
        let ty = alloc.rectangle.min.y as i32;

        if !bitmap.is_empty() {
            unsafe {
                gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    tx,
                    ty,
                    w as i32,
                    h as i32,
                    glow::RED,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(bitmap)),
                );
                gl.bind_texture(glow::TEXTURE_2D, None);
            }
        }

        let tile = AtlasTile {
            x: tx as u16,
            y: ty as u16,
            w,
            h,
            bearing_x,
            bearing_y,
        };
        self.cache.insert(key, tile);
        Ok(tile)
    }
}
