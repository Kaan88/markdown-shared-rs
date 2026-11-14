# Linux (and other non-Windows) support in the shared crates

Written 2026-11-14 on branch `feat/linux`. Everything here is under `cfg(not(windows))` (or
`target_os = "linux"`); the Windows paths are unchanged. Nothing below has been built or run for
Windows from this branch. Everything marked "run" was run on one machine: CachyOS, KDE Plasma on
Wayland, cargo 1.99.

## What is implemented

| Area | Off Windows | How it was checked |
|---|---|---|
| Downloads (`mdshared-fetch`) | The system libcurl, loaded at run time with `dlopen` (`libcurl.so.4`, then `libcurl.so`; the `libloading` crate). No link-time dependency: without libcurl every download fails as before (`Error::Unsupported`, seen by callers as `State::Failed`). Same contract as WinHTTP: no redirects followed, no cookies/netrc/auth/`Accept-Encoding`, http and https only, TLS verification on, size cap from `Content-Length` and while reading, overall deadline. | The network tests (local TCP server) run on unix; `live_https_badge` (`--ignored`) fetched an https badge. |
| File dialogs (`platform`) | `pick_files_async`, `pick_folder_async`, `pick_save_async`: the blocking dialog on a helper thread; one at a time (a second request calls `done` at once with the empty result and returns false). The blocking `pick_files`/`pick_folder` stay for Windows. | Unit test of the one-at-a-time rule; `examples/dialog.rs` run: a portal request was open on the bus while it waited. |
| Self-update (`mdshared-app`) | Only inside an AppImage: `updater::installed_files` returns the canonical `$APPIMAGE` when it is an existing file in a writable folder, with no engine. A Linux release lists no `engine.*` keys. The staged file gets mode 0755. `.new`/`.old` sit beside the AppImage. A plain Linux binary updates nothing. | Unit tests. |
| Window identity (`window::identify`) | Wayland app id and X11 `WM_CLASS` from the app name; the X11 window icon from PNG bytes. | Compiles; not run in a window. |
| Desktop entry (`desktop_entry::ensure`) | From an AppImage: writes `applications/<name>.desktop` and the 256 px icon under `$XDG_DATA_HOME` (or `~/.local/share`) when missing or when the recorded `X-AppImage-Path` differs; never touches an entry without the marker; sets no defaults. | Unit tests with a temp data directory. |
| Fonts (`mdshared-text`) | `add_fallbacks` (applied by `shared_fonts()`): Windows names first, then Noto/Twemoji/JoyPixels/Symbola, Noto Symbols/Math, DejaVu Sans, and the CJK families, behind the system's fallback for Han, Common, Inherited and Latin. `EMOJI_FAMILIES` for family strings (see below). | `examples/fontprobe.rs` rendered to PNG and looked at. |
| Hand-off between the apps (`sister`, `single::forward_request`, `place`) | Finding the other app (`$APPIMAGE` neighbours and the `X-AppImage-Path` of its desktop entry among the rules), starting it detached, and sending it a line, a follow update or a window rectangle over the single-instance port. Window placement: winit on X11; on Wayland under KDE a KWin script loaded through `org.kde.KWin` `/Scripting` on a helper thread; false on other Wayland desktops. `docs/SISTER.md` is the protocol. | Unit tests; run on KDE Wayland: the KWin move (`examples/place.rs`, seen with a KWin listing script, left and right half, clamped to the panel), the message between two real processes, a spawn of a recording script. X11, GNOME, Plasma 5 not run. |
| System theme (`platform::system_is_dark`) | `org.freedesktop.appearance` `color-scheme` from the settings portal (zbus, blocking call on a helper thread, 500 ms limit, cached for 1 s). False on any failure. | Run: about 1.6 ms, dark, matching `gdbus` and KDE's BreezeDark. |

