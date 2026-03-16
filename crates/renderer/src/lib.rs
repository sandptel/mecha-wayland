use std::ffi::c_void;
use std::os::unix::io::{AsFd, AsRawFd, OwnedFd};

use anyhow::{Context, Result, bail};
use gbm::{AsRaw, BufferObjectFlags, Device as GbmDevice, Format as GbmFormat};
use glow::HasContext;
use khronos_egl as egl;

const EGL_PLATFORM_GBM_MESA: u32 = 0x31D7;
const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: i32 = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: i32 = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: i32 = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: i32 = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: i32 = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: i32 = 0x3444;
// DRM_FORMAT_ARGB8888 = fourcc_code('A','R','2','4')
const DRM_FORMAT_ARGB8888: u32 = 0x34325241;

type PfnGetPlatformDisplay = unsafe extern "C" fn(u32, *const c_void, *const i32) -> *const c_void;
type PfnCreateImageKHR = unsafe extern "C" fn(
    *const c_void,
    *const c_void,
    u32,
    *const c_void,
    *const i32,
) -> *mut c_void;
type PfnDestroyImageKHR = unsafe extern "C" fn(*const c_void, *mut c_void) -> u32;
type PfnImageTargetRenderbufferStorageOES = unsafe extern "C" fn(u32, *mut c_void);

pub struct DmaBufFrame {
    pub fd: OwnedFd,
    pub stride: u32,
    pub offset: u32,
    pub format: u32,
    pub modifier: u64,
}

pub struct Renderer {
    gl: glow::Context,
    egl_lib: egl::DynamicInstance<egl::EGL1_4>,
    display: egl::Display,
    context: egl::Context,
    rbo: glow::Renderbuffer,
    fbo: glow::Framebuffer,
    egl_image: *mut c_void,
    frame_stride: u32,
    fn_destroy_image: PfnDestroyImageKHR,
    width: u32,
    height: u32,
    // Prime fd exported once from the GBM BO. Duped per-frame to avoid
    // repeatedly calling drmPrimeHandleToFD which has a per-BO fd limit.
    bo_prime_fd: OwnedFd,
    // GBM objects — kept alive for the lifetime of the renderer.
    // gbm_bo must be dropped before _gbm_device (reverse field order).
    gbm_bo: gbm::BufferObject<()>,
    _gbm_device: GbmDevice<std::fs::File>,
}

