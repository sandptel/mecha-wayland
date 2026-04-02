use crate::event_manager::{EventImpl, EventSystem};
use crate::object::Object;
use crate::{WlPointer, WlPointerHandler};
pub struct Pointer {
    pub inner: WlPointer,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Copy)]
pub enum PointerEvent {
    OnEnter {
        x: f64,
        y: f64,
    },
    OnLeave {
        x: f64,
        y: f64,
    },
    OnMotion {
        x: f64,
        y: f64,
    },
    OnButton {
        x: f64,
        y: f64,
        button: u32,
        state: u32,
    },
    OnAxis {
        x: f64,
        y: f64,
        axis: u32,
        value: i32,
    },
}

impl EventImpl for PointerEvent {}

impl Pointer {
    pub fn new(object_id: u32) -> Self {
        Pointer {
            inner: WlPointer::new(object_id),
            x: 0.0,
            y: 0.0,
        }
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

        let pointer_event = PointerEvent::OnEnter {
            x: self.x,
            y: self.y,
        };
        EventSystem::emit(pointer_event);

        tracing::debug!(x = self.x, y = self.y, "mouse entered window");
    }

    fn on_leave(&mut self, _event: crate::WlPointerLeaveEvent) {
        EventSystem::emit(PointerEvent::OnLeave {
            x: self.x,
            y: self.y,
        });
        tracing::debug!("mouse left window");
    }

    fn on_motion(&mut self, event: crate::WlPointerMotionEvent) {
        self.x = event.surface_x as f64 / 256.0;
        self.y = event.surface_y as f64 / 256.0;

        EventSystem::emit(PointerEvent::OnMotion {
            x: self.x,
            y: self.y,
        });

        tracing::trace!(x = self.x, y = self.y, "mouse motion");
    }

    fn on_button(&mut self, event: crate::WlPointerButtonEvent) {
        EventSystem::emit(PointerEvent::OnButton {
            x: self.x,
            y: self.y,
            button: event.button,
            state: event.state,
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
        EventSystem::emit(PointerEvent::OnAxis {
            x: self.x,
            y: self.y,
            axis: event.axis,
            value: event.value,
        });

        tracing::trace!(axis = event.axis, value = event.value, "mouse wheel scroll");
    }
}
