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

Written 2026-11-14 on branch `feat/shadow`. An undecorated window gets no shadow from KWin on
Wayland. `Shadow::attach(&window)` hands KWin eight tiles (premultiplied ARGB8888, black, about 21
KB in one memfd-backed `wl_shm` pool, 36 px reach, sigma 12, dropped 6 px; peak alpha 0.35, 0.20
when inactive) through `org_kde_kwin_shadow_manager`, on a private event queue over winit's
`wl_display` and `wl_surface` (raw handles; winit's queue is never dispatched). KWin draws it, so
nothing runs per frame. The apps call `Shadow::attach` once after `create_window` and keep the
`Option<Shadow>`; they call `update(scale, active, maximized)` on `ScaleFactorChanged` and
`Focused` (it returns at once when scale and focus are unchanged); a maximised window needs nothing,
KWin hides the shadow itself (seen). The shadow object is leaked, not destroyed, when `Shadow` is
dropped: the app's state is usually dropped after the event loop, when the `wl_display` is gone,
and a request then crashes. The compositor removes it with the surface. `MDSHARED_NO_SHADOW=1`
disables it. `examples/shadow.rs` shows it.

Not available: GNOME (Mutter) and wlroots compositors have no such global, so `attach` returns
`None` and nothing changes (not tested, no such compositor here). X11 sessions: `None`;
`_KDE_NET_WM_SHADOW` was tried (property set correctly, checked with xprop) and KWin did not draw
anything for an Xwayland window, so it was removed. The general route (a transparent margin
plus `xdg_surface.set_window_geometry`) is not reachable with winit 0.30.13: it keeps the
`xdg_surface` private and calls `set_window_geometry(0, 0, w, h)` itself on every resize
(`wayland/window/state.rs`, `reload_state`). Scale factors above 1 are untested: the tiles and
offsets are rendered in device pixels.
