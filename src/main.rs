#![allow(unused_variables, unused_mut, dead_code)]

mod callback;
mod connection;
mod display;
mod object;
mod registry;
mod shm;
mod wire;
mod xdg;

use callback::SyncCallback;
use connection::Connection;
use object::Object;
use registry::Registry;
use shm::{alloc_shm_file, ShmHandler};
use std::io;
use tracing::{debug, info};
use xdg::{Toplevel, WmBase, XdgSurf};

include!(concat!(env!("OUT_DIR"), "/wayland_protocol.rs"));

macro_rules! dispatch_to {
    ($conn:expr, $obj_id:expr, $opcode:expr, $body:expr; $($handler:expr),+ $(,)?) => {{
        $(
            if $obj_id == $handler.object_id() {
                $handler.dispatch(&mut $conn, $opcode, $body)?;
            } else
        )+
        {
            tracing::debug!(obj_id = $obj_id, opcode = $opcode, "unhandled event");
        }
    }};
}

impl WlCompositorHandler for WlCompositor {}
impl WlShmPoolHandler for WlShmPool {}
impl WlSurfaceHandler for WlSurface {
    fn on_enter(&mut self, _event: WlSurfaceEnterEvent) {}
    fn on_leave(&mut self, _event: WlSurfaceLeaveEvent) {}
}
impl WlBufferHandler for WlBuffer {
    fn on_release(&mut self) {}
}

fn main() -> io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut conn = Connection::connect()?;

    let mut display = WlDisplay::new(1);
    let mut registry = Registry::new(conn.alloc_id());
    let mut sync = SyncCallback::new(conn.alloc_id());

    display.get_registry(&mut conn, &registry.inner)?;
    display.sync(&mut conn, &sync)?;
    conn.flush()?;

    info!("waiting for globals");

    loop {
        let (obj_id, opcode, body) = conn.recv_msg()?;
        debug!(obj_id, opcode, bytes = body.len(), "event received");
        dispatch_to!(conn, obj_id, opcode, &body; display, registry, sync);
        if sync.done {
            break;
        }
    }

    info!("sync complete, binding globals");

    let (comp_name, comp_ver) = registry.find("wl_compositor").expect("wl_compositor missing");
    let (shm_name, shm_ver) = registry.find("wl_shm").expect("wl_shm missing");
    let (xdg_name, _) = registry.find("xdg_wm_base").expect("xdg_wm_base missing");

    let compositor = WlCompositor::new(conn.alloc_id());
    let shm_inner = WlShm::new(conn.alloc_id());
    let wm_inner = XdgWmBase::new(conn.alloc_id());

    registry.bind(&mut conn, comp_name, "wl_compositor", comp_ver.min(4), &compositor)?;
    registry.bind(&mut conn, shm_name, "wl_shm", shm_ver.min(1), &shm_inner)?;
    registry.bind(&mut conn, xdg_name, "xdg_wm_base", 1, &wm_inner)?;

    let mut shm = ShmHandler { inner: shm_inner };
    let mut wm_base = WmBase::new(wm_inner);

    const WIDTH: i32 = 640;
    const HEIGHT: i32 = 480;
    let stride = WIDTH * 4;
    let size = (stride * HEIGHT) as usize;

    let fd = alloc_shm_file(size, [0x88, 0x77, 0x44, 0x00])?;

    let pool_obj = WlShmPool::new(conn.alloc_id());
    let mut buffer_obj = WlBuffer::new(conn.alloc_id());
    let mut surface = WlSurface::new(conn.alloc_id());
    let xdg_inner = XdgSurface::new(conn.alloc_id());
    let top_inner = XdgToplevel::new(conn.alloc_id());

    shm.create_pool(&mut conn, &pool_obj, fd, size as i32)?;
    pool_obj.create_buffer(&mut conn, &buffer_obj, 0, WIDTH, HEIGHT, stride, 1)?;
    compositor.create_surface(&mut conn, &surface)?;
    wm_base.get_xdg_surface(&mut conn, &xdg_inner, &surface)?;

    let mut xdg_surf = XdgSurf::new(xdg_inner);
    let mut toplevel = Toplevel::new(top_inner);

    xdg_surf.get_toplevel(&mut conn, &toplevel.inner)?;
    toplevel.set_title(&mut conn, "hello wayland")?;
    toplevel.set_app_id(&mut conn, "basic-wayland-client")?;
    surface.commit(&mut conn)?;
    conn.flush()?;

    info!("surface created, entering event loop");

    let mut attached = false;
    loop {
        let (obj_id, opcode, body) = conn.recv_msg()?;
        debug!(obj_id, opcode, "event");
        dispatch_to!(conn, obj_id, opcode, &body;
            display, registry, shm, wm_base, xdg_surf, toplevel, surface, buffer_obj);

        if let Some(serial) = wm_base.pending_pong.take() {
            wm_base.pong(&mut conn, serial)?;
        }
        if let Some(serial) = xdg_surf.pending_ack.take() {
            xdg_surf.ack_configure(&mut conn, serial)?;
            if !attached {
                surface.attach(&mut conn, &buffer_obj, 0, 0)?;
                surface.damage(&mut conn, 0, 0, WIDTH, HEIGHT)?;
                surface.commit(&mut conn)?;
                attached = true;
                info!("buffer attached, window should be visible");
            }
        }
        if toplevel.closed {
            info!("window closed");
            break;
        }
        conn.flush()?;
    }

    Ok(())
}
