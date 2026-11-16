//! The menu bar a Mac application is expected to have, in place of the one winit makes. winit's
//! Quit (Command+Q) ends the process at once through `terminate:`, so a document with unsaved
//! changes would be lost without a question; here Quit, the Dock's Quit (the `kAEQuitApplication`
//! event) and logging out reach the application as [`AppCommand::Quit`], which it answers as its
//! own Exit command. About and Settings go to the application too. Hide, Services, the Edit menu
//! (which also makes Command+C and the like work in the system's file panels) and the Window menu
//! are the system's own.
//!
//! The Edit items have no target: they act on the first responder, and winit's view answers none
//! of them, so over the window they are disabled and the keys reach the window as key events.
//! Elsewhere [`install`] does nothing.

/// A menu bar item the application answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppCommand {
    About,
    Settings,
    Quit,
}

/// Sets the menu bar for the application `name` and sends what is chosen in it to `command`
/// (which should hand it to the event loop). Call it on the main thread once the event loop runs
/// (in `resumed`), after winit has set its own.
pub fn install(name: &str, command: impl Fn(AppCommand) + Send + 'static) {
    imp::install(name, Box::new(command));
}

#[cfg(target_os = "macos")]
mod imp {
    use super::AppCommand;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, Sel};
    use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSString};
    use std::sync::Mutex;

    type Command = Box<dyn Fn(AppCommand) + Send>;
    static COMMAND: Mutex<Option<Command>> = Mutex::new(None);

    fn send(c: AppCommand) {
        if let Some(command) = COMMAND.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            command(c);
        }
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "MdsharedMenuTarget"]
        struct Target;

        impl Target {
            #[unsafe(method(mdAbout:))]
            fn about(&self, _sender: Option<&AnyObject>) {
                send(AppCommand::About);
            }

            #[unsafe(method(mdSettings:))]
            fn settings(&self, _sender: Option<&AnyObject>) {
                send(AppCommand::Settings);
            }

            #[unsafe(method(mdQuit:))]
            fn quit(&self, _sender: Option<&AnyObject>) {
                send(AppCommand::Quit);
            }

            #[unsafe(method(handleQuit:withReply:))]
            fn handle_quit(&self, _event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
                send(AppCommand::Quit);
            }
        }
    );

    const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
    const QUIT_APPLICATION: u32 = u32::from_be_bytes(*b"quit");

    /// An item; `key` is its key equivalent (lower case: Command and `extra`), `target` the object
    /// its action goes to (none: the first responder).
    fn item(
        mtm: MainThreadMarker,
        title: &str,
        action: Option<Sel>,
        key: &str,
        extra: NSEventModifierFlags,
        target: Option<&AnyObject>,
    ) -> Retained<NSMenuItem> {
        // SAFETY: the selectors are the standard responder actions or the target's own methods.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                action,
                &NSString::from_str(key),
            )
        };
        if !key.is_empty() {
            item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command | extra);
        }
        // SAFETY: the target lives for the process (it is never released).
        unsafe { item.setTarget(target) };
        item
    }

    fn menu(
        mtm: MainThreadMarker,
        title: &str,
        items: &[Retained<NSMenuItem>],
    ) -> Retained<NSMenu> {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
        for i in items {
            menu.addItem(i);
        }
        menu
    }

    fn submenu_item(mtm: MainThreadMarker, sub: &NSMenu) -> Retained<NSMenuItem> {
        let item = NSMenuItem::new(mtm);
        item.setSubmenu(Some(sub));
        item
    }

    pub(super) fn install(name: &str, command: Command) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        *COMMAND.lock().unwrap_or_else(|e| e.into_inner()) = Some(command);
        // SAFETY: a plain NSObject subclass without instance variables, made with `init`; kept
        // for the process's life, as menu items and the event manager do not retain targets.
        let target: Retained<Target> = unsafe { msg_send![Target::alloc(), init] };
        let t: &AnyObject = &target;
        let none = NSEventModifierFlags::empty();
        let sep = || NSMenuItem::separatorItem(mtm);

        let services = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Services"));
        let app_menu = menu(
            mtm,
            name,
            &[
                item(
                    mtm,
                    &format!("About {name}"),
                    Some(sel!(mdAbout:)),
                    "",
                    none,
                    Some(t),
                ),
                sep(),
                item(
                    mtm,
                    "Settings\u{2026}",
                    Some(sel!(mdSettings:)),
                    ",",
                    none,
                    Some(t),
                ),
                sep(),
                {
                    let i = item(mtm, "Services", None, "", none, None);
                    i.setSubmenu(Some(&services));
                    i
                },
                sep(),
                item(
                    mtm,
                    &format!("Hide {name}"),
                    Some(sel!(hide:)),
                    "h",
                    none,
                    None,
                ),
                item(
                    mtm,
                    "Hide Others",
                    Some(sel!(hideOtherApplications:)),
                    "h",
                    NSEventModifierFlags::Option,
                    None,
                ),
                item(
                    mtm,
                    "Show All",
                    Some(sel!(unhideAllApplications:)),
                    "",
                    none,
                    None,
                ),
                sep(),
                item(
                    mtm,
                    &format!("Quit {name}"),
                    Some(sel!(mdQuit:)),
                    "q",
                    none,
                    Some(t),
                ),
            ],
        );
        let edit = menu(
            mtm,
            "Edit",
            &[
                item(mtm, "Undo", Some(sel!(undo:)), "z", none, None),
                item(
                    mtm,
                    "Redo",
                    Some(sel!(redo:)),
                    "z",
                    NSEventModifierFlags::Shift,
                    None,
                ),
                sep(),
                item(mtm, "Cut", Some(sel!(cut:)), "x", none, None),
                item(mtm, "Copy", Some(sel!(copy:)), "c", none, None),
                item(mtm, "Paste", Some(sel!(paste:)), "v", none, None),
                item(mtm, "Select All", Some(sel!(selectAll:)), "a", none, None),
            ],
        );
        let window = menu(
            mtm,
            "Window",
            &[
                item(
                    mtm,
                    "Minimize",
                    Some(sel!(performMiniaturize:)),
                    "m",
                    none,
                    None,
                ),
                item(mtm, "Zoom", Some(sel!(performZoom:)), "", none, None),
                sep(),
                item(
                    mtm,
                    "Bring All to Front",
                    Some(sel!(arrangeInFront:)),
                    "",
                    none,
                    None,
                ),
            ],
        );
        let bar = NSMenu::new(mtm);
        for sub in [&app_menu, &edit, &window] {
            bar.addItem(&submenu_item(mtm, sub));
        }
        let app = NSApplication::sharedApplication(mtm);
        app.setMainMenu(Some(&bar));
        app.setServicesMenu(Some(&services));
        app.setWindowsMenu(Some(&window));

        let manager = NSAppleEventManager::sharedAppleEventManager();
        // SAFETY: the selector is the target's own method, of the signature the manager calls.
        unsafe {
            manager.setEventHandler_andSelector_forEventClass_andEventID(
                t,
                sel!(handleQuit:withReply:),
                CORE_EVENT_CLASS,
                QUIT_APPLICATION,
            );
        }
        std::mem::forget(target);
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub(super) fn install(_name: &str, _command: Box<dyn Fn(super::AppCommand) + Send>) {}
}
