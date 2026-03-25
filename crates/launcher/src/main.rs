#![allow(unused_variables, unused_mut, dead_code)]
use anyhow::Result;
use calloop::generic::Generic;
use calloop::{EventLoop, Interest, Mode, PostAction};
use launcher::event_manager::{
    Element, EventCtx, EventManager, EventMask, EventResult, MouseButton, PaintCtx, UiEvent,
    UiRect, UiTree,
};
use launcher::{profile_function, profile_scope};
use renderer::primitives::RenderablePrimitive as _;
use renderer::{DmaBufSurface, MonoSprite, Quad, Rect, Renderer, Scene, TextSystem};
use std::time::{Duration, Instant};
use wayland_protocols::connection::Connection;
use wayland_protocols::wl_callback::SyncCallback;
use wayland_protocols::wl_display::Display;
use wayland_protocols::wl_pointer::{Pointer, RawPointerEvent};
use wayland_protocols::wl_registry::Registry;
use wayland_protocols::wl_seat::Seat;
use wayland_protocols::xdg_surface::XdgSurf;
use wayland_protocols::xdg_toplevel::Toplevel;
use wayland_protocols::xdg_wm_base::WmBase;
use wayland_protocols::zwp_linux_dmabuf::DmaBuf;
use wayland_protocols::*;

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

struct DemoElement {
    bounds: UiRect,
    base_color: [f32; 4],
    hovered: bool,
    pressed: bool,
}

impl DemoElement {
    fn new(bounds: UiRect, base_color: [f32; 4]) -> Self {
        Self {
            bounds,
            base_color,
            hovered: false,
            pressed: false,
        }
    }

    fn display_color(&self) -> [f32; 4] {
        if self.pressed {
            [
                (self.base_color[0] * 0.6).min(1.0),
                (self.base_color[1] * 0.6).min(1.0),
                (self.base_color[2] * 0.6).min(1.0),
                self.base_color[3],
            ]
        } else if self.hovered {
            [
                (self.base_color[0] * 1.2).min(1.0),
                (self.base_color[1] * 1.2).min(1.0),
                (self.base_color[2] * 1.2).min(1.0),
                self.base_color[3],
            ]
        } else {
            self.base_color
        }
    }
}

impl Element for DemoElement {
    fn bounds(&self) -> UiRect {
        self.bounds
    }

    fn event_mask(&self) -> EventMask {
        EventMask::ALL_POINTER
    }

    fn on_event(&mut self, event: &UiEvent, ctx: &mut EventCtx) -> EventResult {
        match event {
            UiEvent::PointerEnter { .. } => {
                self.hovered = true;
                ctx.request_repaint_rect(self.bounds);
                EventResult::Handled
            }
            UiEvent::PointerLeave { .. } => {
                self.hovered = false;
                self.pressed = false;
                ctx.request_repaint_rect(self.bounds);
                EventResult::Handled
            }
            UiEvent::PointerDown { .. } => {
                self.pressed = true;
                ctx.request_repaint_rect(self.bounds);
                EventResult::Handled
            }
            UiEvent::PointerUp { .. } => {
                self.pressed = false;
                ctx.request_repaint_rect(self.bounds);
                EventResult::Handled
            }
            UiEvent::PointerMove { .. } => EventResult::Ignored,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>) {
        Quad {
            bounds: Rect {
                x: self.bounds.x,
                y: self.bounds.y,
                w: self.bounds.w,
                h: self.bounds.h,
            },
            color: self.display_color(),
            clip_rect: None,
        }
        .add_to_scene(ctx.scene);
    }
}

struct LauncherState {
    conn: Connection,
    display: Display,
    registry: Registry,
    wm_base: WmBase,
    dmabuf: DmaBuf,
    surface: WlSurface,
    seat: Option<Seat>,
    pointer: Option<Pointer>,
    xdg_surf: XdgSurf,
    toplevel: Toplevel,
    renderer: Renderer,
    text_sys: TextSystem,
    font_id: u32,
    scene: Scene,
    render_surface: DmaBufSurface,
    width: u32,
    height: u32,
    configured: bool,
    wl_buf: Option<WlBuffer>,
    start: Instant,
    frame_count: u64,
    last_fps_report: Instant,
    event_manager: EventManager,
    ui_tree: UiTree<DemoElement>,
    needs_repaint: bool,
    fatal_error: Option<anyhow::Error>,
}

impl LauncherState {
    const WIDTH: u32 = 1028;
    const HEIGHT: u32 = 1080;
    const TITLE: &'static str = "hello wayland";
    const APP_ID: &'static str = "mecha-wayland";

