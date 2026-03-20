use std::any::TypeId;

use bytemuck::{Pod, Zeroable};

use crate::atlas::AtlasTile;
use crate::gpu_image::GpuTextureId;
use crate::scene::{PrimitiveId, Scene};

// ── Shared geometry ────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

// ── Attribute descriptor ───────────────────────────────────────────────────

/// Describes one `layout(location = N) in vec4 ...` attribute of an instance.
pub struct AttribDesc {
    pub location: u32,
    pub size:     i32, // component count (e.g. 4 for vec4)
    pub stride:   i32, // byte stride of the whole instance struct
    pub offset:   i32, // byte offset of this field within the struct
}

// ── Trait ──────────────────────────────────────────────────────────────────

pub trait RenderablePrimitive: Sized + 'static {
    type InstanceData: Pod + Zeroable;

    fn vert_src() -> &'static str;
    fn frag_src() -> &'static str;
    fn attrib_layout() -> &'static [AttribDesc];

    /// Convert this primitive into GPU-ready instance data (pixel coordinates).
    /// The vertex shader receives a `u_viewport` uniform for NDC conversion.
    fn to_instance(&self) -> Self::InstanceData;

    fn bounding_box(&self) -> Rect;
    fn clip_rect(&self) -> Option<Rect> { None }
    fn texture_id(&self) -> Option<GpuTextureId> { None }

    fn add_to_scene(self, scene: &mut Scene) -> PrimitiveId {
        let instance = self.to_instance();
        let bytes = bytemuck::bytes_of(&instance);
        let texture = self.texture_id().map(|t| t.0);
        scene.push_raw(TypeId::of::<Self>(), bytes, texture)
    }
}

// ── Quad ───────────────────────────────────────────────────────────────────

const QUAD_VERT: &str = r#"#version 300 es
precision mediump float;

layout(location = 0) in vec4 i_screen_rect;
layout(location = 1) in vec4 i_color;
layout(location = 2) in vec4 i_clip_rect;

uniform vec2 u_viewport;

out vec4 v_color;
out vec4 v_clip;

void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
    vec2 pos = mix(i_screen_rect.xy, i_screen_rect.zw, corner);
    vec2 ndc = pos / u_viewport * 2.0 - 1.0;
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_color = i_color;
    v_clip  = i_clip_rect;
}
"#;

const QUAD_FRAG: &str = r#"#version 300 es
precision mediump float;

in vec4 v_color;
in vec4 v_clip;

out vec4 frag_color;

void main() {
    if (gl_FragCoord.x < v_clip.x || gl_FragCoord.x > v_clip.z ||
        gl_FragCoord.y < v_clip.y || gl_FragCoord.y > v_clip.w) {
        discard;
    }
    frag_color = v_color;
}
"#;

pub struct Quad {
    pub bounds:    Rect,
    pub color:     [f32; 4],
    pub clip_rect: Option<Rect>,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct QuadInstance {
    screen_rect: [f32; 4], // pixel: x, y, x+w, y+h
    color:       [f32; 4],
    clip_rect:   [f32; 4], // pixel clip or [0,0,∞,∞]
    _pad:        [f32; 4],
}

impl RenderablePrimitive for Quad {
    type InstanceData = QuadInstance;

    fn vert_src() -> &'static str { QUAD_VERT }
    fn frag_src() -> &'static str { QUAD_FRAG }

    fn attrib_layout() -> &'static [AttribDesc] {
        const L: &[AttribDesc] = &[
            AttribDesc { location: 0, size: 4, stride: 64, offset: 0  },
            AttribDesc { location: 1, size: 4, stride: 64, offset: 16 },
            AttribDesc { location: 2, size: 4, stride: 64, offset: 32 },
            // _pad at offset 48 — no attrib
        ];
        L
    }

    fn to_instance(&self) -> QuadInstance {
        let clip = self.clip_rect.unwrap_or(Rect { x: 0.0, y: 0.0, w: 1e9, h: 1e9 });
        QuadInstance {
            screen_rect: [self.bounds.x, self.bounds.y,
                          self.bounds.x + self.bounds.w, self.bounds.y + self.bounds.h],
            color:       self.color,
            clip_rect:   [clip.x, clip.y, clip.x + clip.w, clip.y + clip.h],
            _pad:        [0.0; 4],
        }
    }

