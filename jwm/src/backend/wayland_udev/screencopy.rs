/// wlr-screencopy-unstable-v1 protocol implementation for JWM.
///
/// This allows clients like `grim` to request screen content from the compositor.
/// The compositor captures the framebuffer during the render loop and copies the data
/// into the client-provided wl_shm buffer.

use std::sync::{Arc, Mutex};

use log::{debug, info, warn};

use smithay::output::Output;
use smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::protocol::wl_shm;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};

// IMPORTANT: Use the canonical path that matches Display<JwmWaylandState> in backend.rs.
// Do NOT use `super::state::JwmWaylandState` — that's a different type due to the #[path]
// attribute in wayland.rs re-exporting the same file under a different module path.
use crate::backend::wayland::state::JwmWaylandState;

// ---- Shared pending-copy queue ---------------------------------------------------

/// A screencopy frame waiting for the compositor to capture pixels.
pub struct PendingScreencopyFrame {
    /// The `zwlr_screencopy_frame_v1` resource to send events on.
    pub frame: ZwlrScreencopyFrameV1,
    /// The client's wl_buffer to copy pixels into.
    pub buffer: WlBuffer,
    /// The smithay `Output` to capture.
    pub output: Output,
    /// Optional sub-region (x, y, width, height) in output logical coords.
    pub region: Option<(i32, i32, i32, i32)>,
    /// Whether to composite the cursor onto the frame.
    pub overlay_cursor: bool,
}

// PendingScreencopyFrame contains Wayland protocol objects which are !Send.
// JWM runs everything on the main thread so this is fine.
unsafe impl Send for PendingScreencopyFrame {}

pub type PendingScreencopyQueue = Arc<Mutex<Vec<PendingScreencopyFrame>>>;

pub fn new_pending_screencopy_queue() -> PendingScreencopyQueue {
    Arc::new(Mutex::new(Vec::new()))
}

// ---- Per-frame user data ---------------------------------------------------------

/// User data stored per `zwlr_screencopy_frame_v1` object.
pub struct ScreencopyFrameData {
    pub output: Output,
    pub region: Option<(i32, i32, i32, i32)>,
    pub overlay_cursor: bool,
    pub buffer_info: (u32, u32, u32, wl_shm::Format), // (width, height, stride, format)
    pub pending_queue: PendingScreencopyQueue,
}

// ---- Manager global (zwlr_screencopy_manager_v1) ----------------------------------

// Use () as global data - access pending_queue from JwmWaylandState.screencopy_pending
impl GlobalDispatch<ZwlrScreencopyManagerV1, ()> for JwmWaylandState {
    fn bind(
        _state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrScreencopyManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for JwmWaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrScreencopyManagerV1,
        request: zwlr_screencopy_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_manager_v1::Request::CaptureOutput {
                frame: frame_new_id,
                overlay_cursor,
                output: wl_output,
            } => {
                handle_capture(state, data_init, frame_new_id, overlay_cursor, wl_output, None);
            }
            zwlr_screencopy_manager_v1::Request::CaptureOutputRegion {
                frame: frame_new_id,
                overlay_cursor,
                output: wl_output,
                x,
                y,
                width,
                height,
            } => {
                handle_capture(
                    state,
                    data_init,
                    frame_new_id,
                    overlay_cursor,
                    wl_output,
                    Some((x, y, width, height)),
                );
            }
            zwlr_screencopy_manager_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

fn handle_capture(
    state: &mut JwmWaylandState,
    data_init: &mut DataInit<'_, JwmWaylandState>,
    frame_new_id: New<ZwlrScreencopyFrameV1>,
    overlay_cursor: i32,
    wl_output: WlOutput,
    region: Option<(i32, i32, i32, i32)>,
) {
    // Get pending queue from state
    let pending_queue = match state.screencopy_pending.as_ref() {
        Some(q) => q.clone(),
        None => {
            warn!("[screencopy] no pending queue available");
            return;
        }
    };

    // Find the smithay Output that matches this wl_output.
    let output = Output::from_resource(&wl_output);

    let output = match output {
        Some(o) => o,
        None => {
            // No matching output → create the frame but immediately fail it.
            warn!("[screencopy] no matching output for wl_output {:?}", wl_output.id());
            let frame_data = ScreencopyFrameData {
                output: state.outputs.first().unwrap().clone(), // dummy
                region,
                overlay_cursor: overlay_cursor != 0,
                buffer_info: (0, 0, 0, wl_shm::Format::Argb8888),
                pending_queue: pending_queue.clone(),
            };
            let frame = data_init.init(frame_new_id, frame_data);
            frame.failed();
            return;
        }
    };

    // Determine output dimensions.
    let mode = output.current_mode().unwrap();
    let (out_w, out_h) = (mode.size.w as u32, mode.size.h as u32);

    // For region captures, use the region size; otherwise full output.
    let (cap_w, cap_h) = if let Some((_, _, rw, rh)) = region {
        (rw as u32, rh as u32)
    } else {
        (out_w, out_h)
    };

    let stride = cap_w * 4; // ARGB8888 → 4 bytes per pixel

    let frame_data = ScreencopyFrameData {
        output: output.clone(),
        region,
        overlay_cursor: overlay_cursor != 0,
        buffer_info: (cap_w, cap_h, stride, wl_shm::Format::Argb8888),
        pending_queue,
    };

    let frame = data_init.init(frame_new_id, frame_data);

    // Send buffer info to the client.
    frame.buffer(wl_shm::Format::Argb8888, cap_w, cap_h, stride);

    // Signal that all buffer types have been enumerated (v3).
    if frame.version() >= 3 {
        frame.buffer_done();
    }

    debug!(
        "[screencopy] capture_output: output={} size={}x{} region={:?}",
        output.name(),
        cap_w,
        cap_h,
        region,
    );
}

// ---- Frame dispatch (zwlr_screencopy_frame_v1) -----------------------------------

impl Dispatch<ZwlrScreencopyFrameV1, ScreencopyFrameData> for JwmWaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZwlrScreencopyFrameV1,
        request: zwlr_screencopy_frame_v1::Request,
        data: &ScreencopyFrameData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_screencopy_frame_v1::Request::Copy { buffer } => {
                queue_copy(resource, &buffer, data, false);
                state.needs_redraw = true;
            }
            zwlr_screencopy_frame_v1::Request::CopyWithDamage { buffer } => {
                queue_copy(resource, &buffer, data, true);
                state.needs_redraw = true;
            }
            zwlr_screencopy_frame_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

fn queue_copy(
    frame: &ZwlrScreencopyFrameV1,
    buffer: &WlBuffer,
    data: &ScreencopyFrameData,
    _with_damage: bool,
) {
    debug!("[screencopy] copy request queued for output {}", data.output.name());
    let mut queue = data.pending_queue.lock().unwrap();
    queue.push(PendingScreencopyFrame {
        frame: frame.clone(),
        buffer: buffer.clone(),
        output: data.output.clone(),
        region: data.region,
        overlay_cursor: data.overlay_cursor,
    });
}

// ---- Initialization ---------------------------------------------------------------

/// Create the zwlr_screencopy_manager_v1 global and return the shared pending queue.
pub fn init_screencopy_manager(
    dh: &DisplayHandle,
) -> PendingScreencopyQueue {
    let queue = new_pending_screencopy_queue();
    // Version 3 – includes buffer_done, linux_dmabuf, copy_with_damage.
    dh.create_global::<JwmWaylandState, ZwlrScreencopyManagerV1, _>(3, ());
    info!("[screencopy] zwlr_screencopy_manager_v1 global created (v3)");
    queue
}