    fn new() -> Result<Self> {
        let (mut conn, display, registry, mut wm_base, dmabuf, compositor, seat) =
            Self::init_wayland_globals()?;

        let (surface, xdg_surf, toplevel) =
            Self::create_window(&mut conn, &compositor, &mut wm_base)?;

        let (renderer, text_sys, font_id, scene, render_surface) =
            Self::init_renderer(Self::WIDTH, Self::HEIGHT)?;
        let ui_tree = Self::init_ui_tree(Self::WIDTH, Self::HEIGHT);

        Ok(Self {
            conn,
            display,
            registry,
            wm_base,
            dmabuf,
            surface,
            seat,
            pointer: None,
            xdg_surf,
            toplevel,
            renderer,
            text_sys,
            font_id,
            scene,
            render_surface,
            width: Self::WIDTH,
            height: Self::HEIGHT,
            configured: false,
            wl_buf: None,
            start: Instant::now(),
            frame_count: 0,
            last_fps_report: Instant::now(),
            event_manager: EventManager::new(),
            ui_tree,
            needs_repaint: true,
            fatal_error: None,
        })
    }

    fn init_ui_tree(width: u32, height: u32) -> UiTree<DemoElement> {
        let mut tree = UiTree::new();
        tree.add_root(
            DemoElement::new(
                UiRect {
                    x: 50.0,
                    y: 50.0,
                    w: 200.0,
                    h: 100.0,
                },
                [0.2, 0.7, 0.9, 1.0],
            ),
            1,
        );
        tree.add_root(
            DemoElement::new(
                UiRect {
                    x: 0.0,
                    y: (height - 80) as f32,
                    w: width as f32,
                    h: 80.0,
                },
                [1.0, 0.0, 0.0, 0.8],
            ),
            0,
        );
        tree
    }

    fn init_wayland_globals()
    -> Result<(Connection, Display, Registry, WmBase, DmaBuf, WlCompositor, Option<Seat>)> {
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
        let seat_info = registry.find("wl_seat");

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

        let seat = if let Some((seat_name, seat_ver)) = seat_info {
            let seat = Seat::new(conn.alloc_id());
            registry
                .inner
                .bind(&mut conn, seat_name, "wl_seat", seat_ver.min(7), &seat.inner)?;
            tracing::info!("phase=bind_seat wl_seat bound");
            Some(seat)
        } else {
            tracing::info!("phase=bind_seat wl_seat not available");
            None
        };

        let wm_base = WmBase::new(wm_inner);
        let dmabuf = DmaBuf::new(dmabuf_inner);

        Ok((conn, display, registry, wm_base, dmabuf, compositor, seat))
    }

    fn map_mouse_button(button: u32) -> MouseButton {
        // Linux input-event button codes used by wl_pointer.button.
        match button {
            0x110 => MouseButton::Left,
            0x111 => MouseButton::Right,
            0x112 => MouseButton::Middle,
            other => MouseButton::Other(other),
        }
    }

    fn ensure_pointer_binding(&mut self) -> Result<()> {
        let Some(seat) = self.seat.as_mut() else {
            return Ok(());
        };

        if seat.has_pointer && self.pointer.is_none() {
            let pointer = Pointer::new(self.conn.alloc_id());
            seat.inner.get_pointer(&mut self.conn, &pointer.inner)?;
            self.pointer = Some(pointer);
            tracing::info!("phase=attach_pointer wl_pointer connected");
        } else if !seat.has_pointer && self.pointer.is_some() {
            if let Some(pointer) = &self.pointer {
                pointer.inner.release(&mut self.conn)?;
            }
            self.pointer = None;
            tracing::info!("phase=attach_pointer wl_pointer released");
        }

        Ok(())
    }

    // Drains pending pointer events and translates them into UiEvents for the EventManager.
    fn drain_ui_events_from_pointer(&mut self) {
        let Some(pointer) = self.pointer.as_mut() else {
            return;
        };

        let raw_events = pointer.take_pending_events();
        for raw in raw_events {
            let ui_event = match raw {
                RawPointerEvent::Enter { x, y } => Some(UiEvent::PointerEnter {
                    x: x as f32,
                    y: y as f32,
                }),
                RawPointerEvent::Leave => {
                    let x = pointer.x as f32;
                    let y = pointer.y as f32;
                    Some(UiEvent::PointerLeave { x, y })
                }
                RawPointerEvent::Motion { x, y } => Some(UiEvent::PointerMove {
                    x: x as f32,
                    y: y as f32,
                }),
                RawPointerEvent::Button {
                    button,
                    state,
                    x,
                    y,
                } => {
                    let mapped = Self::map_mouse_button(button);
                    if state == 1 {
                        Some(UiEvent::PointerDown {
                            x: x as f32,
                            y: y as f32,
                            button: mapped,
                        })
                    } else {
                        Some(UiEvent::PointerUp {
                            x: x as f32,
                            y: y as f32,
                            button: mapped,
                        })
                    }
                }
            };

            if let Some(event) = ui_event {
                tracing::trace!(phase = "ui_event_translation", kind = ?event.kind(), "translated raw pointer event");
                self.event_manager.process_event(&mut self.ui_tree, event);
                self.needs_repaint = true;
            }
        }
    }

