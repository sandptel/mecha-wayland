pub trait RenderableSurface {
    fn fbo(&self) -> glow::Framebuffer;
    fn size(&self) -> (u32, u32);
}

pub struct DmaBufSurface {
    pub fbo: glow::Framebuffer,
    pub width: u32,
    pub height: u32,
}

impl RenderableSurface for DmaBufSurface {
    fn fbo(&self) -> glow::Framebuffer { self.fbo }
    fn size(&self) -> (u32, u32) { (self.width, self.height) }
}
