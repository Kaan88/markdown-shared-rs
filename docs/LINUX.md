# Linux (and other non-Windows) support in the shared crates

Written 2026-11-14 on branch `feat/linux` (`7cd6461` when mdedit 0.5.0 and mdview 0.3.0 were
released from it; it is to become `main`). Everything here is under `cfg(not(windows))` (or
`target_os = "linux"`) unless it says otherwise; the Windows paths are unchanged. Both apps use all of
it. Nothing below has run on Windows from this branch: the apps' Windows files were cross-compiled
on Linux and checked under Wine only. Everything marked "run" was run on one machine: CachyOS, KDE
Plasma on Wayland, cargo 1.99, except where sway (headless, in a container) is named.

## What is implemented

| Area | Off Windows | How it was checked |
|---|---|---|
| Downloads (`mdshared-fetch`) | The system libcurl, loaded at run time with `dlopen` (`libcurl.so.4`, then `libcurl.so`; the `libloading` crate). No link-time dependency: without libcurl every download fails as before (`Error::Unsupported`, seen by callers as `State::Failed`). Same contract as WinHTTP: no redirects followed, no cookies/netrc/auth/`Accept-Encoding`, http and https only, TLS verification on, size cap from `Content-Length` and while reading, overall deadline. | The network tests (local TCP server) run on unix; `live_https_badge` (`--ignored`) fetched an https badge. |
| File dialogs (`platform`) | Both apps use these for Open files, Open folder and Save as. `pick_files_async`, `pick_folder_async`, `pick_save_async`: the blocking dialog on a helper thread; one at a time (a second request calls `done` at once with the empty result and returns false). The blocking `pick_files`/`pick_folder` stay for Windows. | Unit test of the one-at-a-time rule; `examples/dialog.rs` run: a portal request was open on the bus while it waited. The apps' dialog flows were not run by hand (not re-checked). |
| Self-update (`mdshared-app`) | `updater::installed_files` checks the executable's kernel mount ID from `/proc/self/fdinfo` against `/proc/self/mountinfo` and `$APPDIR`, and that the AppDir's `AppRun` leads to the file this process executes. For a mounted image it uses the application's inherited AppDir handle and keepalive pipe to find the live FUSE runtime and its open backing file, and it requires the block and inode counts `statvfs` reports for the AppDir to be those of the squashfs superblock inside that file. For extract-and-run the kernel pins the direct parent's executable, so the parent need not keep a separate file descriptor open. A mount source label or filename prefix proves nothing. The swap moves only the file that was verified and the file the updater wrote, and a launch's clean-up takes the installation's lock. | Filesystem staging and recheck tests ran on Windows. The proc fixtures, the superblock reader, the held-file swap and the lock tests are Linux-only: type-checked from Windows, run by CI on Linux. No real AppImage was mounted or updated, so whether a real Type 2 runtime passes these checks is not yet known. |
| Window identity (`window::identify`) | Wayland app id and X11 `WM_CLASS` from the app name; the X11 window icon from PNG bytes. | Used by both apps. Compiles; the identity as the desktop shows it (taskbar name and icon) was not re-checked. |
| Desktop entry (`desktop_entry::ensure`) | From an AppImage: writes `applications/<name>.desktop` and the 256 px icon under `$XDG_DATA_HOME` (or `~/.local/share`) when missing or when the recorded `X-AppImage-Path` differs; never touches an entry without the marker; sets no defaults. | Unit tests with a temp data directory; mdedit run with `$APPIMAGE` set and `XDG_DATA_HOME` on a scratch folder wrote the entry and the icon. |
| Fonts (`mdshared-text`) | `add_fallbacks` (applied by `shared_fonts()`): Windows names first, then Noto/Twemoji/JoyPixels/Symbola, Noto Symbols/Math, DejaVu Sans, and the CJK families, behind the system's fallback for Han, Common, Inherited and Latin. `EMOJI_FAMILIES` for family strings, and `emoji_presentation_runs` / `push_emoji_runs` so that emoji with U+FE0F, keycaps, skin tones and ZWJ sequences draw in colour (see below; both apps call it, the chrome text too; empty on Windows). | `examples/fontprobe.rs` rendered to PNG and looked at. |
| Clipboard and icons (`mdshared-ui`) | The clipboard uses `arboard`, one handle kept for the life of the process; the icons come from a small subset of the Fluent UI System Icons built into the binary. | Copy and paste read back with `xclip` on X11 and between two editor windows (mdedit's HANDOFF); not checked on Wayland. Icons looked at in shots; some were never seen (mdedit's HANDOFF lists them). |
| Hand-off between the apps (`sister`, `single::forward_request`, `place`) | Finding the other app (`$APPIMAGE` neighbours and the `X-AppImage-Path` of its desktop entry among the rules), starting it detached, and sending it a line, a follow update or a window rectangle over the single-instance port. Window placement: winit on X11; on Wayland under KDE a KWin script loaded through `org.kde.KWin` `/Scripting` on a helper thread; false on other Wayland desktops. `sister::alive(name)` asks whether a window answers without sending it anything. A request can carry live text (2 MiB cap) or end it. Request receivers are handed folders as well as files (`mdedit <dir>`); the paths-only receivers still get files only. `docs/SISTER.md` is the protocol. | Unit tests; run on KDE Wayland with both apps: side-by-side placement and a scripted edit showing unsaved in the viewer; the KWin move (`examples/place.rs`, seen with a KWin listing script, left and right half, clamped to the panel), the message between two real processes, a spawn of a recording script. X11, GNOME, Plasma 5 not run. |
| System theme (`platform::system_is_dark`) | `org.freedesktop.appearance` `color-scheme` from the settings portal (zbus; read on a helper thread that then listens for `SettingChanged`; `platform::LastGood` keeps the last answer, and `platform::watch_system_dark(wake)` wakes on a change). The first call in the process waits up to 500 ms for the first answer, as the old blocking read did; every later call is at once. False before the first answer; after a failure the last answer stands. | The old one-shot read ran in about 1.6 ms, dark, matching `gdbus` and KDE's BreezeDark. The non-blocking path and its wake are type-checked only. |

