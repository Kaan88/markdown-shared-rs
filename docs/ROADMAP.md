# Roadmap: what is not built yet

The gaps between the toolkit as it is ([CATALOGUE.md](CATALOGUE.md)) and what the next
applications need: a music player, then a file manager, then a terminal emulator. Written
2026-11-24 from a survey of the crates, after the first batch of that survey was built (`fmt`,
`select`, `table`, `grid`, `splitter`, the painter's shapes, `slider`, `commands`, `pool`,
`mdshared-term`).

Nothing here is a promise or an order from the owner; it is the list an agent picks from when
told to "take the next piece", and the place to look before proposing a new one.

## How to claim an item

1. Change its **Status** from `open` to `claimed: <who>, <date>` in a commit of its own on
   your branch, so two agents do not build the same thing. If it is already claimed, pick
   another or ask the owner.
2. Read the modules under **Builds on** and the rules in [../AGENTS.md](../AGENTS.md) and
   [NEW-MODULE.md](NEW-MODULE.md).
3. Items marked **owner decides** in the Deps column need an answer before any code: a new
   dependency, or a choice that changes what the applications look like.
4. When done: Status `done: <commit>`, and anything left over becomes a new row.

Sizes are for the model, its drawing, tests and docs together: **S** up to about 300 lines,
**M** about 300 to 1,000, **L** 1,000 to 3,000, **XL** more, or several systems' glue.
Risk is how likely the piece is to need rework once a person uses it: **low** (pure
arithmetic), **medium** (feel, or one system's API), **high** (three systems' APIs, or
something that cannot be tested headless).