The emoji box: a cluster flagged as emoji is looked up in the style's family list first and in the
`emoji` generic only afterwards, and the first font with the character wins. The editor's list ends
in `monospace`, which on this machine includes Adwaita Mono, whose monochrome U+1F642 is a tall
pill with a face (glyph id 7598, not `.notdef`): that is the "box". Fix in the family string: name
the colour emoji fonts before the generic (`"..., DejaVu Sans Mono, Noto Color Emoji, monospace"`).
The viewer's `sans-serif` has the same flaw (DejaVu Sans has a monochrome U+1F600).

## What is not implemented or not verified

- Windows build of this branch (a later stage cross-compiles).
- Any run in a real application window on Linux: the window identity helper, the desktop entry
  written from a real AppImage, and the dialogs beside a window (they have no parent window, so
  they are not modal to it).
- GNOME, other portal backends, X11 (xdg-desktop-portal-gtk was running; KDE's portal answered).
- A theme-changed event: `system_is_dark` is read when called; nothing listens for the portal's
  `SettingChanged` signal.
- Packaging (building the AppImage), `update-desktop-database`, MIME registration beyond the
  desktop entry's `MimeType=`, HiDPI scale (`system_scale` is 1.0), double-click and caret-blink
  settings (fixed values), subpixel order (RGB), `reveal_in_file_manager` (opens the folder).
- Non-x86_64 Linux: the manifest only has `linux-x86_64`.

## File associations

`desktop_entry::ensure` writes `MimeType=` (semicolon-terminated) and `Exec=... %F`, then runs
`update-desktop-database <applications dir>` when that program exists (absence and failure ignored),
so the app shows under "Open with" without a logout. It takes no defaults. `make_default(name,
mime_types)` and `is_default(name, mime_types)` edit and read `[Default Applications]` in
`$XDG_CONFIG_HOME` or `~/.config` `/mimeapps.list`; the apps call them from a menu command.
`EDITOR_MIME_TYPES` and `VIEWER_MIME_TYPES` are the shared lists. Run: unit tests against a temp
directory only; `update-desktop-database` was not checked to take effect in a desktop session.

## Window shadow (`mdshared_ui::shadow`)

An undecorated window gets no shadow from the compositor on Linux. `Shadow::attach(&window)` is
the one entry point and picks a route (`MD_SHADOW=client|kwin|none` forces one,
`MDSHARED_NO_SHADOW=1` turns it off):

| Route | Where | How | The app paints? |
|---|---|---|---|
| **client** (Wayland) | every compositor, with the patched winit | the window is the inner rectangle of a larger transparent surface (`xdg_surface.set_window_geometry`), the shadow lives in the margin | yes, `Shadow::paint_frame` |
| **client-x11** | X11 and Xwayland, feature on | ARGB window plus `_GTK_FRAME_EXTENTS` and an input shape (x11rb, a second connection; no winit change) | yes, same call |
| **kwin** | KDE Wayland | KWin's `org_kde_kwin_shadow_manager`: eight tiles handed over once (about 21 KB, 36 px reach, sigma 12, dropped 6 px; peak alpha 0.35, 0.20 inactive) | no |
| none | anything else | `attach` returns `None` | |

**Policy (`Mode::Auto`, `shadow::decide`, a tested table).** On Wayland where the compositor offers
KWin's shadow protocol (KDE) Auto uses it and the window is created exactly as without a shadow:
not transparent, no margin, no size growth. The client margin is used only where the protocol is
missing (GNOME, sway, other compositors) and the feature is built. On X11 Auto gives no shadow (a
tiled X11 window would draw its shadow over its neighbour: winit cannot report tiling there).
`MD_SHADOW=client|kwin|none` force a route wherever it can work (`client` also on X11 and on KDE).
`Shadow::prepare` runs before the window exists, so it asks the event loop's display whether the
protocol is there (one registry round trip on a private queue, cached for `attach`): measured
0.07 to 0.5 ms (first call in a process; 0.07-0.09 ms typical), release build, KDE. The client
routes exist only with the cargo feature `client-margin`; without it the crate builds against
stock winit and Auto is KWin-or-nothing, as before.

### Why a patched winit (and softbuffer)

winit creates the `xdg_surface` privately and itself calls `set_window_geometry(0, 0, w, h)` on
every resize, so an app cannot say "my window is an inner rectangle of the surface". The fork
`github.com/Kaan88/winit`, branch `md-0.30.13` (version still 0.30.13; see `PATCH.md` there) adds
`WindowExtWayland::set_content_margin(u32)` and `content_margin_now() -> u32`. Behaviour is
unchanged unless `set_content_margin` is called with a margin above 0, and nothing happens while
winit's own client-side frame (sctk-adwaita) is shown.

softbuffer 0.4.8 on Wayland always creates its buffers as `wl_shm` XRGB8888: the alpha byte is
ignored, so the margin comes out opaque black (run: `MD_ALPHA=0` in the example shows exactly
that). softbuffer's unreleased master has `AlphaMode`, but a different API (`Pixel`,
`buffer.pixels()`), so the apps would have to be ported; the small patch
`docs/patches/softbuffer-0.4.8-wayland-alpha.patch` (about 55 changed lines in 5 files) adds
`Surface::set_premultiplied_alpha(bool)` instead (Wayland: ARGB8888 buffers; everywhere else a
no-op returning false; X11 with an ARGB window already honours alpha). It is published as
`github.com/Kaan88/softbuffer`, branch `md-0.4.8`, rev `dd18fec9d08e2f466c42d4c2cece62aa0ee1eba9`
(the patch on the v0.4.8 tag, crate version unchanged).

