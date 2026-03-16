#![allow(unused_variables, unused_mut, dead_code)]

use anyhow::Result;
use renderer::Renderer;
use std::time::{Duration, Instant};
use wayland_protocols::connection::Connection;
use wayland_protocols::wl_callback::SyncCallback;
use wayland_protocols::wl_display::Display;
use wayland_protocols::wl_registry::Registry;
use wayland_protocols::xdg_surface::XdgSurf;
use wayland_protocols::xdg_toplevel::Toplevel;
use wayland_protocols::xdg_wm_base::WmBase;
use wayland_protocols::zwp_linux_dmabuf::DmaBuf;
use wayland_protocols::*;

/// HSV → RGB, all values in [0.0, 1.0] except hue which is in [0, 360).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = h % 360.0;
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (r1 + m, g1 + m, b1 + m)
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut conn = Connection::connect()?;

    let mut display = Display::new(1);
    let mut registry = Registry::new(conn.alloc_id());
    let mut sync = SyncCallback::new(conn.alloc_id());

    display.inner.get_registry(&mut conn, &registry.inner)?;
    display.inner.sync(&mut conn, &sync)?;
    conn.flush()?;

    tracing::info!("waiting for globals");

    loop {
        let (obj_id, opcode, body) = conn.recv_msg()?;
        dispatch_to!(conn, obj_id, opcode, &body; display, registry, sync);
        if sync.done {
            break;
        }
    }

    tracing::info!("sync complete, binding globals");

    let (comp_name, comp_ver) = registry
        .find("wl_compositor")
        .expect("wl_compositor missing");
    let (xdg_name, _) = registry.find("xdg_wm_base").expect("xdg_wm_base missing");
    let (dmabuf_name, dmabuf_ver) = registry
        .find("zwp_linux_dmabuf_v1")
        .expect("zwp_linux_dmabuf_v1 missing");

    let compositor = WlCompositor::new(conn.alloc_id());
    let wm_inner = XdgWmBase::new(conn.alloc_id());
    let dmabuf_inner = ZwpLinuxDmabufV1::new(conn.alloc_id());

    registry.inner.bind(
        &mut conn,
        comp_name,
        "wl_compositor",
        comp_ver.min(4),
        &compositor,
    )?;
    registry
        .inner
        .bind(&mut conn, xdg_name, "xdg_wm_base", 1, &wm_inner)?;
    registry.inner.bind(
        &mut conn,
        dmabuf_name,
        "zwp_linux_dmabuf_v1",
        dmabuf_ver.min(4),
        &dmabuf_inner,
    )?;

    let mut wm_base = WmBase::new(wm_inner);
    let mut dmabuf = DmaBuf::new(dmabuf_inner);

    let mut surface = WlSurface::new(conn.alloc_id());
    let xdg_inner = XdgSurface::new(conn.alloc_id());
    let top_inner = XdgToplevel::new(conn.alloc_id());

    compositor.create_surface(&mut conn, &surface)?;
    wm_base
        .inner
        .get_xdg_surface(&mut conn, &xdg_inner, &surface)?;

    let mut xdg_surf = XdgSurf::new(xdg_inner);
    let mut toplevel = Toplevel::new(top_inner);

    xdg_surf.inner.get_toplevel(&mut conn, &toplevel.inner)?;
    toplevel.inner.set_title(&mut conn, "hello wayland")?;
    toplevel.inner.set_app_id(&mut conn, "mecha-wayland")?;
    surface.commit(&mut conn)?;
    conn.flush()?;

    tracing::info!("surface created, entering event loop (VSync unlocked)");

    const WIDTH: u32 = 1028;
    const HEIGHT: u32 = 1080;

    let mut renderer = Renderer::new(WIDTH, HEIGHT)?;
    let mut configured = false;
    let mut wl_buf: Option<WlBuffer> = None;

    let start = Instant::now();
    let mut frame_count: u64 = 0;
    let mut last_fps_report = Instant::now();

    loop {
        // Drain all pending Wayland events without blocking.
        while let Some((obj_id, opcode, body)) = conn.try_recv_msg()? {
            dispatch_to!(conn, obj_id, opcode, &body;
                        display, registry, dmabuf, wm_base, xdg_surf, toplevel, surface);
        }

        if let Some(serial) = wm_base.pending_pong.take() {
            wm_base.inner.pong(&mut conn, serial)?;
        }

        if let Some(serial) = xdg_surf.pending_ack.take() {
            xdg_surf.inner.ack_configure(&mut conn, serial)?;
            configured = true;
        }

        if configured {
            // One-time buffer setup: create params + wl_buffer once, then reuse.
            if wl_buf.is_none() {
                let frame = renderer.present()?;
                let params = ZwpLinuxBufferParamsV1::new(conn.alloc_id());
                dmabuf.inner.create_params(&mut conn, &params)?;
                let mod_hi = (frame.modifier >> 32) as u32;
                let mod_lo = frame.modifier as u32;
                params.add(
                    &mut conn,
                    frame.fd,
                    0,
                    frame.offset,
                    frame.stride,
                    mod_hi,
                    mod_lo,
                )?;
                let buf = WlBuffer::new(conn.alloc_id());
                params.create_immed(
                    &mut conn,
                    &buf,
                    WIDTH as i32,
                    HEIGHT as i32,
                    frame.format,
                    0,
                )?;
                params.destroy(&mut conn)?;
                wl_buf = Some(buf);
            }

            let buf = wl_buf.as_ref().unwrap();

            // Animate hue: full rotation every 5 seconds.
            let t = start.elapsed().as_secs_f32();
            let hue = (t * 72.0) % 360.0;
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);

            renderer.clear_screen(r, g, b);
            renderer.sync();

            surface.attach(&mut conn, buf, 0, 0)?;
            surface.damage(&mut conn, 0, 0, WIDTH as i32, HEIGHT as i32)?;
            surface.commit(&mut conn)?;

            frame_count += 1;

            let now = Instant::now();
            let since_last = now.duration_since(last_fps_report);
            if since_last >= Duration::from_secs(60) {
                let fps = frame_count as f64 / since_last.as_secs_f64();
                tracing::info!(fps = format!("{:.1}", fps), "FPS report");
                frame_count = 0;
                last_fps_report = now;
            }
        }

        if toplevel.closed {
            tracing::info!("window closed");
            break;
        }

        conn.flush()?;
    }

    if let Some(buf) = wl_buf {
        buf.destroy(&mut conn)?;
        conn.flush()?;
    }

    Ok(())
}
