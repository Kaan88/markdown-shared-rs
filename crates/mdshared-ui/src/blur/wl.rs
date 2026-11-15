//! The Wayland side of [`super`]: KWin's `org_kde_kwin_blur_manager`, driven through winit's own
//! `wl_display`/`wl_surface` on a private event queue (winit's queue is never dispatched here),
//! as `shadow/wl.rs` does for the shadow.

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::ManuallyDrop;

use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_plasma::blur::client::{
    org_kde_kwin_blur::OrgKdeKwinBlur, org_kde_kwin_blur_manager::OrgKdeKwinBlurManager,
};

use super::offers_blur;

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(State: ignore WlSurface);
delegate_noop!(State: ignore OrgKdeKwinBlurManager);
delegate_noop!(State: ignore OrgKdeKwinBlur);

/// One window's connection to the blur protocol.
struct Entry {
    conn: Connection,
    // Keeps the queue the objects live on; never dispatched.
    _queue: EventQueue<State>,
    qh: QueueHandle<State>,
    manager: OrgKdeKwinBlurManager,
    surface: WlSurface,
    /// The blur object while the blur is on.
    blur: Option<OrgKdeKwinBlur>,
}

thread_local! {
    // By the address of the window's `wl_surface`. Entries are never dropped (as in the shadow:
    // an app drops its state after the event loop, when the `wl_display` is already freed, and
    // any request or queue destroy then touches freed memory). The compositor removes the blur
    // with the surface. Winit windows live on the event loop's thread.
    static ENTRIES: RefCell<HashMap<usize, ManuallyDrop<Entry>>> = RefCell::new(HashMap::new());
}

fn connect(
    display: *mut std::ffi::c_void,
    surface: *mut std::ffi::c_void,
) -> Option<ManuallyDrop<Entry>> {
    // SAFETY: winit's window (alive for this call) owns both pointers; the foreign backend
    // neither closes the display nor destroys the surface.
    let backend = unsafe { Backend::from_foreign_display(display.cast()) };
    let conn = Connection::from_backend(backend);
    let (globals, queue) = registry_queue_init::<State>(&conn).ok()?;
    let qh = queue.handle();
    let present = globals
        .contents()
        .with_list(|list| offers_blur(list.iter().map(|g| g.interface.as_str())));
    if !present {
        return None;
    }
    let manager: OrgKdeKwinBlurManager = globals.bind(&qh, 1..=1, ()).ok()?;
    // SAFETY: as above; the pointer is a live `wl_surface`.
    let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.cast()) }.ok()?;
    let surface = WlSurface::from_id(&conn, id).ok()?;
    Some(ManuallyDrop::new(Entry {
        conn,
        _queue: queue,
        qh,
        manager,
        surface,
        blur: None,
    }))
}

/// Whether the compositor offers the blur global. The connection is kept for [`set`], so the
/// registry round trip is made once per window.
pub fn available(display: *mut std::ffi::c_void, surface: *mut std::ffi::c_void) -> bool {
    let key = surface as usize;
    if ENTRIES.with(|e| e.borrow().contains_key(&key)) {
        return true;
    }
    match connect(display, surface) {
        Some(entry) => {
            ENTRIES.with(|e| e.borrow_mut().insert(key, entry));
            true
        }
        None => false,
    }
}

/// Turns the blur on or off for the surface. False when there is no blur protocol.
pub fn set(display: *mut std::ffi::c_void, surface: *mut std::ffi::c_void, on: bool) -> bool {
    if !available(display, surface) {
        return false;
    }
    ENTRIES.with(|entries| {
        let mut entries = entries.borrow_mut();
        let Some(e) = entries.get_mut(&(surface as usize)) else {
            return false;
        };
        match (on, e.blur.is_some()) {
            (true, false) => {
                let blur = e.manager.create(&e.surface, &e.qh, ());
                // No region: the whole surface.
                blur.set_region(None);
                blur.commit();
                e.blur = Some(blur);
            }
            (false, true) => {
                e.manager.unset(&e.surface);
                if let Some(blur) = e.blur.take() {
                    blur.release();
                }
            }
            _ => return true,
        }
        // Flushed, not waited for: the blur is applied with the surface's next commit.
        e.conn.flush().is_ok()
    })
}