### Enabling it in an app

In the app's workspace root, next to the other overrides:

```toml
[patch.crates-io]
winit = { git = "https://github.com/Kaan88/winit", rev = "<sha of md-0.30.13>" }
softbuffer = { git = "https://github.com/Kaan88/softbuffer", rev = "dd18fec9d08e2f466c42d4c2cece62aa0ee1eba9" }
```

and in the app's `Cargo.toml`: `mdshared-ui = { ..., features = ["client-margin"] }`. Both
together: the feature calls winit functions that stock winit does not have. A build without the
patches leaves the feature and the `[patch]` out; the same source then falls back to KWin.

### The app-side checklist

1. **Transparent window.** `Shadow::prepare(attributes, Mode::from_env(), event_loop)`: sets
   `with_transparent(true)` when a client route may be used and, on Wayland, grows a logical
   `with_inner_size` (the CONTENT size you want) by the margin, because there the initial size is
   a surface size. Do not pass a min size through the attributes; see 7.
2. **Attach** once after `create_window`: `let mut shadow = Shadow::attach_mode(&window, mode);`;
   keep it for the window's life. Then, if `shadow.wants_alpha()`:
   `surface.set_premultiplied_alpha(true)` on the softbuffer surface (patched softbuffer), and
   write **content pixels with alpha 0xFF** (a `0x00RRGGBB` pixel is transparent from then on: an
   invisible window if you forget).
3. **Two sizes.** `window.inner_size()` and `WindowEvent::Resized` report the SURFACE: allocate
   the softbuffer buffer from them. The content is `shadow.content_size(sw, sh)` (surface minus
   `2 * margin`, device px): render the content at that size into its own bitmap and copy it to
   `shadow.content_origin()` (`(margin, margin)`) in the buffer, row by row. `shadow.margin()` is
   0 while maximized, fullscreen or tiled: the content is the whole surface then and nothing is
   painted. `surface_size` is the inverse of `content_size`.
4. **Keep the shadow in step.** On `Resized`, `ScaleFactorChanged` and `Focused` call
   `shadow.update(&window, scale, active)`; it re-reads the margin in force from winit (a
   maximize, restore or tile arrives as a `Resized`) and returns true when what you draw
   changed: redraw then.
5. **Paint** in the redraw, after the content copy: `shadow.paint_frame(&mut buffer, sw, sh,
   buffer.age())`. It fills only the margin and touches no content pixel. The margin does not
   change between frames at one size, so a normal frame writes none of it: softbuffer keeps two
   buffers and resizes each lazily, so after a change of size, margin, scale or focus both are
   repainted once (two frames); a buffer of age 0 is repainted at once. Cost, release build,
   scale 1: `paint` 10 us at 672x472 and 45 us at 3440x1440 (1.35 MB written), 113 us at 3840x2160
   scale 2; `Painter::new` (tiles, on a scale or focus change) 0.14 ms; `paint_frame` on a normal
   frame is a comparison. With `present_with_damage`: the margin needs no rectangle on a normal
   frame; on the two repaint frames damage the whole surface (a buffer after a resize is new
   memory).
