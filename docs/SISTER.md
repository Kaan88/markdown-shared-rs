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
    pub column: Option<u32>,    // with `line`: the byte in that line, from 1
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
| Alive | `sister::alive(name) -> bool` | no | whether a listener is present, including a legacy one; at most a bare connection with no payload, up to 300 ms, so off the UI thread |

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

**Connection authentication.** The listener holds an exclusive lock and publishes a fresh
`instance.port` record in the private endpoint directory. It holds a two-byte
big-endian port followed by 32 random key bytes. Lookups verify the private directory and
the opened file handles; they do not follow file links. A second lock guards publication:
senders briefly retry discovery while a listener is publishing, rather than using a record
from a previous launch. Unix checks the effective-user
owner and requires a 0700 directory with no group/other access to the files. macOS also
checks the extended ACL on the handles and refuses allow entries even with mode 0600.
Windows rejects reparse points and requires the user SID and a protected owner-only ACL.
The endpoint lives in `instance-v2` beside the settings, regardless of launch environment.
On Linux the record and lock names carry a hash of `/etc/machine-id`, falling back to
`/var/lib/dbus/machine-id` and then the kernel hostname. They also carry the network namespace
inode when readable, so separate loopback namespaces do not contend. On macOS they use
the host UUID, with a 100 ms lookup timeout; a failed lookup uses the hostname for that
lookup only. Windows uses MachineGuid, falling back to `COMPUTERNAME`. Discovery opens
both locks read-only; publication opens them read/write.
Neither end sends the key. The sender
opens with a random 32-byte challenge `C`; the listener returns its own random 32-byte `S`
and `HMAC-SHA256(key, 'S' || C || S)`. Only after checking that proof does the sender
write `HMAC-SHA256(key, 'C' || C || S)` and the message below. No paths or live text go to
a reused port. Unauthenticated input is discarded before paths are inspected.

Both peers must support this handshake. `alive` can use a private legacy lock on Windows,
or a bare connection to a legacy decimal port on Unix. On Windows a contended lock readable
by other unprivileged accounts also needs a successful bare connection, and cannot defer
publication. Windows accepts the current user's legacy files
or files owned by Administrators when that group is the token's default owner. Other users
may have read access but not write or control access; SYSTEM and local administrators are
already privileged. The v2 directory and files still require a protected, user-only ACL.
An owner-only Unix settings directory may contain
a legacy port file made group-writable by umask `0002`; it is used only for that bare probe.
No paths or live text go to a legacy listener;
`deliver` returns `Older` and `forward` and `send` return false. Restart and registration only
wait if the predecessor actually holds the legacy lock. On Unix both the record and lock
must be owner-private to delay registration; readable records are only liveness hints.
Registration then retries in the
background for up to ten seconds, then publishes the independent endpoint even if the old
lock is still held. An
unlocked legacy port on Unix (including a reused port) is only a liveness hint and never delays
registration. A later legacy launch cannot overwrite the authenticated record, but unchanged
legacy binaries can still open a separate window because they do not know the new transport.

The older-build examples below describe **payload** compatibility, not a plaintext retry.
The name and option lines must end in LF (CRLF is also accepted), with at most 64 KiB per
line, 256 path/option lines and a 1 MiB header budget. The total path and shared string data
after expanding the header to one request per destination must also fit 1 MiB. Authentication
and the complete header share a two-second receiver deadline even if bytes keep arriving.
The existing 2 MiB text limit applies
separately.

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
(instead of `listen_waking`, which still gives paths to files only). A worker filters ordinary
requests to existing files or folders. A document, `?closed`, or `?unplace` doesn't need an
existing path. An application that wants to check paths itself can opt in to
`single::listen_unresolved_requests_waking`. The reply confirms queueing, not that the window
opened a path. Empty or entirely filtered launches still receive a no-op reply. A paths-only
listener preserves the `ok doc` reply for a fileless document, but queues no path. Sender side:
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
  does not answer. A message with text must name exactly one destination (a path or a document).
  Invalid or overflowing lengths, text that is not UTF-8, and text that does not arrive in full
  within two seconds are all refused without an `ok`. Repeated declarations must agree and
  each fit the cap. The listener reads the text; the window
  only sees a finished `Request` in its channel.
- `Request.text` and `Request.live_off` are set only by `send`; no command-line argument carries
  them.