## Shell and window

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| W1 | **A shared window host.** The loop every application rewrites: winit events reduced to the toolkit's own (pointer, key, wheel, text, focus, scale), the softbuffer present, damage rectangles, timers (caret blink, tooltip delay, the animator's deadline in one `WaitUntil`), the hidden first frame. `docs/PLAN.md` step 4. The applications plug regions into it | XL | high | none (winit, softbuffer are there) | `anim::Animator::end_frame`/`wake_at`, `window`, `chrome`, `corners`, `shadow`; two real loops to read: mdview's and mdedit's `app.rs` | open |
| W2 | **Focus model.** Which widget has the keyboard, Tab order, focus rings on keyboard use only, focus scopes for a dialog or a menu, return of focus when one closes | M | medium | none | `settings::Form` (has its own focus), `textfield`, `slider::Look::focused`, `check::Tick::focused` | open |
| W3 | **Window-state persistence.** Position, size, maximized and the monitor they belong to, as settings lines; restored on screen, on a monitor that still exists, at the right scale | S | medium | none | `mdshared_app::settings`, `place::work_area` | open |
| W4 | **Cursors.** One enum of the cursors the widgets ask for (resize east-west for `table` and `splitter` edges, text, hand, grab, not-allowed) and the hit that picks one, so the loop sets it in one place | S | low | none | `table::HeaderHit`, `splitter::Splitter::hit`, `window::edge_at` | open |
| W5 | **Theme tokens.** The palette as named tokens with an accent taken from the system (optional), high-contrast variants, and sizes (radius, row height, text sizes) that the widgets read from one place | M | medium | owner decides (changes the look) | `theme::Palette`, every `draw_*` | open |
| W6 | **Tray icon and its menu.** For a player that keeps running with its window closed | M | high | owner decides (a tray crate, or three systems' own APIs) | `menu::Item` for the items' data | open |
| W7 | **Notifications.** A system toast (track changed, copy finished), with actions where the system has them | M | high | owner decides (WinRT on Windows, the portal on Linux, UserNotifications on macOS) | `platform`, zbus (already there) | open |
| W8 | **Accessibility.** A tree of roles, names and values for screen readers (UI Automation, AT-SPI, NSAccessibility), fed from the same models that draw | XL | high | owner decides (AccessKit is the usual answer and is large) | every model; W2 first | open |
| W9 | **Translations.** Every text the toolkit itself draws goes through a table an application can replace; plural forms; `fmt` taking a locale's separators | L | medium | none | `fmt`, `overlay`, `notice`, `settings`, `stale` | open |

## Lists, menus and input

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| L1 | **Submenus, and menus anchored to a rectangle.** A menu item that opens another menu to its side with the usual hover delay and keyboard (Right opens, Left closes); a context menu that stays on screen at any corner | M | medium | none | `menu::Menu` (has `above`, `place_in`), `anim::Transient` | open |
| L2 | **Sectioned lists.** Rows of two heights: group headings that stick to the top while their group scrolls, collapsible groups, with the same questions `rows::Rows` answers (visible, rect, row at, nav, reveal) | M | low | none | `rows`, `select` | open |
| L3 | **Horizontal scrollbar and two-axis scrolling.** `scroll::scrollbar` turned on its side; a viewport model with both offsets, for a table wider than its view and for a terminal | S | low | none | `scroll`, `table` (already takes a shifted `row.x`), `scroll::SmoothScroll` | open |
| L4 | **Table: hidden and reordered columns.** A header context menu to show and hide columns, dragging a title to another place, both saved in the settings line | M | medium | none | `table` (its doc lists these as refused), L1 | open |
| L5 | **Rename in place.** A text field laid over a row or a grid cell: begins with the stem selected, commits on Enter, cancels on Escape or a click away, refuses names the system refuses | S | medium | none | `textfield`, `rows`/`grid` `rect`, `select::Selection::single` | open |
| L6 | **Breadcrumb bar.** A path as clickable segments with a chevron menu per segment, collapsing in the middle when too long, turning into a text field on click | M | medium | none | `textfield`, `menu`, `segments` (for the look), `mdshared_app::paths` | open |
| L7 | **Drag and drop inside a window.** A drag that starts after a threshold, a drag image, drop targets that light up, drop position between rows, auto-scroll at the edges | M | medium | none | `rows`, `grid`, `select`, `anim` | open |
| L8 | **Drag and drop with the system.** Files dragged in (winit gives hover and drop of paths); files dragged out to another application, which winit does not do and needs OLE, XDND/Wayland data devices and NSDraggingSource | L | high | owner decides (three systems' glue, `unsafe`) | L7, `platform` | open |
| L9 | **Files and images on the clipboard.** Copy and cut of file lists (with the "cut" flag file managers honour), paste of them; an image as pixels, both ways | M | high | arboard carries images behind a feature that is off here (it brings an image crate: owner decides); file lists need each system's format | `platform` clipboard, `cf_html` as the pattern for a pure format | open |
| L10 | **Settings form with motion and the new widgets.** `settings::Form` draws its own switches, ticks and scrollbar; give it an animator and a slider row | M | medium | none | `settings`, `slider`, `anim`; must stay pixel-identical when not opted in | open |

## Files

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| F1 | **Directory listing model.** The entries of a folder read on a worker (name, kind, size, times, hidden), sorted by any column with folders first, filtered, kept in step with the disk without re-reading it all, with the selection remapped across a re-sort | L | medium | none | `job` or `pool`, `fmt::natural_cmp`, `select::Selection::remap`, `table::Sort`, `tree` (reads folders lazily already) | open |
| F2 | **A watcher that says what changed.** `watch::FileWatcher` says "something in this folder"; a file manager needs created, removed, renamed-from-to and modified per entry, debounced and coalesced | M | medium | none (notify is there) | `watch` | open |
| F3 | **File operations with undo.** Copy, move, delete to the bin and rename as jobs with progress, conflict questions (replace, skip, keep both), cancel that leaves no half file, and a record that undoes each | XL | high | none | `runs` (records and undo for notes: the pattern), `files::replace`, `platform::recycle`, `job` | open |
| F4 | **Volumes.** The drives and mounts, their names, kinds, free space, and arrivals and removals | M | high | each system's own API (`unsafe` on Windows, `/proc/mounts` and UDisks on Linux, DiskArbitration on macOS) | `platform`, zbus | open |
| F5 | **Shell icons and thumbnails.** The system's icon for a file type as pixels; a thumbnail for an image file | L | high | owner decides (JPEG and other decoders: see F6) | `Painter::image`, `pool`, `platform` | open |
| F6 | **Image decoding beyond PNG, and a thumbnail cache.** JPEG at least (album art, photos); a cache of scaled pictures in memory with a byte cap, and on disk keyed by path and change stamp | L | medium | **owner decides: a JPEG decoder is a new dependency** (`zune-jpeg` or `jpeg-decoder`), or the systems' own codecs (WIC, ImageIO, none on Linux) | `mdshared_text::decode_png_rgba`, `Painter::image` (scales well but keeps nothing), `pool`, `mdshared_fetch::cache` (the pattern for a disk cache) | open |

## Media

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| M1 | **Media session.** The system's now-playing surface and media keys: SMTC on Windows, MPRIS on Linux, MPNowPlayingInfoCenter on macOS; title, artist, art, position, play/pause/next from the system | L | high | owner decides (WinRT on Windows needs the `windows` crate or hand-written COM; MPRIS is zbus, already there) | `platform`, zbus | open |
| M2 | **Waveform and spectrum data.** Peaks of a track reduced to one (min, max) a pixel on a worker, cached; `Painter::columns` already draws them | M | low | none (decoding audio is the player's) | `Painter::columns`, `pool` | open |
| M3 | **A seek bar with a hover time and chapter marks.** | S | low | none | `slider` (`Look::buffered`), `overlay::Bubble::tip`, `fmt::duration` | open |

## Terminal

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| T1 | **Drawing a grid of cells.** Monospace glyphs placed on a fixed cell grid from the text engine's glyph cache, wide cells, colours and attributes from `mdshared_term::Cell`, box-drawing characters drawn as lines so they join, the cursor, the selection, only the dirty rows | L | medium | none | `mdshared_text::TextEngine`, `gfx::Face::Mono`, `mdshared_term::Screen::take_dirty`, `anim` for the cursor blink | open |
| T2 | **A pseudo-terminal.** ConPTY on Windows, `openpty`/`forkpty` on Unix: start a shell, read and write on threads, resize, exit status | L | high | none (`windows-sys`, libc calls by hand or `rustix`, both there) but `unsafe`, so it lives in a platform module | `job` (thread and wake), `mdshared_term::Screen::feed`/`take_reply` | open |
| T3 | **Reflow on resize.** Re-wrap lines when the width changes, using the `wrapped` flag lines already carry | M | medium | none | `mdshared_term::screen` | open |
| T4 | **Combining marks, grapheme clusters, Unicode widths.** Cells that hold a cluster, not a `char`; widths from the Unicode data | M | medium | none (the tables can be generated into the source) | `mdshared_term::width`, `screen::Cell` | open |
| T5 | **The rest of xterm that people meet.** OSC 8 hyperlinks, OSC 52 clipboard, colour queries (OSC 4/10/11), synchronized output (2026), underline styles and colours, the kitty keyboard protocol | M each | low | none | `mdshared_term::parse`, `screen`, `input` | open |
| T6 | **A selection that follows rewrites, and words across wrapped rows.** | S | low | none | `mdshared_term::select` | open |

## Left over from what exists

| Id | What | Size | Risk | Builds on | Status |
|---|---|---|---|---|---|
| X1 | **See the animations in a window.** Nothing in `anim` has run in a real loop; wire it into one application (or an example with softbuffer) and let the owner judge the timings | S | medium | `docs/ANIMATION.md`, W1 if it exists by then | open |
| X2 | **Check the Linux reduced-motion keys on a real desktop.** `platform::motion_reduced` reads three portal keys written from memory | S | low | `platform`, LINUX.md | open |
| X3 | **Motion for the rest.** `notice` bars, the command palette, the tree's disclosure, the tab strip, a menu's hover row | M | medium | `anim`, each widget | open |
| X4 | **A stack of toasts.** `anim::Transient` is one slot; several toasts each leaving on its own | S | low | `anim::Transient`, `overlay::Bubble` | open |
| X5 | **`pool`: cancel by key, and results in a chosen order.** | S | low | `mdshared_app::pool` | open |
| X6 | **`commands`: chord sequences (`Ctrl+K Ctrl+S`) and keymaps per context.** | M | medium | `commands`, `keys` | open |
| X7 | **Compare `mdshared-term` with a real terminal.** Run a known test suite (vttest, esctest) through `Screen` and note the differences; nothing was compared with xterm | M | low | `mdshared-term` | open |
