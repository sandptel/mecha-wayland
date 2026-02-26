#![allow(unused_variables, unused_mut, dead_code)]

mod connection;
mod display;
mod object;
mod wire;

use connection::Connection;
use object::Object;
use std::io;
use tracing::{debug, info};

include!(concat!(env!("OUT_DIR"), "/wayland_protocol.rs"));

fn main() -> io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut conn = Connection::connect()?;

    let mut display = WlDisplay::new(1);
    let registry = WlRegistry::new(conn.alloc_id());
    let callback = WlCallback::new(conn.alloc_id());

    display.get_registry(&mut conn, &registry)?;
    display.sync(&mut conn, &callback)?;
    conn.flush()?;

    info!("waiting for events");

    let registry_id = registry.object_id();
    let callback_id = callback.object_id();

    loop {
        let (obj_id, opcode, body) = conn.recv_msg()?;
        debug!(obj_id, opcode, bytes = body.len(), "event received");
        match obj_id {
            1 => {
                debug!(obj_id, opcode, "dispatching to wl_display");
                display.dispatch(opcode, &body);
            }
            id if id == registry_id && opcode == wl_registry::event::GLOBAL => {
                let name = wire::read_u32(&body, 0);
                let (interface, next) = wire::read_string(&body, 4);
                let version = wire::read_u32(&body, next);
                info!(name, %interface, version, "wl_registry::global");
            }
            id if id == callback_id && opcode == wl_callback::event::DONE => {
                debug!(obj_id = id, "wl_callback::done, exiting loop");
                break;
            }
            _ => {
                debug!(obj_id, opcode, "unhandled event, ignoring");
            }
        }
    }

    info!("sync complete, all globals enumerated");
    Ok(())
}