- Sending: `sister::send` connects with a 300 ms limit. The sender has two seconds for the
  challenge, then up to four seconds to upload the proof, header and text. It starts a
  fresh acknowledgement timer that covers the receiver's remaining header and text
  windows (two seconds each) and the two-second acknowledgement window, with a small
   margin. A single worker handles every request in arrival order, even if it needs no
   filesystem check. A stalled lookup can hold up later requests; a late result or a full
   queue drops only that message. A send that discovers the public channel is closed releases
   the instance lock before closing the request socket and stopping the listener.
   If the sender disconnects after queueing, the window still receives the request.
   The `*_guarded` listener functions return a receiver that releases its registration when
   dropped, even if no more requests arrive. The original receiver-returning functions
   retain their behavior.
   `single::forward` sends large file selections as batches on one authenticated connection.
   The complete selection must fit in 1 MiB, with at most 256 paths per batch. The receiver
   buffers just the path bytes until the final marker, discards incomplete or invalid
   selections, and acknowledges only after queueing the whole selection.
  The 200 KB loopback round trip is measured in a test.

Why older receivers are safe. An old receiver reads the name line and then lines up to the first
empty line; everything after it is never looked at (`take_while` stops, and the answer is written).
`?text=` is an option line it skips like `?line=`, and the request is `follow`, so it does nothing
with it. The text cannot be taken for a message or paths, because the old reader stops at that empty
line and the connection handles one message. One effect remains, and a test against a copy of the old
listener shows it: an old receiver closes with the text unread, which resets the connection when
that is more than its read buffer held, so for about 2 MiB the sender gets `false` instead of `ok`
(up to at least 20 KB it gets `ok`). The editor must treat `false` as "no live link", never as
an error. The old receiver has by then done nothing to the window.

### A document that has no file

Added on branch `f/live-preview-unsaved` (2026-11-15), for the editor's "Preview in mdview" on a
document that was never saved. Three more fields, carried only by `send`:

```rust
pub struct Request {
    // ...
    pub doc: Option<String>,    // the sender's name for a document without a file
    pub title: Option<String>,  // with `doc`: what to call it ("Untitled 1")
    pub open: bool,             // with `doc` and `text`: show it (make the tab, switch, raise)
}
Request::document(id, title)    // such a request; its `path` is empty
sister::doc_id(id)              // Some(id) if it is 1 to 64 of A-Z a-z 0-9 . _ -
sister::deliver(name, &request) -> Delivery   // Taken | Older | Nobody
```

The name is the sender's and lasts as long as the sender has the document open; the receiver
only compares it. It is not a path, and nothing with that name exists on disk.

```
mdview
?doc=mdedit-4120-7
?title=Untitled 1
?open
?line=3
?place=960,0,960,1040
?text=18
?from=mdedit

# never saved yet
```

- Showing it (`open`, with `text`): the one request with text that is not a `follow`. The
  receiver makes a tab for the document if it has none, switches to it, moves its window if
  `place`, and raises it. There is no path line and no `?path=`.
- Later texts: `?follow`, `?doc=`, `?title=`, `?text=`, as for a file. A receiver that has no
  tab for the document does nothing (it never opens one for a `follow`).
- `?live=off` with `?doc=`: the sender no longer sends this document's text.
- Saved: `?path=<the file>`, `?follow`, `?doc=<the name it had>`. The receiver's tab for the
  document becomes the tab of that file.
- No argument carries any of this: a document is started by `open(name, &Request { from, .. })`
  with an empty path, which starts the application bare (or reaches the running window, which
  raises itself), and the text follows over the port. `Request::args` leaves an empty path out.
- The receiver delivers a request with `doc` whether or not anything is on disk
  (`listen_requests_waking`; `listen_waking`, which gives paths, never sees one) and answers
  `ok doc` instead of `ok`.

Why older builds are safe. An older receiver finds only option lines, so no file: it delivers
nothing and answers `ok`. The sender asked about a document and wants `ok doc`, so
`deliver` returns `Delivery::Older` (and `send` false): the editor says that this mdview cannot
show a document that has no file, and ends the link. The saved message does carry a path, as
`?path=` with `?follow`: to an older receiver it is an ordinary live update for a file it does
not show, which does nothing. An older *sender* never writes `?doc=`. A test against a copy of
the old listener shows both answers.

### A view closed