The emoji box: a cluster flagged as emoji is looked up in the style's family list first and in the
`emoji` generic only afterwards, and the first font with the character wins. The editor's list ends
in `monospace`, which on this machine includes Adwaita Mono, whose monochrome U+1F642 is a tall
pill with a face (glyph id 7598, not `.notdef`): that is the "box". Fix in the family string: name
the colour emoji fonts before the generic (`"..., DejaVu Sans Mono, Noto Color Emoji, monospace"`);
both apps do. The viewer's `sans-serif` had the same flaw (DejaVu Sans has a monochrome U+1F600).
A second case: `❤️` and `⚠️` (a base plus U+FE0F) drew as text glyphs because a font in the style's
own list has the base character. `push_emoji_runs(&mut builder, text, &stack)` (bbb1c7a) pushes the
colour emoji families, then the style's own, for the byte ranges that are a base plus U+FE0F, a
keycap U+20E3 or a skin-tone modifier, or a ZWJ sequence of pictographs. A bare `✔`, `→` or `©` is
not touched. Looked at in shots of both apps; cost about 29 us against 20 us for a line with emoji
in the editor, none for ASCII.

## What is not implemented or not verified

- Any run on Windows of this branch. The apps' Windows files were cross-compiled on Linux and
  checked under Wine only.
- Desktops: GNOME (Mutter) and other portal backends were not available; X11 (Xwayland) was run
  only for the shadow example. KDE Plasma on Wayland and sway (headless) are what ran.
- Fractional display scales in a real window, and two monitors with different scales.
- The dialogs beside a window: they have no parent window, so they are not modal to it.
- A real portal change waking the application: `system_is_dark` now listens for the portal's
  `SettingChanged` signal and `watch_system_dark(wake)` calls a wake, but that path was
  type-checked only (no Linux desktop run); the theme answer itself was read earlier.
- HiDPI scale without a window (`system_scale` is 1.0), double-click and caret-blink settings
  (fixed values), subpixel order (RGB), `reveal_in_file_manager` (runs `xdg-open` on the folder, so
  it does not select the file).
- `update-desktop-database` taking effect in a desktop session, and the apps' "Make default"
  command (it edits `mimeapps.list`; only unit-tested against a temp directory).
- An AppImage updating itself from a published release.
- Non-x86_64 Linux: the manifest only has `linux-x86_64`.

## File associations

