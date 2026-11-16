//! Files the system asks the running application to open: on macOS a double-click in Finder,
//! "Open With", a drop on the Dock icon, or `open -a` reach a bundle as an Apple Event
//! (`kAEOpenDocuments`), not as arguments, and winit 0.30 does not pass that event on. Its
//! handler is installed here, before the event loop runs, so the event that launched the
//! application is caught too. Elsewhere there is no such event and [`on_open`] does nothing.

use std::path::PathBuf;

/// Calls `open` with the files each time the system asks this application to open some. Call it
/// once, after the event loop is made and before it runs; `open` runs on the main thread, so it
/// should hand the paths to the loop (an `EventLoopProxy`) rather than open them itself.
pub fn on_open(open: impl Fn(Vec<PathBuf>) + Send + 'static) {
    imp::install(Box::new(open));
}

#[cfg(target_os = "macos")]
mod imp {
    use super::PathBuf;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{AllocAnyThread, define_class, msg_send, sel};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};
    use std::sync::Mutex;

    type Open = Box<dyn Fn(Vec<PathBuf>) + Send>;
    static OPEN: Mutex<Option<Open>> = Mutex::new(None);

    /// `'aevt'`, `'odoc'` and `'----'` (the direct object: the list of files).
    const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
    const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "MdsharedOpenDocuments"]
        struct Handler;

        impl Handler {
            #[unsafe(method(handleOpen:withReply:))]
            fn handle_open(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
                let paths = files_of(event);
                if paths.is_empty() {
                    return;
                }
                if let Some(open) = OPEN.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    open(paths);
                }
            }
        }
    );

    /// The files an open-documents event names: its direct object is a list of file URLs, or one.
    fn files_of(event: &NSAppleEventDescriptor) -> Vec<PathBuf> {
        let Some(list) = event.paramDescriptorForKeyword(DIRECT_OBJECT) else {
            return Vec::new();
        };
        let path = |d: &NSAppleEventDescriptor| {
            d.fileURLValue()
                .and_then(|url| url.path())
                .map(|p| PathBuf::from(p.to_string()))
        };
        let n = list.numberOfItems();
        if n <= 0 {
            return path(&list).into_iter().collect();
        }
        (1..=n)
            .filter_map(|i| list.descriptorAtIndex(i))
            .filter_map(|d| path(&d))
            .collect()
    }

    pub(super) fn install(open: Open) {
        *OPEN.lock().unwrap_or_else(|e| e.into_inner()) = Some(open);
        // SAFETY: a plain NSObject subclass without instance variables, made with `init`.
        let handler: Retained<Handler> = unsafe { msg_send![Handler::alloc(), init] };
        let manager = NSAppleEventManager::sharedAppleEventManager();
        // SAFETY: the selector is the handler's own method, of the signature the manager calls;
        // the manager does not retain its handler, so the handler is kept for the process's life.
        unsafe {
            manager.setEventHandler_andSelector_forEventClass_andEventID(
                &handler as &AnyObject,
                sel!(handleOpen:withReply:),
                CORE_EVENT_CLASS,
                OPEN_DOCUMENTS,
            );
        }
        std::mem::forget(handler);
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub(super) fn install(_open: Box<dyn Fn(Vec<super::PathBuf>) + Send>) {}
}
