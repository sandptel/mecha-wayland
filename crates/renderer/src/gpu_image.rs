use std::sync::{Arc, Mutex};

/// Opaque GPU texture handle. Copy because it's just a NonZeroU32.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GpuTextureId(pub glow::Texture);

/// Ref-counted GPU texture. Dropping the last Arc queues the texture for
/// deferred deletion on the render thread (where the EGL context is current).
pub struct GpuImage {
    pub texture: glow::Texture,
    pub width:   u32,
    pub height:  u32,
    deletion_queue: Arc<Mutex<Vec<GpuTextureId>>>,
}

impl Drop for GpuImage {
    fn drop(&mut self) {
        if let Ok(mut q) = self.deletion_queue.lock() {
            q.push(GpuTextureId(self.texture));
        }
    }
}

impl GpuImage {
    pub fn new(
        texture: glow::Texture,
        width: u32,
        height: u32,
        deletion_queue: Arc<Mutex<Vec<GpuTextureId>>>,
    ) -> Self {
        Self { texture, width, height, deletion_queue }
    }

    pub fn id(&self) -> GpuTextureId {
        GpuTextureId(self.texture)
    }
}
