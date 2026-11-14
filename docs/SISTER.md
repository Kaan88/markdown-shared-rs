# The hand-off between mdview and mdedit

Written 2026-11-14 on branch `feat/linux`. Code: `mdshared_app::sister` (finding, the request,
starting, sending), `mdshared_app::single` (the wire), `mdshared_ui::place` (window placement).
Nothing in it runs unless a hand-off happens.

What the user gets:

- "Edit in mdedit" in the viewer opens the file in the editor at the line being read.
- "Preview in mdview" in the editor opens the file in the viewer at the caret's line, keeps the
  viewer scrolled to the caret while the link lasts, and can put the two windows side by side.

## The request

```rust
pub struct Rect { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }   // physical screen px

pub struct Request {
    pub path: PathBuf,
    pub line: Option<u32>,      // source line, from 1
    pub column: Option<u32>,    // with `line`, from 1; the viewer ignores it
    pub follow: bool,           // a live update; see below
    pub place: Option<Rect>,    // move and resize the receiver's window
    pub from: Option<String>,   // the sender's name: "mdview" or "mdedit"
}
```

`follow` means: do not raise or focus the window, do not open the file if it is not already the
file shown, and if no window runs, start nothing.

## Two ways to deliver it

| | Call | Starts a process | Use |
|---|---|---|---|
| Open | `sister::open(name, &Request) -> Result<(), String>` | yes | the first hand-off; works whether or not the other window runs |
| Send | `sister::send(name, &Request) -> bool` | no | live updates (`follow`); false when no window of `name` answers |

`open` finds the application (below), starts it detached (no stdio, no console window on Windows,
its own process group on Unix) with the arguments of `Request::args(name)`, and returns. The
started process parses them like any launch: when a window of that application runs it passes the
request to it over the single-instance port and exits; otherwise it becomes the window and applies
the request itself. `Err` is the toast to show: `mdedit is not installed`.

`send` writes the request to the loopback port of a running window directly. The port is found
from the other application's settings folder: `settings::default_settings_path(name)`, or the file
named by `MD_SISTER_<NAME>_SETTINGS` (for tests).

### Arguments

The path is always the last argument.

| For | Argument | Meaning |
|---|---|---|
| mdview | `--line N` | show source line N (from 1) |
| mdedit | `--goto L[:C]` | caret to line L, column C (exists already) |
| both | `--follow` | a live update (a launch with it starts no window of its own) |
| both | `--place x,y,w,h` | move and resize the window; physical px, x and y may be negative |
| both | `--from NAME` | who is asking |

A bad value (`--line 0`, `--place 1,2,3`) is dropped, not guessed at. `Rect::parse` and
`Rect`'s `Display` are the one form of `x,y,w,h`.

## The wire format

The single-instance message was: the application's name, one absolute path per line, an empty
line. The receiver skips a path that is not an existing file and answers `ok`. A request adds
option lines that begin with `?`, after the paths and before the empty line:

```
mdview
/home/u/notes.md
?line=200
?col=5
?place=100,80,960,1000
?from=mdedit

```

Keys: `line`, `col`, `follow` (no value), `place`, `from`, `path`. Unknown keys and bad values are
skipped one by one. All paths of a message share its options.

Why older builds are safe: an old receiver reads every line as a path, and `?...` is not an
absolute path, so it finds no such file and skips it; it still answers `ok`. A request with none of
the optional fields is a path line alone, byte for byte today's message (tested). A `follow` request
gives its path as `?path=...` and has no plain path line, so an old receiver, which would open
and raise a plain path at every caret move, does nothing. A non-follow request to an old receiver
opens the file and loses the line, which is the right fallback.

Receiver side: `single::listen_requests_waking(app, settings, wake) -> Option<Receiver<Request>>`
(instead of `listen_waking`, which still gives paths only). Sender side:
`single::forward_request(app, settings, &Request) -> bool`, and `single::forward` as before.

## Finding the other application

`sister::find(name) -> Option<PathBuf>`, first match wins:

1. `MD_SISTER_<NAME>` (`MD_SISTER_MDEDIT`), if it names a runnable file. For tests.
2. Beside the running executable: `<dir>/<name>` (`.exe` on Windows).
3. Linux: the `X-AppImage-Path=` in `$XDG_DATA_HOME or ~/.local/share`/applications/`<name>.desktop`
   (written by `desktop_entry::ensure` the first time that application runs from an AppImage), if
   that file is executable; then another AppImage beside the current `$APPIMAGE` named
   `<name>-*.AppImage` (the last by name).
