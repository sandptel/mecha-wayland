use crate::object::Object;
use crate::{
    XdgSurface, XdgSurfaceConfigureEvent, XdgSurfaceHandler, XdgToplevel,
    XdgToplevelConfigureBoundsEvent, XdgToplevelConfigureEvent, XdgToplevelHandler,
    XdgToplevelWmCapabilitiesEvent, XdgWmBase, XdgWmBasePingEvent, XdgWmBaseHandler,
};

pub struct WmBase {
    pub inner: XdgWmBase,
    pub pending_pong: Option<u32>,
}

impl WmBase {
    pub fn new(inner: XdgWmBase) -> Self {
        WmBase { inner, pending_pong: None }
    }
}

impl Object for WmBase {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl XdgWmBaseHandler for WmBase {
    fn on_ping(&mut self, event: XdgWmBasePingEvent) {
        tracing::debug!(serial = event.serial, "xdg_wm_base::ping");
        self.pending_pong = Some(event.serial);
    }
}

pub struct XdgSurf {
    pub inner: XdgSurface,
    pub pending_ack: Option<u32>,
    pub configured: bool,
}

impl XdgSurf {
    pub fn new(inner: XdgSurface) -> Self {
        XdgSurf { inner, pending_ack: None, configured: false }
    }
}

impl Object for XdgSurf {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl XdgSurfaceHandler for XdgSurf {
    fn on_configure(&mut self, event: XdgSurfaceConfigureEvent) {
        tracing::debug!(serial = event.serial, "xdg_surface::configure");
        self.pending_ack = Some(event.serial);
        self.configured = true;
    }
}

pub struct Toplevel {
    pub inner: XdgToplevel,
    pub closed: bool,
    pub width: i32,
    pub height: i32,
}

impl Toplevel {
    pub fn new(inner: XdgToplevel) -> Self {
        Toplevel { inner, closed: false, width: 0, height: 0 }
    }
}

impl Object for Toplevel {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl XdgToplevelHandler for Toplevel {
    fn on_configure(&mut self, event: XdgToplevelConfigureEvent) {
        tracing::debug!(width = event.width, height = event.height, "xdg_toplevel::configure");
        if event.width != 0 {
            self.width = event.width;
        }
        if event.height != 0 {
            self.height = event.height;
        }
    }

    fn on_close(&mut self) {
        tracing::info!("xdg_toplevel::close");
        self.closed = true;
    }

    fn on_configure_bounds(&mut self, _event: XdgToplevelConfigureBoundsEvent) {}

    fn on_wm_capabilities(&mut self, _event: XdgToplevelWmCapabilitiesEvent) {}
}
