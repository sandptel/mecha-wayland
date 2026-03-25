use crate::object::Object;
use crate::{WlPointer, WlPointerHandler};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawPointerEvent {
    Enter { x: f64, y: f64 },
    Leave,
    Motion { x: f64, y: f64 },
    Button { button: u32, state: u32, x: f64, y: f64 },
}

pub struct Pointer {
    pub inner: WlPointer,
    pub x: f64,
    pub y: f64,
    pending_events: Vec<RawPointerEvent>,
}

impl Pointer {
    pub fn new(object_id: u32) -> Self {
        Pointer {
            inner: WlPointer::new(object_id),
            x: 0.0,
            y: 0.0,
            pending_events: Vec::new(),
        }
    }

    pub fn take_pending_events(&mut self) -> Vec<RawPointerEvent> {
        core::mem::take(&mut self.pending_events)
    }
}

impl Object for Pointer {
    fn object_id(&self) -> u32 {
        self.inner.object_id()
    }
}

impl WlPointerHandler for Pointer {
    fn on_enter(&mut self, event: crate::WlPointerEnterEvent) {
        self.x = event.surface_x as f64 / 256.0;
        self.y = event.surface_y as f64 / 256.0;
        self.pending_events.push(RawPointerEvent::Enter {
            x: self.x,
            y: self.y,
        });
        tracing::debug!(x = self.x, y = self.y, "mouse entered window");
    }

    fn on_leave(&mut self, event: crate::WlPointerLeaveEvent) {
        self.pending_events.push(RawPointerEvent::Leave);
        tracing::debug!("mouse left window");
    }

    fn on_motion(&mut self, event: crate::WlPointerMotionEvent) {
        self.x = event.surface_x as f64 / 256.0;
        self.y = event.surface_y as f64 / 256.0;
        self.pending_events.push(RawPointerEvent::Motion {
            x: self.x,
            y: self.y,
        });
        tracing::trace!(x = self.x, y = self.y, "mouse motion");
    }

    fn on_button(&mut self, event: crate::WlPointerButtonEvent) {
        self.pending_events.push(RawPointerEvent::Button {
            button: event.button,
            state: event.state,
            x: self.x,
            y: self.y,
        });
        tracing::debug!(
            button = event.button,
            state = event.state,
            x = self.x,
            y = self.y,
            "mouse button"
        );
    }

    fn on_axis(&mut self, event: crate::WlPointerAxisEvent) {
        tracing::trace!(axis = event.axis, value = event.value, "mouse wheel scroll");
    }
}