4. Windows: `%LOCALAPPDATA%\Programs\<name>\<name>.exe`, where both installers put it
   (`DefaultDirName` in `mdview.iss` and `mdedit.iss`). The uninstall key's `InstallLocation` is not
   read: no shared code reads the registry.
5. `PATH`.

`find_in(name, &Env)` is the same with the environment passed in. A file must be runnable (a file
with an execute bit on Unix).

## Placing windows

```rust
mdshared_ui::place::work_area(&Window) -> Option<Rect>
mdshared_ui::place::halves(Rect) -> (Rect, Rect)      // left, right; the odd pixel goes right
mdshared_ui::place::set(&Window, Rect) -> bool
```

`set` takes the outer rectangle. Windows and X11: un-maximise, `set_outer_position`,
`request_inner_size`. Wayland on KDE: a client cannot position itself, so a KWin script is
written to `$XDG_RUNTIME_DIR`, loaded and run through `org.kde.KWin` `/Scripting`, and unloaded,
on a helper thread, all failures silent. The script finds the window by process id, un-maximises it,
and sets `frameGeometry` to the rectangle (converted to KWin's logical px by the window's scale),
cut to the area the panels of the screen holding the rectangle's centre leave free. `set` returns
true for "done or asked for", and false on Wayland under any other desktop; show a hint then (the
viewer's toast: "This desktop does not let a window be placed").

`work_area` is the monitor of the window, without the taskbar on Windows. On X11 and Wayland it is
the whole monitor (KWin's script trims the panels when it places). It is `None` when winit knows no
monitor for the window, which Wayland does until the window has been shown on an output.

## What the viewer does with a request

- Not `follow`: opens the file (or switches to its tab), shows `line` (the block that line is in,
  just below the top edge), moves the window if `place`, and raises and focuses the window.
- `follow`: nothing unless the file is the active tab. Then it scrolls to `line`, but a move of
  less than one arrow-key step (48 logical px) is ignored. Of several waiting at once the last
  counts, so a frame moves the page once. No tab switch, no raise, no focus. With `place` it moves.
- A line maps to a block through the line each block starts on (`Document::blocks[i].line`), so the
  viewer's page and an editor with unsaved changes can disagree on what a line is. That is accepted.
- At the top of the page, "the source line" is the line of the first text in view: the innermost
  block there (a list item, a table cell), not the whole list.

## What the editor must do

Receiving ("Edit in mdedit", from the viewer):

- [ ] Accept `--from NAME`, `--follow` and `--place x,y,w,h` in `args.rs`; `--goto` exists.
- [ ] Listen with `single::listen_requests_waking("mdedit", ..)`; open the path, put the caret at
      `line`/`column` (the viewer sends a line only), move the window with `place::set` if `place`.
- [ ] Launch path: a launch with options hands each file over with
      `single::forward_request("mdedit", settings, &request)`; with `--follow` and no running
      window, exit without opening one. Otherwise as `forward` before.

Sending ("Preview in mdview"):

- [ ] First time: `sister::open("mdview", &Request { path, line: Some(caret line), from:
      Some("mdedit".into()), place, ..})`. On `Err(text)` show `text` as a toast. The path must
      be the saved file; the viewer reads the disk.
- [ ] Side by side, once, at that first hand-off: `let (left, right) = place::halves(
      place::work_area(&window)?)`; `place::set(&window, left)` for itself, `place: Some(right)` in
      the request. If `set` returns false, show a hint and send no `place`.
- [ ] While linked, on each caret line change: `sister::send("mdview", &Request { follow: true,
      line: Some(line), from: Some("mdedit".into()), ..Request::new(path) })`. At most 10 a second
      (send the latest line when the interval has passed, not every one), only while linked, and
      never `place` again.
- [ ] `send` returning false: the first seconds after `open` the viewer is still starting, so keep
      the link and try again; after that it means the viewer window is gone: end the link.
- [ ] Unsaved changes are not in the viewer; the line still maps. Saving makes the viewer reload
      (its file watcher) and keep its place.
- [ ] Nothing is started or sent at launch, and nothing runs while no link exists.

## Run and not run

Run on KDE Plasma 6.8 Wayland (CachyOS): the KWin move (`examples/place.rs`, checked by a KWin
listing script), the single-instance message with options between two processes, finding and
spawning with a recording script. Not run: Windows (type-checked for `x86_64-pc-windows-msvc`
only), X11, GNOME, Plasma 5's script object path (`/<id>`), a second monitor with another scale.