6. **Pointer.** `CursorMoved` is in surface coordinates: `shadow.to_content(x, y)` gives content
   coordinates for hit-testing (`to_surface` goes back). In the resize band they are negative or
   past the content size: the compositor delivers events only from the content grown by
   `RESIZE_BAND` (6 logical px, the winit patch's input region) and lets clicks in the rest of the
   shadow through to the window behind. An edge hit test written as `x < edge` (not `0 <= x <
   edge`) therefore catches the outer band too and `drag_resize_window` keeps working; an app that
   resizes only from inside its content needs no change. `shadow.takes_input(x, y, sw, sh)` is the
   same rule for tests. X11: the same, by an input shape.
7. **Minimum size.** The winit patch counts min/max sizes as CONTENT sizes on Wayland, so
   `with_min_inner_size(520x360)` keeps its meaning there. On X11 the window manager sees the whole
   window: call `shadow.set_min_content_size(&window, size)` after `attach` on every route (it adds
   the margin on X11 only) instead of the attribute.
8. **Sizes you request** (`request_inner_size`) are surface sizes on Wayland: add `2 * margin /
   scale` logical px.

The shadow's logical reach is 36 px (`EXTENT`); the margin in device px is `ceil(36 * scale)`. At
fractional scales it is whole for 1.25, 1.5 and 1.75 (45, 54, 63); at others the content edge
falls between device pixels and blurs by under half a pixel.

### What was run (2026-11-14, this machine: KDE Plasma 6 Wayland, 3440x1440, scale 1; cargo 1.99)

`examples/shadow.rs` models the apps (content bitmap rendered separately, copied to the margin
offset; `drag_resize_window` from the content edges, `drag_window` from a fake 40 px title row).
`MD_SHADOW=client|kwin|none`; arguments `backdrop`, `backdrop-dark`, `maximize`.

- softbuffer: stock 0.4.8 gives an opaque black margin; with the alpha patch the margin is
  transparent and the shadow shows. Screenshots over a light and a dark backdrop: the content
  pixels (corner marks, 1 px border, title row) are exactly what was rendered and the shadow is
  pixel-identical to the KWin route's.
- KWin's `frameGeometry` read from a KWin script: normal 600x400 (the content; surface 672x472);
  maximized 3440x1396 (surface 3440x1396, margin 0, content flush to the screen edges); restored
  600x400 with the shadow back; quick-tiled left 1720x1396 (surface 1720x1396, margin 0, content
  edge on the screen edge and flush on the split line).
- X11 (Xwayland, `WAYLAND_DISPLAY` unset): ARGB window, `_GTK_FRAME_EXTENTS` honoured by KWin:
  `frameGeometry` 600x400 normal, 3440x1396 maximized, 1720x1396 tiled; the shadow shows. On X11
  the margin stays in force when maximized (it lies off screen) and when tiled (the shadow is then
  drawn over the neighbouring tile: winit's X11 window does not report tiling, so the app cannot
  know; GTK sets the extents to 0 on maximize and tile for this reason).
- A wlroots compositor: sway in a container (`WLR_BACKENDS=headless`, no display access): the
  client route works, window geometry honoured (`swaymsg` rect = content), fullscreen margin 0,
  scale 2 correct. Not run: Mutter/GNOME (not available), a click through the margin and
  `drag_resize_window` from the outer band (need input), fractional scales on a real compositor.

GNOME (Mutter) and wlroots compositors have no KWin shadow protocol, so the kwin route returns
`None` there; they use the client route when the patches are in. The KWin shadow object is leaked,
not destroyed, when `Shadow` is dropped: the app's state is usually dropped after the event loop,
when the `wl_display` is gone, and a request then crashes. The compositor removes it with the
surface.