// SAFETY: EGLImageKHR pointer is only accessed from one thread
unsafe impl Send for Renderer {}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        // Open DRM render node and create GBM device
        let drm_file = open_drm_render_node()?;
        let gbm_device =
            GbmDevice::new(drm_file).map_err(|e| anyhow::anyhow!("gbm_create_device: {}", e))?;

        // Load EGL dynamically
        let egl_lib = unsafe {
            egl::DynamicInstance::<egl::EGL1_4>::load_required().context("failed to load EGL")?
        };

        // Load extension function pointers
        let fn_get_platform_display: PfnGetPlatformDisplay = unsafe {
            let ptr = egl_lib
                .get_proc_address("eglGetPlatformDisplayEXT")
                .context("eglGetPlatformDisplayEXT not available")?;
            std::mem::transmute(ptr)
        };
        let fn_create_image: PfnCreateImageKHR = unsafe {
            let ptr = egl_lib
                .get_proc_address("eglCreateImageKHR")
                .context("eglCreateImageKHR not available")?;
            std::mem::transmute(ptr)
        };
        let fn_destroy_image: PfnDestroyImageKHR = unsafe {
            let ptr = egl_lib
                .get_proc_address("eglDestroyImageKHR")
                .context("eglDestroyImageKHR not available")?;
            std::mem::transmute(ptr)
        };
        let fn_image_target_rbo: PfnImageTargetRenderbufferStorageOES = unsafe {
            let ptr = egl_lib
                .get_proc_address("glEGLImageTargetRenderbufferStorageOES")
                .context("glEGLImageTargetRenderbufferStorageOES not available")?;
            std::mem::transmute(ptr)
        };

        // Create EGL display backed by the GBM device (ensures same GPU as GBM)
        let raw_dpy = unsafe {
            fn_get_platform_display(
                EGL_PLATFORM_GBM_MESA,
                gbm_device.as_raw() as *const c_void,
                std::ptr::null(),
            )
        };
        if raw_dpy.is_null() {
            bail!("eglGetPlatformDisplayEXT(GBM) returned EGL_NO_DISPLAY");
        }
        let display = unsafe { egl::Display::from_ptr(raw_dpy as *mut c_void) };

        egl_lib
            .initialize(display)
            .context("eglInitialize failed")?;
        egl_lib
            .bind_api(egl::OPENGL_ES_API)
            .context("failed to bind GLES API")?;

        let config_attribs = [
            egl::RENDERABLE_TYPE,
            egl::OPENGL_ES2_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::NONE,
        ];
        let config = egl_lib
            .choose_first_config(display, &config_attribs)
            .context("eglChooseConfig failed")?
            .context("no suitable EGL config found")?;

        let ctx_attribs = [egl::CONTEXT_CLIENT_VERSION, 2, egl::NONE];
        let context = egl_lib
            .create_context(display, config, None, &ctx_attribs)
            .context("failed to create EGL context")?;

        egl_lib
            .make_current(display, None, None, Some(context))
            .context("eglMakeCurrent (surfaceless) failed")?;

        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                let sym = std::ffi::CString::new(s).unwrap();
                egl_lib
                    .get_proc_address(sym.to_str().unwrap())
                    .map(|f| f as *const _)
                    .unwrap_or(std::ptr::null())
            })
        };

        // Allocate a linear GBM buffer object — GPU renders directly into linear memory
        let gbm_bo = gbm_device
            .create_buffer_object::<()>(
                width,
                height,
                GbmFormat::Argb8888,
                BufferObjectFlags::RENDERING | BufferObjectFlags::LINEAR,
            )
            .map_err(|e| anyhow::anyhow!("gbm_bo_create (linear ARGB8888): {}", e))?;

        tracing::info!(
            modifier = format!("0x{:016x}", u64::from(gbm_bo.modifier())),
            "gbm bo modifier after creation"
        );

        let bo_fd = gbm_bo
            .fd()
            .map_err(|e| anyhow::anyhow!("gbm_bo_get_fd: {}", e))?;
        let frame_stride = gbm_bo.stride();

        // Import the linear GBM BO as an EGLImage via dmabuf
        let raw_display = display.as_ptr() as *const c_void;
        let import_attribs: [i32; 17] = [
            egl::WIDTH,
            width as i32,
            egl::HEIGHT,
            height as i32,
            EGL_LINUX_DRM_FOURCC_EXT,
            DRM_FORMAT_ARGB8888 as i32,
            EGL_DMA_BUF_PLANE0_FD_EXT,
            bo_fd.as_raw_fd(),
            EGL_DMA_BUF_PLANE0_OFFSET_EXT,
            0,
            EGL_DMA_BUF_PLANE0_PITCH_EXT,
            frame_stride as i32,
            EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
            0, // DRM_FORMAT_MOD_LINEAR = 0
            EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
            0,
            egl::NONE,
        ];
        let egl_image = unsafe {
            (fn_create_image)(
                raw_display,
                std::ptr::null(), // EGL_NO_CONTEXT
                EGL_LINUX_DMA_BUF_EXT,
                std::ptr::null(), // buffer = NULL for DMA_BUF target
                import_attribs.as_ptr(),
            )
        };
        if egl_image.is_null() {
            bail!("eglCreateImageKHR(EGL_LINUX_DMA_BUF_EXT) failed");
        }

        // Create a renderbuffer whose storage is the imported EGLImage
        let rbo = unsafe {
            let rbo = gl
                .create_renderbuffer()
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rbo));
            fn_image_target_rbo(glow::RENDERBUFFER, egl_image);
            rbo
        };

        // Create FBO with the renderbuffer as color attachment
        let fbo = unsafe {
            let fbo = gl
                .create_framebuffer()
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::RENDERBUFFER,
                Some(rbo),
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if status != glow::FRAMEBUFFER_COMPLETE {
                bail!("FBO incomplete: 0x{:x}", status);
            }
            gl.viewport(0, 0, width as i32, height as i32);
            fbo
        };

        Ok(Self {
            gl,
            egl_lib,
            display,
            context,
            rbo,
            fbo,
            egl_image,
            frame_stride,
            fn_destroy_image,
            width,
            height,
            bo_prime_fd: bo_fd,
            gbm_bo,
            _gbm_device: gbm_device,
        })
    }

    pub fn clear_screen(&self, r: f32, g: f32, b: f32) {
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            self.gl.clear_color(r, g, b, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    /// Flush GPU work and return a dmabuf fd pointing at the rendered frame.
    /// Each call dups the GBM BO fd; modifier is always 0 (DRM_FORMAT_MOD_LINEAR).
    pub fn present(&mut self) -> Result<DmaBufFrame> {
        unsafe { self.gl.finish() };

        let fd = self
            .bo_prime_fd
            .as_fd()
            .try_clone_to_owned()
            .map_err(|e| anyhow::anyhow!("dup(bo_prime_fd): {}", e))?;
        Ok(DmaBufFrame {
            fd,
            stride: self.frame_stride,
            offset: 0,
            format: DRM_FORMAT_ARGB8888,
            modifier: 0, // DRM_FORMAT_MOD_LINEAR
        })
    }

    pub fn sync(&self) {
        unsafe { self.gl.finish() };
    }

    pub fn release_frame(&mut self) {}
}

fn open_drm_render_node() -> Result<std::fs::File> {
    for i in 128..=255 {
        let path = format!("/dev/dri/renderD{}", i);
        if let Ok(f) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
        {
            return Ok(f);
        }
    }
    bail!("no DRM render node found in /dev/dri/renderD128..255")
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let raw_display = self.display.as_ptr() as *const c_void;
        unsafe { (self.fn_destroy_image)(raw_display, self.egl_image) };
        unsafe {
            self.gl.delete_framebuffer(self.fbo);
            self.gl.delete_renderbuffer(self.rbo);
        }
        let _ = self.egl_lib.make_current(self.display, None, None, None);
        let _ = self.egl_lib.destroy_context(self.display, self.context);
        let _ = self.egl_lib.terminate(self.display);
        // gbm_bo is dropped before _gbm_device (reverse field declaration order)
    }
}