    fn process_wayland_messages(&mut self) -> Result<()> {
        tracing::trace!(phase = "wayland_readable", "draining Wayland messages");
        while let Some((obj_id, opcode, body)) = self.conn.try_recv_msg()? {
            let handled_optional = dispatch_optional!(
                self.conn,
                obj_id,
                opcode,
                &body;
                self.pointer,
                self.seat
            );

            if !handled_optional {
                dispatch_to!(self.conn, obj_id, opcode, &body;
                    self.display, self.registry, self.dmabuf, self.wm_base, self.xdg_surf, self.toplevel, self.surface);
            }

            tracing::trace!(phase = "protocol_dispatch", obj_id, opcode, "Wayland event dispatched");
        }

        if let Some(serial) = self.wm_base.pending_pong.take() {
            self.wm_base.inner.pong(&mut self.conn, serial)?;
            tracing::debug!(phase = "protocol_dispatch", "sent xdg_wm_base pong");
        }

        if let Some(serial) = self.xdg_surf.pending_ack.take() {
            self.xdg_surf.inner.ack_configure(&mut self.conn, serial)?;
            self.configured = true;
            self.needs_repaint = true;
            tracing::info!(phase = "protocol_dispatch", "acknowledged xdg surface configure");
        }

        self.ensure_pointer_binding()?;
        self.drain_ui_events_from_pointer();

        let frame_state = self.event_manager.take_frame_state();
        if !frame_state.redraws.is_empty() || frame_state.layout_requested {
            self.needs_repaint = true;
            tracing::debug!(
                phase = "ui_dispatch",
                redraw_count = frame_state.redraws.len(),
                layout_requested = frame_state.layout_requested,
                "ui requested redraw"
            );
        }

        Ok(())
    }

    fn render_once(&mut self) -> Result<()> {
        if !self.configured {
            return Ok(());
        }

        self.ensure_wl_buffer()?;
        let Some(buf) = self.wl_buf.as_ref() else {
            return Ok(());
        };

        let t = self.start.elapsed().as_secs_f32();
        let hue = (t * 72.0) % 360.0;
        let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);

        {
            profile_scope!("renderer");
            tracing::trace!(phase = "frame_build", "building scene");
            self.scene.clear_primitives();
            self.scene.background = (r * 0.15, g * 0.15, b * 0.15);

            let mut paint_ctx = PaintCtx {
                scene: &mut self.scene,
            };
            self.ui_tree.paint_all(&mut paint_ctx);

            self.text_sys.draw_text(
                &mut self.scene,
                self.renderer.gl(),
                "Hello, Wayland!",
                self.font_id,
                24.0,
                [1.0, 1.0, 1.0, 1.0],
                [110.0, 110.0],
            )?;

            self.renderer
                .begin_frame(&self.render_surface, self.scene.background);
            self.renderer
                .render_primitive::<Quad>(&self.scene, &self.render_surface)?;
            self.renderer
                .render_primitive::<MonoSprite>(&self.scene, &self.render_surface)?;
            self.renderer.end_frame();
        }

        {
            profile_scope!("surface_commit");
            tracing::trace!(phase = "frame_commit", "committing surface");
            self.surface.attach(&mut self.conn, buf, 0, 0)?;
            self.surface
                .damage(&mut self.conn, 0, 0, self.width as i32, self.height as i32)?;
            self.surface.commit(&mut self.conn)?;
        }

        self.frame_count += 1;
        let now = Instant::now();
        let since_last = now.duration_since(self.last_fps_report);
        if since_last >= Duration::from_secs(60) {
            let fps = self.frame_count as f64 / since_last.as_secs_f64();
            tracing::info!(fps = format!("{:.1}", fps), "FPS report");
            self.frame_count = 0;
            self.last_fps_report = now;
        }

        self.needs_repaint = false;
        Ok(())
    }