    fn bounding_box(&self) -> Rect { self.bounds }
    fn clip_rect(&self) -> Option<Rect> { self.clip_rect }
}

// ── MonoSprite ─────────────────────────────────────────────────────────────

const MONO_VERT: &str = r#"#version 300 es
precision mediump float;

layout(location = 0) in vec4 i_screen_rect;
layout(location = 1) in vec4 i_atlas_rect;
layout(location = 2) in vec4 i_color;
layout(location = 3) in vec4 i_clip_rect;

uniform vec2 u_viewport;

out vec2 v_uv;
out vec4 v_color;
out vec4 v_clip;

void main() {
    vec2 corner = vec2(float(gl_VertexID & 1), float((gl_VertexID >> 1) & 1));
    vec2 pos;
    pos.x = mix(i_screen_rect.x, i_screen_rect.z, corner.x);
    pos.y = mix(i_screen_rect.y, i_screen_rect.w, corner.y);
    vec2 ndc = pos / u_viewport * 2.0 - 1.0;
    gl_Position = vec4(ndc, 0.0, 1.0);
    vec2 raw_uv = mix(i_atlas_rect.xy, i_atlas_rect.zw, corner);
    v_uv    = raw_uv;
    v_color = i_color;
    v_clip  = i_clip_rect;
}
"#;

const MONO_FRAG: &str = r#"#version 300 es
precision mediump float;

uniform sampler2D u_atlas;

in vec2 v_uv;
in vec4 v_color;
in vec4 v_clip;

out vec4 frag_color;

void main() {
    if (gl_FragCoord.x < v_clip.x || gl_FragCoord.x > v_clip.z ||
        gl_FragCoord.y < v_clip.y || gl_FragCoord.y > v_clip.w) {
        discard;
    }
    float alpha = texture(u_atlas, v_uv).r;
    frag_color = vec4(v_color.rgb, v_color.a * alpha);
}
"#;

pub struct MonoSprite {
    pub bounds:      Rect,
    pub tile:        AtlasTile,
    pub atlas_size:  (u32, u32),
    pub atlas_tex:   glow::Texture,
    pub color:       [f32; 4],
    pub clip:        Option<Rect>,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct MonoSpriteInstance {
    screen_rect: [f32; 4], // pixel: x, y, x+w, y+h
    atlas_rect:  [f32; 4], // UV [0,1]: u0, v0, u1, v1
    color:       [f32; 4],
    clip_rect:   [f32; 4],
}

impl RenderablePrimitive for MonoSprite {
    type InstanceData = MonoSpriteInstance;

    fn vert_src() -> &'static str { MONO_VERT }
    fn frag_src() -> &'static str { MONO_FRAG }

    fn attrib_layout() -> &'static [AttribDesc] {
        const L: &[AttribDesc] = &[
            AttribDesc { location: 0, size: 4, stride: 64, offset: 0  },
            AttribDesc { location: 1, size: 4, stride: 64, offset: 16 },
            AttribDesc { location: 2, size: 4, stride: 64, offset: 32 },
            AttribDesc { location: 3, size: 4, stride: 64, offset: 48 },
        ];
        L
    }

    fn to_instance(&self) -> MonoSpriteInstance {
        let (aw, ah) = (self.atlas_size.0 as f32, self.atlas_size.1 as f32);
        let u0 = self.tile.x as f32 / aw;
        let v0 = self.tile.y as f32 / ah;
        let u1 = (self.tile.x + self.tile.w) as f32 / aw;
        let v1 = (self.tile.y + self.tile.h) as f32 / ah;

        let clip = self.clip.unwrap_or(Rect { x: 0.0, y: 0.0, w: 1e9, h: 1e9 });
        MonoSpriteInstance {
            screen_rect: [self.bounds.x, self.bounds.y,
                          self.bounds.x + self.bounds.w, self.bounds.y + self.bounds.h],
            atlas_rect:  [u0, v0, u1, v1],
            color:       self.color,
            clip_rect:   [clip.x, clip.y, clip.x + clip.w, clip.y + clip.h],
        }
    }

    fn bounding_box(&self) -> Rect { self.bounds }
    fn clip_rect(&self) -> Option<Rect> { self.clip }
    fn texture_id(&self) -> Option<GpuTextureId> { Some(GpuTextureId(self.atlas_tex)) }
}
