# Next release: linux-frost (branch `f/linux-frost`)

**This has never been run or seen on Linux. It has only been compiled** (the Wayland module in
isolation for `x86_64-unknown-linux-gnu`, with clippy `-D warnings`, and by CI on Linux; the whole
workspace cannot be cross-checked from Windows, see below). The Windows build, tests and behaviour
are unchanged.

## Built

Blur behind the window on KDE Plasma (KWin, Wayland) for the "Window backdrop" setting, with
`org_kde_kwin_blur` on the window's own Wayland connection, as the shadow does.

API (crate `mdshared-ui`):

- `blur::available(&winit::window::Window) -> bool`, `blur::set(&Window, on: bool) -> bool`: false
  (and nothing done) on Windows, X11, GNOME, wlroots, with the global missing, and with
  `MDSHARED_NO_BLUR` set.
- `frost::BlurSystem<'a>(pub &'a Window)`: a `frost::System`, for use in `Frost::sync` where
  `WindowSystem(hwnd)` is used on Windows. `WindowSystem` is unchanged.
- `Frost::wants_alpha() -> bool`: true while the chrome is frosted.
- `Backdrop::HELP`: unchanged on Windows; elsewhere "Windows 11, and KDE Plasma (Wayland): the
  blurred desktop behind the bars and the side pane. Does nothing on other desktops."

No new crates, Cargo.lock unchanged (`wayland-protocols-plasma` already provides `blur`).

## What each app must do (not nothing)

Not possible without app changes, because the pixels and the softbuffer surface are the app's:

1. Off Windows, pass `&mut frost::BlurSystem(&window)` to `Frost::sync` instead of
   `WindowSystem(None)`.
2. After `sync`, present with alpha:
   `surface.set_premultiplied_alpha(frost.wants_alpha() || shadow.wants_alpha())` (needs the
   patched softbuffer, `docs/patches/`; stock softbuffer makes opaque XRGB8888 buffers on Wayland
   and the blur would show nothing).
3. In the non-Windows frame copy, apply `frost::key_out` to the marked pixels and write all other
   pixels with alpha `0xFF` while `frost.key()` is `Some` (as on Windows).

`blur::set(on)` itself calls `Window::set_transparent(true)`; nothing else about window creation
needs to change.

## What was run

- Windows: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --release --workspace`: pass. New tests: the global-name match, the environment
  switch, Wayland pointer extraction from raw handles (and that Windows, Xlib and mixed handles
  give none), `Frost::wants_alpha`.
- Linux from Windows: `cargo check --target x86_64-unknown-linux-gnu` does not get through
  (fontconfig and X11 `pkg-config` need a sysroot). `blur.rs` and `blur/wl.rs` were instead
  compiled and linted (`clippy -D warnings`) in a scratch crate with the same pinned winit and
  wayland crates. CI on `f/linux-frost` (Linux and Windows): see the report.

## Not verified

Everything on a real compositor: that KWin accepts the request, that the blur shows, how it looks
with the chrome keyed out, the focus rule, turning it off, a second window, the stale-entry case
(a destroyed window's `wl_surface` address reused), and the behaviour of both apps. The protocol
objects were written from the `blur.xml` in `wayland-protocols-plasma` 0.3.12 only.

## Decisions

- Both Mica and Acrylic mean the same blur here; the existing veil values still apply.
- `set_transparent(true)` is never undone (no getter for the creation value; the shadow's client
  route needs it).
- Entries are leaked (as the shadow's), keyed by the `wl_surface` address in a thread-local map.