Added 2026-11-22. The request option `?closed` (`Request::closed`, a `follow`, with the file's
`?path=` or the document's `?doc=`) says that the sender no longer shows that file or document:
its tab of it was closed. The viewer sends it to the editor for a tab the editor had asked it to
show (an opening `from=mdedit`, or a document opened by `?open`), so the editor ends that preview
link; when the viewer's window closes it sends one for each such tab still open, waiting at most
half a second, before the process ends. The listener delivers it whatever is on disk (the file
may have been deleted), and the paths-only `listen_waking` never takes it as a file to open. An
older receiver finds a live update with no line and does nothing; an older sender never writes it.
A window that crashed sends nothing: the editor learns that with `alive` (below, "What the editor
must do").

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

**Going back (2026-11-15).** `mdshared_ui::place::Tiling` is one window's side-by-side
arrangement: `set` remembers where the window is (outer rectangle, inner size, maximised) and
then places it; `restore` puts it back, unless the user has moved, resized or maximised it
since (it is then no longer within 4 px of where it was put); `before` says what is remembered,
for an application that exits while arranged and wants to save the place it had. A second `set`
while the window is still where the first put it keeps the first place. Where the desktop does
not tell a window its position (Wayland) nothing is remembered. The request option `?unplace`
(`Request::unplace`, a `follow`, sent alone with an empty `?path=`) asks the other application
to do its `restore`: an older build skips it and does not raise its window. Each application
restores itself when it ends the arrangement and tells the other; one that is told does not
tell back.

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
- `follow`: nothing unless the file is the active tab. Then it scrolls to `line` (and `column`), but
  a small move is ignored (below). Of several waiting at once the last
  counts, so a frame moves the page once. No tab switch, no raise, no focus. With `place` it moves.
- A line maps to a place inside its block (since `f/finer-sync`, 2026-11-15): the block through
  the line each block starts on (`Document::blocks[i].line`), then the line of text inside it. A
  line of a code block is found exactly, a line of a paragraph or list item where its text
  begins, a table by its row. With `column` (`?col=`, the byte in the line, from 1) the place is
  that far along the line's text, so a paragraph written as one long line is shown at the row
  the editor's caret is in. The row that holds the place goes just below the top edge. A build
  that predates this shows the block's first line, and skips `?col=` like any key it has no use
  for; a sender that predates it sends no column, which is the line's beginning. No new key and
  no new answer: nothing has to be told apart.
- The viewer's page and an editor with unsaved changes can disagree on what a line is (the
  viewer reads the file unless it is sent the text). That is accepted.
- A `follow` moves the page only when the place would otherwise leave the top rows: down by a
  step (48 logical px) or more, up as soon as the row would be cut by the top edge.
- At the top of the page, "the source line" is the line of the file whose text is in the first
  row in view, inside a paragraph, list item, code block or table too. "Edit in mdedit" sends
  it, with `column` when that row begins past the line's first text (a row inside a line that
  wraps), and the editor puts its caret there.

- A document without a file (`doc`): see "The preview of a document that has no file" in the
  viewer's `docs/next/live-preview-unsaved.md` (later its DESIGN.md).
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
- [ ] (Since `f/finer-sync`: also `column: Some(the caret's byte in its line, from 1)`, and an
      update when the caret has moved 32 bytes or more along one line.)
- [ ] While linked, on each caret line change: `sister::send("mdview", &Request { follow: true,
      line: Some(line), from: Some("mdedit".into()), ..Request::new(path) })`. At most 10 a second
      (send the latest line when the interval has passed, not every one), only while linked, and
      never `place` again.
- [ ] `send` returning false: the first seconds after `open` the viewer is still starting, so keep
      the link and try again; after that it means the viewer window is gone: end the link.
- [ ] (Since 2026-11-22.) A viewer closed while nothing is sent: at moments the user does
      something (the window gains focus, a menu opens), and not within the first seconds after
      `open`, ask `alive("mdview")` off the window's thread and end the links when it says no;
      when it says yes, ask once more a second later (a viewer whose window was just closed still
      answers until its process ends). No timer beyond that one. A `?closed` from the viewer ends
      the link of that file or document.
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

For the authenticated transport, document delivery, legacy overlap in both startup orders,
deferred registration, generation races, queue rejection, delayed file checks, header limits,
delayed replies, a 300-file hand-off and a selection of 5,000 short paths were tested on
Windows. Linux and macOS (including
host-scoped registration and the inherited ACL test) were type-checked only.
No hand-off between real applications was run. The desktop runs below predate authentication.

Run on KDE Plasma 6.8 Wayland (CachyOS), with both apps running: side-by-side placement (the editor
and the viewer at the two halves) and a scripted edit shown in the viewer as unsaved text, with the
live dot in its tab; the viewer's return to the saved file when the editor was gone (seen; the
toast was not). Earlier: the KWin move (`examples/place.rs`, checked by a KWin
listing script), the single-instance message with options between two processes, finding and
spawning with a recording script. Not run: Windows (the apps' Windows files were cross-built and
checked under Wine only), X11, GNOME, Plasma 5's script object path (`/<id>`), a second monitor with another scale.
