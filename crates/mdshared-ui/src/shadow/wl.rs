//! The Wayland side: KWin's `org_kde_kwin_shadow_manager`, driven through winit's own
//! `wl_display`/`wl_surface` on a private event queue (winit's queue is never dispatched here).

use std::mem::ManuallyDrop;
use std::os::fd::AsFd;

use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer::WlBuffer,
    wl_registry::WlRegistry,
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_plasma::shadow::client::{
    org_kde_kwin_shadow::OrgKdeKwinShadow, org_kde_kwin_shadow_manager::OrgKdeKwinShadowManager,
};
use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use winit::window::Window;

use super::{Tiles, render};

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
delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore WlBuffer);
delegate_noop!(State: ignore WlSurface);
delegate_noop!(State: ignore OrgKdeKwinShadowManager);
delegate_noop!(State: ignore OrgKdeKwinShadow);

pub enum Inner {
    // Never dropped (leaked, about 25 KB of memory the compositor frees when the surface or the
    // connection goes): an app usually drops its state after the event loop, when the
    // `wl_display` is already freed, and any request or queue destroy then touches freed memory
    // (seen as a segfault at exit). The compositor removes the shadow with the surface.
    Wayland(ManuallyDrop<Wayland>),
}

impl Inner {
    pub fn attach(window: &Window, scale: f64, active: bool) -> Option<Inner> {
        match window.display_handle().ok()?.as_raw() {
            RawDisplayHandle::Wayland(d) => {
                let RawWindowHandle::Wayland(w) = window.window_handle().ok()?.as_raw() else {
                    return None;
                };
                Wayland::attach(
                    d.display.as_ptr().cast(),
                    w.surface.as_ptr().cast(),
                    scale,
                    active,
                )
                .map(|w| Inner::Wayland(ManuallyDrop::new(w)))
            }
            _ => None,
        }
    }

    pub fn set(&mut self, scale: f64, active: bool) {
        match self {
            Inner::Wayland(w) => w.set(scale, active),
        }
    }

    pub fn bytes(&self) -> usize {
        match self {
            Inner::Wayland(w) => w.bytes,
        }
    }
}

pub struct Wayland {
    conn: Connection,
    // Kept alive: it owns the queue the objects below live on (never dispatched after attach).
    _queue: EventQueue<State>,
    qh: QueueHandle<State>,
    shm: WlShm,
    shadow: OrgKdeKwinShadow,
    buffers: Vec<WlBuffer>,
    pub bytes: usize,
}

impl Wayland {
    fn attach(
        display: *mut std::ffi::c_void,
        surface: *mut std::ffi::c_void,
        scale: f64,
        active: bool,
    ) -> Option<Wayland> {
        // SAFETY: winit's window (alive for this call) owns both pointers; the foreign backend
        // neither closes the display nor destroys the surface.
        let backend = unsafe { Backend::from_foreign_display(display.cast()) };
        let conn = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        // No such global (GNOME, wlroots): nothing to do.
        let manager: OrgKdeKwinShadowManager = globals.bind(&qh, 1..=2, ()).ok()?;
        let shm: WlShm = globals.bind(&qh, 1..=1, ()).ok()?;
        let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.cast()) }.ok()?;
        let surface = WlSurface::from_id(&conn, id).ok()?;
        let shadow = manager.create(&surface, &qh, ());
        let mut me = Wayland {
            conn,
            _queue: queue,
            qh,
            shm,
            shadow,
            buffers: Vec::new(),
            bytes: 0,
        };
        me.set(scale, active);
        // One round trip: protocol errors (none expected) surface here, not in winit's queue.
        me._queue.roundtrip(&mut State).ok()?;
        if manager.version() >= 2 {
            manager.destroy();
        }
        Some(me)
    }

    fn set(&mut self, scale: f64, active: bool) {
        let tiles = render(scale, active);
        let Some(new) = self.upload(&tiles) else {
            return;
        };
        let s = &self.shadow;
        s.attach_top(&new[0]);
        s.attach_top_right(&new[1]);
        s.attach_right(&new[2]);
        s.attach_bottom_right(&new[3]);
        s.attach_bottom(&new[4]);
        s.attach_bottom_left(&new[5]);
        s.attach_left(&new[6]);
        s.attach_top_left(&new[7]);
        let m = tiles.m as f64;
        s.set_top_offset(m);
        s.set_right_offset(m);
        s.set_bottom_offset(m);
        s.set_left_offset(m);
        s.commit();
        for old in std::mem::replace(&mut self.buffers, new) {
            old.destroy();
        }
        self.bytes = tiles.tiles.iter().map(|t| t.2.len()).sum();
        let _ = self.conn.flush();
    }

    /// The tiles in one memfd-backed pool, one buffer each. The pool is destroyed at once; the
    /// memory lives as long as its buffers.
    fn upload(&self, tiles: &Tiles) -> Option<Vec<WlBuffer>> {
        let total: usize = tiles.tiles.iter().map(|t| t.2.len()).sum();
        let fd = memfd_create("mdshared-shadow", MemfdFlags::CLOEXEC).ok()?;
        ftruncate(&fd, total as u64).ok()?;
        let mut all = Vec::with_capacity(total);
        for t in &tiles.tiles {
            all.extend_from_slice(&t.2);
        }
        let mut written = 0;
        while written < all.len() {
            written += rustix::io::write(&fd, &all[written..]).ok()?;
        }
        let pool = self.shm.create_pool(fd.as_fd(), total as i32, &self.qh, ());
        let mut offset = 0usize;
        let buffers = tiles
            .tiles
            .iter()
            .map(|(w, h, d)| {
                let b = pool.create_buffer(
                    offset as i32,
                    *w as i32,
                    *h as i32,
                    (*w * 4) as i32,
                    wl_shm::Format::Argb8888,
                    &self.qh,
                    (),
                );
                offset += d.len();
                b
            })
            .collect();
        pool.destroy();
        Some(buffers)
    }
}
