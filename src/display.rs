use tracing::error;

use crate::WlDisplay;
use crate::WlDisplayDeleteIdEvent;
use crate::WlDisplayErrorEvent;
use crate::WlDisplayHandler;

impl WlDisplayHandler for WlDisplay {
    fn on_error(&mut self, event: WlDisplayErrorEvent) {
        error!(
            object_id = event.object_id,
            code = event.code,
            message = %event.message,
            "wl_display error"
        );
    }

    fn on_delete_id(&mut self, event: WlDisplayDeleteIdEvent) {
        tracing::debug!(id = event.id, "wl_display::delete_id");
    }
}
