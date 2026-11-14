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
| Alive | `sister::alive(name) -> bool` | no | whether a window of `name` answers: a bare connection, closed at once, that changes nothing in it; up to 300 ms, so off the UI thread |

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

### Live text

`?text=<n>` announces `n` bytes of raw UTF-8 that follow the message's terminating empty line,
unframed and uncounted otherwise: the sender writes `Request::to_wire(app)` (the message, then the
text). `?live=off` has no bytes. Both imply `follow`, so they never raise, focus or open, and the
message gives its path as `?path=`.

```
mdview
?path=/home/u/notes.md
?follow
?line=200
?from=mdedit
?text=18423

<18423 bytes of the document as the editor has it>
```

- Cap: `MAX_TEXT` = 2 MiB per update. A sender whose document is larger sends no text and leaves the
  viewer on the file (the save-driven preview); a receiver that is announced more reads nothing and
  does not answer. Text that is not UTF-8, or does not arrive in full within 2 seconds in all,
  delivers nothing and gets no `ok`. The receiver reads on the listener thread; the window thread
  only sees a finished `Request` in its channel.
- `Request.text` and `Request.live_off` are set only by `send`; no command-line argument carries
  them.
- Sending: `sister::send` connects with a 300 ms limit and writes within 1.5 s; for 200 KB, one send
  and receive over loopback is about 0.5 ms (a test measures it).

Why older receivers are safe. An old receiver reads the name line and then lines up to the first
empty line; everything after it is never looked at (`take_while` stops, and the answer is written).
`?text=` is an option line it skips like `?line=`, and the request is `follow`, so it does nothing
with it. The text cannot be taken for a message or paths, because the old reader stops at that empty
line and the connection handles one message. One effect remains, and a test against a copy of the old
listener shows it: an old receiver closes with the text unread, which resets the connection when
that is more than its read buffer held, so for about 2 MiB the sender gets `false` instead of `ok`
(up to at least 20 KB it gets `ok`). The editor must treat `false` as "no live link", never as
an error. The old receiver has by then done nothing to the window.

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

- With `text`: the tab of that file (it must already be open; nothing is opened) shows the text
  instead of the file's, with the same parser, incremental updates and keeping its place; the
  request's `line` is the scroll target. A small "live" mark shows in the tab and status. At most
  one parse a frame; of updates that wait together the last wins, and all earlier ones are skipped.
  Images and links resolve against the file's folder as before. While live, file-change reloads of
  that tab are ignored.
- Live ends, and the tab shows the file again, when: `live_off` arrives; the file changes on disk
  and no newer live text follows (the editor saved, so disk is now the newest: each live text
  records the file's state as of its arrival, and a later difference is a save); the user reloads
  (F5); or the tab closes. A dead editor sends no `live_off`, so while a tab is live the viewer asks `sister::alive("mdedit")`
  every 5 seconds on a helper thread; when none answers, every live tab returns to its file as for
  `live_off`, with the toast "mdedit closed: showing the saved file". No tab live, no check. A `follow` without text does not end it. Only an open, loaded
  tab takes text; a request never opens a file or switches tabs. The mark is a dot before the tab's
  name, and one toast says when it starts.

## What the editor must do

(When this was written it was the editor's to-do list. mdedit 0.5.0 builds the receiving and the
sending, the live text and the side-by-side placement; see its `docs/DESIGN.md`, section 54, and
`docs/HANDOFF.md`. The boxes were not ticked one by one in this file (not re-checked per item).)

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

Live text (unsaved changes in the viewer):

- [ ] While linked, after an edit: send `Request { follow: true, text: Some(whole document), line:
      Some(caret line), from, ..Request::new(path) }`. Debounced to at most 10 a second, the latest
      text when the interval has passed, and only when the text differs from what was last sent.
      Never when the document is over `MAX_TEXT` bytes: send nothing and leave the viewer on the
      file, which it shows after a save.
- [ ] On save: send nothing more for it; the viewer sees the file change and returns to the file.
      (A final live text equal to the saved one is harmless.)
- [ ] On unlink (the user ends the link), on closing the document's tab, and on leaving the file
      (rename, close without save): send `Request { live_off: true, ..Request::new(path) }`.
- [ ] The path is the file's, saved or not; for a document that has no file, send no text.
- [ ] `send` false: as above, no live link. A false after a large text may only mean an old viewer.

## Run and not run

Run on KDE Plasma 6.8 Wayland (CachyOS), with both apps running: side-by-side placement (the editor
and the viewer at the two halves) and a scripted edit shown in the viewer as unsaved text, with the
live dot in its tab; the viewer's return to the saved file when the editor was gone (seen; the
toast was not). Earlier: the KWin move (`examples/place.rs`, checked by a KWin
listing script), the single-instance message with options between two processes, finding and
spawning with a recording script. Not run: Windows (the apps' Windows files were cross-built and
checked under Wine only), X11, GNOME, Plasma 5's script object path (`/<id>`), a second monitor with another scale.
