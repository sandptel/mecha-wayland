#![allow(unused_variables, unused_mut, dead_code)]

pub mod wl_callback;
pub mod wl_display;
pub mod wl_registry;
pub mod wl_shm;
pub mod xdg_surface;
pub mod xdg_toplevel;
pub mod xdg_wm_base;

pub mod connection;
pub mod object;
pub mod wire;

include!(concat!(env!("OUT_DIR"), "/wayland_protocol.rs"));

/// Route a received Wayland event to the matching handler by object_id.
///
/// Usage: `dispatch_to!(conn, obj_id, opcode, &body; handler_a, handler_b, ...);`
/// Each handler must implement `.object_id()` (via `Object`) and `.dispatch(...)`.
#[macro_export]
macro_rules! dispatch_to {
    ($conn:expr, $obj_id:expr, $opcode:expr, $body:expr; $($handler:expr),+ $(,)?) => {{
        use $crate::object::Object as _;
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