`desktop_entry::ensure` writes `MimeType=` (semicolon-terminated) and `Exec=... %F`, then runs
`update-desktop-database <applications dir>` when that program exists (absence and failure ignored),
so the app shows under "Open with" without a logout. It takes no defaults. `make_default(name,
mime_types)` and `is_default(name, mime_types)` edit and read `[Default Applications]` in
`$XDG_CONFIG_HOME` or `~/.config` `/mimeapps.list`; the apps call them from a menu command.
`EDITOR_MIME_TYPES` and `VIEWER_MIME_TYPES` are the shared lists. Run: unit tests against a temp
directory only (and mdedit's entry and icon written to a scratch data folder); the apps' command
was not run, and `update-desktop-database` was not checked to take effect in a desktop session.
The AppImages are built in the apps' repositories (Ubuntu 20.04 container, so glibc 2.31 or later),
not here.

## AppImage self-update

A renamed image, a FUSE mount with a basename-only source, and an extract-and-run launch can
update if the running Type 2 runtime exposes enough information in `/proc`. A plain binary that
inherited the environment, an executable on a nested mount, or an installation path now naming a
different inode cannot. If a wrapper closes the AppDir handle or keepalive pipe inherited by
the application, mounted self-update is unavailable. An ancestor can hold pipes for other
AppImages, so its descriptors are not used as a fallback. Unsupported runtime layouts and
restricted `/proc` also leave self-update unavailable.

Only the application the image starts updates it: the AppDir's `AppRun`, followed to its end,
must be the file the process executes (the applications' images link `AppRun` to
`usr/bin/<name>`, which links to the executable). A second program packed in the same image, an
engine or a sister application, updates nothing. So does an image whose `AppRun` is a script
or a launcher of its own; an application built that way needs another rule before it can
update in place.

The kernel shows no link from a FUSE mount to the process that serves it, so the keepalive
pipe alone would let the live daemon of another image vouch for `$APPIMAGE` while the AppDir
is a different image's. The mount is therefore tied to the file by the filesystem itself: the
squashfs superblock is read from the image (it begins where the runtime's ELF ends, found as
the runtime finds it), and its inode count and its size in blocks must be what `statvfs`
reports for the AppDir. A 64-bit little-endian runtime with a squashfs 4 filesystem is
understood; anything else, or a mount that reports other numbers, leaves mounted self-update
unavailable. Two images agree in both numbers only when they hold the same filesystem, to
the block and the inode. This guards against an inherited or mixed-up environment; another
program of the same account can replace the image without asking, and no check here is a
defence against that. Extract-and-run needs no such tie: only the image's own waiting
runtime is the direct parent of its `AppRun`.

The folder must be writable. The updater checks identity again after download, immediately
before swapping, and before cleaning up. It never truncates an existing `.new`; if identity is
lost after staging, it leaves the staged file alone instead of trying to clean up beside an
unverified target. An install holds a lock on an owner-private, writable `.lck` beside the
image while staging and swapping. The lock file stays in place; it is not a lock on the image.
An interrupted install can replace its leftover `.new` beside the verified image only if it is
a singly linked regular file owned by the current account. The updater opens the new stage
exclusively at mode 0600, then changes it to 0755 after writing. `.new` and `.old` sit beside
the image, with no separate engine.

The swap holds its files by what they are (device and inode), not only by name. What the
image's name denotes is noted before the installation is verified, and the staged file stays
open from its creation. After `<image>` is renamed to `.old` the file moved must be the one
noted, and after `.new` is renamed to `<image>` the file in place must be the one the updater
wrote; otherwise the renames are taken back and the update fails. A failed installation
removes its `.new` only while that name is still its own file. A rename or a removal still
takes a name, so a file given the name in the instant between the check and the call is moved
in the other's place; the check after the rename finds that out and puts it back.
The clean-up at a later launch takes the same `.lck` lock: while an installation holds it,
nothing is removed. It removes `.old` and `.new` only when each is a singly linked regular
file owned by the current account, never a folder or a link, and makes no lock file when
there is nothing to remove. It cannot know which inode an earlier process retired, so for
`.old` the rule is the file's kind and owner, not its identity.

These tests are Linux-only: type-checked from Windows and run by CI on Linux, not on a
desktop. No live AppImage was run or updated, and no NFS mount was tested. In particular the
runtime's descriptors and the numbers `statvfs` reports were written from the Type 2 runtime's
and squashfuse's sources, not observed: if a real launch differs, the checks fail closed and
the update bar does not appear.

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

Both apps pin winit `89ace86e65387b44cec55730e5691e894b451722` and softbuffer
`dd18fec9d08e2f466c42d4c2cece62aa0ee1eba9` this way.

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

## Rounded window corners (`mdshared_ui::corners`, `shadow::Frame`)

Both apps offer a "Window corners" setting (Square, Small = 4, Round = 8 logical px, default
Round; the radius scales with the display). Per platform:

| Where | How |
|---|---|
| Windows 11 | `corners::set_window_corners(hwnd, c)`: `DWMWA_WINDOW_CORNER_PREFERENCE` (do not round, small, round). DWM draws it and squares a maximized window itself. Windows 10 does not know the attribute: the call fails and is ignored. |
| Wayland, client margin | The content's four corner squares are cut with an anti-aliased mask in the ARGB buffer (`Shadow::paint_corners`); the shadow beneath them is drawn for the window's own rounded shape. Square while maximized, tiled or fullscreen (margin 0). |
| KDE Wayland | KWin's shadow route leaves the window opaque, so with rounded corners wanted `Auto` takes the client margin on KDE too (`decide(.., round)`); with Square it keeps KWin's protocol. Switching on from Square at run time on KDE has nothing to draw on until the next start. |
| X11 | No margin by default, so the window is opaque and stays square. With `MD_SHADOW=client` (ARGB window) it rounds like Wayland, and squares while maximized or fullscreen (winit reports no tiling there). |

Cost: the mask touches only the four corner squares (`4 * r^2` pixels, 256 at scale 1), and only
in a frame that copied content into them (`paint_corners` takes the damage rectangles; it rebuilds
the squares from the app's bitmap, so a corner that was not copied again keeps its cut). Nothing
runs per frame when the corners are square or when the window is maximized. The shadow's margin
tiles are unchanged: the blurred shadow of a rounded rectangle differs from the plain one by about
one alpha step for these radii, so only the shadow beneath the uncovered pixels is added
(`r * r` values per top and bottom corner, computed once with the tiles). `Frame::preview` composes
the same pixels over a flat backdrop for the apps' `--shot` pictures.

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

## Window backdrop (`mdshared_ui::blur`)

Added on `f/linux-frost`. **Compiled for Linux only (the module checked in isolation against the
same winit and wayland crate versions, with clippy `-D warnings`); never run or seen on Linux.**

On Windows the "Window backdrop" setting is DWM's Mica or Acrylic behind a chrome made of
transparent pixels (`frost`). On KDE Plasma under Wayland the same chrome logic is used and the
compositor is asked to blur behind the window instead:

- `blur::available(&Window) -> bool`: a Wayland window on a compositor that offers
  `org_kde_kwin_blur_manager`. One registry round trip on a private queue over winit's
  `wl_display` (the way `shadow/wl.rs` does it), kept per window. False on Windows, X11, GNOME,
  wlroots and with `MDSHARED_NO_BLUR` set.
- `blur::set(&Window, on) -> bool`: `manager.create(surface)`, `set_region(None)` (the whole
  surface), `commit`; off is `manager.unset(surface)` and `release`. False when there is no
  protocol (nothing done). Turning it on also calls `Window::set_transparent(true)` (the surface
  then declares no opaque region) and requests a redraw, because the compositor applies the blur
  with the surface's next commit. State is a thread-local map keyed by the `wl_surface` pointer;
  entries are leaked on purpose, as the shadow's are (the apps drop state after the `wl_display`
  is gone). No new crates: `wayland-protocols-plasma` already had the `blur` module; Cargo.lock
  is unchanged.
- `frost::BlurSystem(&Window)`: the `frost::System` that calls the two above. `Frost::sync` is
  unchanged: it asks `available()` when the setting changes and when the window gets focus, and
  frosts the chrome (keys the flat colour out) only while focused and the compositor took the
  request.
- `Frost::wants_alpha()`: true while the chrome is frosted.

### Can the pixels carry alpha on Wayland? Only in part, and the apps must opt in

Blur behind an opaque surface shows nothing, so three things are needed:

1. The surface must not declare an opaque region: winit's `with_transparent(true)` at creation, or
   `Window::set_transparent(true)` later. `blur::set` does the latter, so this needs nothing from
   the app. (It is never switched back off: winit has no getter for the creation-time value, and
   the shadow's client route needs it.)
2. The buffers must be ARGB8888. softbuffer 0.4.8 on Wayland makes XRGB8888 buffers and ignores the
   alpha byte; `docs/patches/softbuffer-0.4.8-wayland-alpha.patch` adds
   `Surface::set_premultiplied_alpha(bool)`. **The window's softbuffer surface belongs to the
   app, so the shared code cannot do this**: the app must call
   `surface.set_premultiplied_alpha(frost.wants_alpha() || shadow.wants_alpha())`. The apps that
   already use the shadow's client route call it with `shadow.wants_alpha()`; KDE with the KWin
   shadow route does not, so there the frost needs the new call.
3. The frame written with alpha: marked chrome pixels through `frost::key_out`, premultiplied
   (which ARGB8888 on Wayland expects), everything else alpha `0xFF`. This is what the Windows
   copy already does; the app's non-Windows copy path must do the same while `frost.key()` is
   `Some`.

Without 2 and 3 the blur is requested and nothing shows; the apps do not have 2 for the frost
today. Apps also pass `&mut frost::BlurSystem(&window)` to `Frost::sync` off Windows.

Known limits: Mica and Acrylic are both just the blur (the veil of Acrylic, 150, is still
applied, Mica has none, so Mica over a bright window can lose contrast); the blur follows the focus
rule of the Windows code (flat while the window is unfocused); a window destroyed and a new one given
the same `wl_surface` address would find a stale entry (not seen: apps keep their windows); the
setting's row is not hidden by the shared code where `available()` is false.