    fn create_window(
        conn: &mut Connection,
        compositor: &WlCompositor,
        wm_base: &mut WmBase,
    ) -> Result<(WlSurface, XdgSurf, Toplevel)> {
        let mut surface = WlSurface::new(conn.alloc_id());
        let xdg_inner = XdgSurface::new(conn.alloc_id());
        let top_inner = XdgToplevel::new(conn.alloc_id());

        compositor.create_surface(conn, &surface)?;
        wm_base.inner.get_xdg_surface(conn, &xdg_inner, &surface)?;

        let mut xdg_surf = XdgSurf::new(xdg_inner);
        let mut toplevel = Toplevel::new(top_inner);

        xdg_surf.inner.get_toplevel(conn, &toplevel.inner)?;
        toplevel.inner.set_title(conn, Self::TITLE)?;
        toplevel.inner.set_app_id(conn, Self::APP_ID)?;
        surface.commit(conn)?;
        conn.flush()?;

        tracing::info!("surface created, entering event loop");

        Ok((surface, xdg_surf, toplevel))
    }

    fn init_renderer(
        width: u32,
        height: u32,
    ) -> Result<(Renderer, TextSystem, u32, Scene, DmaBufSurface)> {
        let mut renderer = Renderer::new(width, height)?;
        renderer.register::<Quad>()?;
        renderer.register::<MonoSprite>()?;

        let mut text_sys = TextSystem::new(renderer.gl(), 1024)?;
        let font_id = text_sys.load_font(include_bytes!("../assets/Inter-Regular.ttf"))?;

        let scene = renderer.create_scene();
        let render_surface = renderer.create_dmabuf_surface();
        Ok((renderer, text_sys, font_id, scene, render_surface))
    }

    fn ensure_wl_buffer(&mut self) -> Result<()> {
        if self.wl_buf.is_some() {
            return Ok(());
        }

        profile_scope!("dmabuf_setup");
        let frame = self.renderer.present()?;
        let params = ZwpLinuxBufferParamsV1::new(self.conn.alloc_id());
        self.dmabuf.inner.create_params(&mut self.conn, &params)?;
        let mod_hi = (frame.modifier >> 32) as u32;
        let mod_lo = frame.modifier as u32;
        params.add(
            &mut self.conn,
            frame.fd,
            0,
            frame.offset,
            frame.stride,
            mod_hi,
            mod_lo,
        )?;
        let buf = WlBuffer::new(self.conn.alloc_id());
        params.create_immed(
            &mut self.conn,
            &buf,
            self.width as i32,
            self.height as i32,
            frame.format,
            0,
        )?;
        params.destroy(&mut self.conn)?;
        self.wl_buf = Some(buf);

        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        tracing::info!("phase=startup initializing calloop event loop");

        let mut event_loop: EventLoop<Self> = EventLoop::try_new()?;
        let signal = event_loop.get_signal();
        let wayland_socket = self.conn.try_clone_socket()?;

        event_loop.handle().insert_source(
            Generic::new(wayland_socket, Interest::READ, Mode::Level),
            move |_, _, state| {
                tracing::trace!(phase = "wayland_readable", "Wayland fd readable");
                if let Err(err) = state.process_wayland_messages() {
                    tracing::error!(phase = "wayland_readable", error = %err, "fatal message processing failure");
                    state.fatal_error = Some(err);
                    signal.stop();
                }
                Ok(PostAction::Continue)
            },
        )?;

        while !self.toplevel.closed {
            #[cfg(feature = "profile")]
            puffin::GlobalProfiler::lock().new_frame();

            profile_scope!("event_loop");

            event_loop.dispatch(Some(Duration::from_millis(16)), self)?;

            if let Some(err) = self.fatal_error.take() {
                return Err(err);
            }

            if self.needs_repaint {
                self.render_once()?;
            }

            self.conn.flush()?;
        }

        tracing::info!("window closed");

        if let Some(buf) = self.wl_buf.take() {
            buf.destroy(&mut self.conn)?;
            self.conn.flush()?;
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    profile_function!();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    #[cfg(feature = "profile")]
    let _puffin_server = {
        puffin::set_scopes_on(true); // Tell puffin to start recording
        let server_addr = format!("127.0.0.1:{}", puffin_http::DEFAULT_PORT);

        // Keep the server handle alive for the whole process lifetime.
        match puffin_http::Server::new(&server_addr) {
            Ok(server) => {
                eprintln!(
                    "Puffin HTTP server running on {}. Connect with: puffin_viewer --url {}",
                    server_addr, server_addr
                );
                Some(server) // Keep the server alive by binding it to _puffin_server
            }
            Err(e) => {
                eprintln!("Failed to start Puffin server: {}", e);
                None
            }
        }
    };

    let mut launcher = LauncherState::new()?;
    launcher.run()
}
