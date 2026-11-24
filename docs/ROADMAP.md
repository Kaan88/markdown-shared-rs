# Roadmap: what is not built yet

The gaps between the toolkit as it is ([CATALOGUE.md](CATALOGUE.md)) and what the applications
need: mdmix now, then a music player, a file manager and a terminal emulator. Written
2026-11-24 from a survey of the crates, after the first batch of that survey was built (`fmt`,
`select`, `table`, `grid`, `splitter`, the painter's shapes, `slider`, `commands`, `pool`,
`mdshared-term`); updated the same day with the owner's decisions and the order of work.

This file is the one place the plan lives. Notes kept elsewhere (an agent's scratch folder, a
chat) are not the plan: copy anything that matters here.

## How to claim an item

1. Change its **Status** from `open` to `claimed: <who>, <date>` in a commit of its own on
   your branch, so two agents do not build the same thing. If it is already claimed, pick
   another or ask the owner.
2. Read the modules under **Builds on** and the rules in [../AGENTS.md](../AGENTS.md) and
   [NEW-MODULE.md](NEW-MODULE.md).
3. Items marked **owner decides** in the Deps column need an answer before any code: a new
   dependency, or a choice that changes what the applications look like.
4. When done: Status `done: <commit>`, and anything left over becomes a new row.
5. A piece goes to `main` once one application really uses it and it has been reviewed; it is
   marked provisional in the catalogue until a second application uses it.

Sizes are for the model, its drawing, tests and docs together: **S** up to about 300 lines,
**M** about 300 to 1,000, **L** 1,000 to 3,000, **XL** more, or several systems' glue.
Risk is how likely the piece is to need rework once a person uses it: **low** (pure
arithmetic), **medium** (feel, or one system's API), **high** (three systems' APIs, or
something that cannot be tested headless).

## Decided by the owner (2026-11-24)

| Question | Answer |
|---|---|
| JPEG and other decoders (F6, M5, F5) | `zune-jpeg =0.5.15`, pure Rust, behind the image module's decoder slot |
| Accessibility (W8) | AccessKit: `accesskit` + `accesskit_winit`, behind an opt-in feature `a11y` |
| WinRT for the media session (M1) and notifications (W7) | The `windows` crate at `=0.62.2` (already in `Cargo.lock` through fontique) with the WinRT features, behind opt-in features (`media-session`, `notify`); on macOS `objc2-media-player` and `objc2-user-notifications` 0.3 (the objc2 0.6 family); on Linux MPRIS and `org.freedesktop.Notifications` over zbus, which is there |
| Files and images on the clipboard (L9) | Hand-written formats, no new crate |
| The tray icon (W6) | Hand-written per system, no new crate: `Shell_NotifyIcon`, StatusNotifierItem over zbus, `NSStatusItem` through objc2-app-kit (the agent's choice; no dependency involved) |
| Unicode widths for the terminal (T4) | Generated into the source from the data of `unicode-width` 0.2.3; no run-time dependency (the agent's choice) |
| The shell's context menu for files, audio decoding | Deferred (rows F7 and M6) until the owner says otherwise |
| The order of work | The music player's pieces first (the owner, 2026-11-24: mdmix is made usable from what exists by its own session), then the file manager and the terminal, one application's batch at a time (below) |
| How the work is run | At most three agents at once (the owner, 2026-11-24): the coordinating agent on one item, DeepSeek Flash on a safe, low-risk one, and one Claude Opus helper (high effort) on a medium one in its own worktree; risky items stay with the coordinating agent; no wider fan-out |

`feat/hosted` (the worktree `mdshared-hosted`, uncommitted) is a Win32 "hosted mode" for a
component that draws into a window whose loop belongs to another program. It is not the base of
the window host W1, which is for the applications' own winit loops; its placement rules (screen
coordinates, the nearest monitor, `WM_DPICHANGED`, never restore minimised) are the ones W3 keeps.

## The order of work

| Phase | What | Items |
|---|---|---|
| 0 | The animation module's review fixes | done on `feat/anim` (`6027946..bc8fa9f`); the breaking changes for mdmix are listed in [FEATURES.md](FEATURES.md), "The review's fixes" |
| 1 | **Music player**, one item at a time, in this order: the seek bar, cover art and its colours, meters with the waveform and spectrum, the knob and fader, JPEG and the thumbnail cache, the media session; then sectioned lists, cursors, the horizontal scrollbar and theme tokens; then the menu's look, which mdplay's mockup asked for (the owner, 2026-11-24) | M3, M5, M2, M4, F6, M1, L2, W4, L3, W5, L11 |
| 2 | **mdmix's further pieces**, when mdmix asks for them (it is made usable from what exists first, by its own session): submenus, window state, tray and notifications, focus | L1, W3, W6, W7, W2 |
| 3 | **File manager** | F1, F2, F3, F4, F5, L5, L6, L7, L8, L9, L4 |
| 4 | **Terminal** | T2, T1, T3, T4, T6, T7, T5 |
| any | When an application asks for it | W1 (once a second application would share it), W8 (after W2), W9 (before a first release), the X items |

## Shell and window

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| W1 | **A shared window host.** The loop every application rewrites: winit events reduced to the toolkit's own (pointer, key, wheel, text, focus, scale), the softbuffer present, damage rectangles, timers (caret blink, tooltip delay, the animator's deadline in one `WaitUntil`), the hidden first frame. `docs/PLAN.md` step 4. The applications plug regions into it | XL | high | none (winit, softbuffer are there; softbuffer becomes a real dependency behind a feature) | `anim::Animator::end_frame`/`wake_at`/`blink`, `window`, `chrome`, `corners`, `shadow`; two real loops to read: mdview's and mdedit's `app.rs` | open |
| W2 | **Focus model.** Which widget has the keyboard, Tab order, focus rings on keyboard use only, focus scopes for a dialog or a menu, return of focus when one closes | M | medium | none | `settings::Form` (has its own focus), `textfield`, `slider::Look::focused`, `check::Tick::focused` | done: `focus` (86efedd), written by DeepSeek Flash, reviewed |
| W3 | **Window-state persistence.** Position, size, maximized and the monitor they belong to, as settings lines; restored on screen, on a monitor that still exists, at the right scale | S | medium | none | `mdshared_app::settings`, `place::work_area` | done: `window_state` (9dcb256), written by DeepSeek Flash, reviewed (the fallback made the nearest monitor) |
| W4 | **Cursors.** One enum of the cursors the widgets ask for (resize east-west for `table` and `splitter` edges, text, hand, grab, not-allowed) and the hit that picks one, so the loop sets it in one place | S | low | none | `table::HeaderHit`, `splitter::Splitter::hit`, `window::edge_at` | done: `cursor` (fcee44c), written by DeepSeek Flash, reviewed |
| W5 | **Theme tokens.** The palette as named tokens with an accent taken from the system (optional), high-contrast variants, and sizes (radius, row height, text sizes) that the widgets read from one place. Opt-in; its defaults reproduce `LIGHT` and `DARK` exactly | M | medium | none (the look changes only for an application that opts in) | `theme::Palette`, every `draw_*` | done: `tokens` (ae06eee); the system's accent and high contrast in `platform`; Linux portal reads unverified |
| W6 | **Tray icon and its menu.** For a player or a control panel that keeps running with its window closed | M | high | decided: hand-written per system | `menu::Item` for the items' data, L1 | done: `tray` (bc1295e); Linux unverified, macOS open |
| W7 | **Notifications.** A system toast (output switched, track changed, copy finished), with actions where the system has them | M | high | decided: `windows` WinRT feature, objc2-user-notifications 0.3, zbus | `platform`, zbus | done: `notify`, feature `notify` (c3f3f8d); Linux unverified, macOS open |
| W8 | **Accessibility.** A tree of roles, names and values for screen readers (UI Automation, AT-SPI, NSAccessibility), fed from the same models that draw | XL | high | decided: AccessKit behind `a11y` | every model; W2 first | open |
| W9 | **Translations.** Every text the toolkit itself draws goes through a table an application can replace; plural forms; `fmt` taking a locale's separators | L | medium | none | `fmt`, `overlay`, `notice`, `settings`, `stale` | open |

## Lists, menus and input

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| L1 | **Submenus, and menus anchored to a rectangle.** A menu item that opens another menu to its side with the usual hover delay and keyboard (Right opens, Left closes); a context menu that stays on screen at any corner | M | medium | none | `menu::Menu` (has `above`, `place_in`), `anim::Transient` | done: `submenu` and `menu::Menu::around` (e1d2f37), written by a Claude Opus 5.5 helper, reviewed |
| L2 | **Sectioned and variable-height lists.** Rows of differing heights (prefix sums, estimated heights corrected when measured); group headings that stick to the top while their group scrolls, collapsible groups, with the same questions `rows::Rows` answers (visible, rect, row at, nav, reveal) | M | low | none | `rows`, `select` | done: `sections` (ba2e935) |
| L3 | **Horizontal scrollbar and two-axis scrolling.** `scroll::scrollbar` turned on its side; a viewport model with both offsets (`f64`), for a table wider than its view and for a terminal | S | low | none | `scroll`, `table` (already takes a shifted `row.x`), `scroll::SmoothScroll` | done: `viewport` and `scroll::hscrollbar`/`scrollbars` (a95a6d4) |
| L4 | **Table: hidden and reordered columns.** A header context menu to show and hide columns, dragging a title to another place, both saved in the settings line | M | medium | none | `table` (its doc lists these as refused), L1 | open |
| L5 | **Rename in place.** A text field laid over a row or a grid cell: begins with the stem selected, commits on Enter, cancels on Escape or a click away, refuses names the system refuses | S | medium | none | `textfield`, `rows`/`grid` `rect`, `select::Selection::single` | open |
| L6 | **Breadcrumb bar.** A path as clickable segments with a chevron menu per segment, collapsing in the middle when too long, turning into a text field on click | M | medium | none | `textfield`, `menu`, `segments` (for the look), `mdshared_app::paths` | open |
| L7 | **Drag and drop inside a window.** A drag that starts after a threshold, a drag image, drop targets that light up, drop position between rows, auto-scroll at the edges, files dropped from the system (winit's hover and drop of paths) | M | medium | none | `rows`, `grid`, `select`, `anim` | open |
| L8 | **Dragging files out to other applications**, which winit does not do and needs OLE, XDND/Wayland data devices and NSDraggingSource | L | high | none (three systems' glue, `unsafe`) | L7, `platform` | open |
| L9 | **Files and images on the clipboard.** Copy and cut of file lists (with the "cut" flag file managers honour), paste of them; an image as pixels, both ways | M | high | decided: hand-written formats | `platform` clipboard, `cf_html` as the pattern for a pure format | open |
| L11 | **A menu's look.** The corners, the shadow, the row height, the padding, the hover's shape, the separator's gap and the submenu's gap of `menu::Menu` and `submenu::Cascade` as an opt-in `menu::Look`, for an application whose own chrome is square and flat (mdplay: its toolbar has no shadows, and the rounded, shadowed drop-down clashed with it). The default is today's look, pixel for pixel; a flat preset beside it | S | low | none (the look changes only for an application that opts in) | `menu`, `submenu`, `tokens::Sizes::radius_popup` | claimed: Claude Opus 5.5 for mdplay, 2026-11-24, branch `wk/menulook` |
| L10 | **Settings form with motion and the new widgets.** `settings::Form` draws its own switches, ticks and scrollbar; give it an animator and a slider row | M | medium | none | `settings`, `slider`, `anim`; must stay pixel-identical when not opted in | open |

## Files

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| F1 | **Directory listing model.** The entries of a folder read on a worker (name, kind, size, times, hidden), sorted by any column with folders first, filtered, kept in step with the disk without re-reading it all, with the selection remapped across a re-sort | L | medium | none | `job` or `pool`, `fmt::natural_cmp`, `select::Selection::remap`, `table::Sort`, `tree` (reads folders lazily already) | open |
| F2 | **A watcher that says what changed.** `watch::FileWatcher` says "something in this folder"; a file manager needs created, removed, renamed-from-to and modified per entry, debounced and coalesced | M | medium | none (notify is there) | `watch` | open |
| F3 | **File operations with undo.** Copy, move, delete to the bin and rename as jobs with progress, conflict questions (replace, skip, keep both), cancel that leaves no half file, and a record that undoes each. Data-loss risk: built on a fake file system that can fail at every step, as `moves` and `runs` are | XL | high | none | `runs` (records and undo for notes: the pattern), `files::replace`, `platform::recycle`, `job` | open |
| F4 | **Volumes.** The drives and mounts, their names, kinds, free space, and arrivals and removals; a dead network drive never blocks | M | high | none (`windows-sys`, `libc`, `/proc/self/mountinfo`) | `platform` | open |
| F5 | **Shell icons and thumbnails.** The system's icon for a file type as pixels; a thumbnail for an image file | L | high | none beyond F6 | `Painter::image`, `pool`, `platform`, F6 | open |
| F6 | **Image decoding beyond PNG, and a thumbnail cache.** JPEG at least (album art, photos); a cache of scaled pictures in memory with a byte cap, and on disk keyed by path and change stamp | L | medium | decided: `zune-jpeg =0.5.15` | `mdshared_text::decode_png_rgba`, `Painter::image` (scales well but keeps nothing), `pool`, `mdshared_fetch::cache` (the pattern for a disk cache) | done: `mdshared_text::image` (6abd491, 2b30165), `thumbs` (65520ca) |
| F7 | **The shell's context menu for a file** (the system's own menu, with the entries other programs add) | L | high | none decided | `platform`, F5 | deferred by the owner |

## Media

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| M1 | **Media session.** The system's now-playing surface and media keys: SMTC on Windows, MPRIS on Linux, MPNowPlayingInfoCenter on macOS; title, artist, art, position, play/pause/next from the system | L | high | decided: `windows` WinRT feature `media-session`, objc2-media-player 0.3, zbus | `platform`, zbus | Windows and Linux done: `media_session` (d268848; Linux type-checked only); macOS open |
| M2 | **Meters, waveform and spectrum.** Level meters with ballistics and peak hold that sleep when at rest (mdmix, the player); peaks of a track reduced to one (min, max) a pixel on a worker, cached at several resolutions; spectrum bars on a log axis from the caller's bins | M | low | none (decoding audio is the player's) | `Painter::columns`, `pool`, `slider` | done: `meter` (64ff263), `waveform` (bb685b7), `spectrum` (ba0ab7c, written by DeepSeek Flash, reviewed) |
| M3 | **A seek bar with a hover time and chapter marks.** | S | low | none | `slider` (`Look::buffered`), `overlay::Bubble::tip`, `fmt::duration` | done: `seek` (35822a6) |
| M4 | **Knob and fader.** A rotary knob (drag, Shift for fine, double click to reset, wheel, bipolar for pan) and a fader with a dB scale of ticks, both on `slider::Slider`'s value model | M | medium | none | `slider`, `Painter::arc`/`ring` | done: `knob` (321ac27) |
| M5 | **Cover art and its colours.** The dominant, text and accent colours of a picture (text at 4.5:1 contrast), and the cover drawn with a placeholder when there is none | S | low | none (JPEG covers need F6) | `Painter::image`, `glyph` | done: `cover` (4984e45) |
| M6 | **Decoding audio** (MP3, FLAC, AAC) | XL | high | none decided | the player | deferred by the owner |

## Terminal

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| T1 | **Drawing a grid of cells.** Monospace glyphs placed on a fixed cell grid from the text engine's glyph cache, wide cells, colours and attributes from `mdshared_term::Cell`, box-drawing characters drawn as lines so they join, the cursor, the selection, and only the damaged rows redrawn | L | medium | none | `mdshared_text::TextEngine`, `gfx::Face::Mono`, `mdshared_term::Screen::take_dirty`, `anim::Animator::blink` for the cursor | open |
| T2 | **A pseudo-terminal.** ConPTY on Windows, `openpty`/`forkpty` on Unix: start a shell, read and write on threads, resize, exit status | L | high | none (`windows-sys`, `libc`, both there) but `unsafe`, so its own crate `mdshared-pty` | `job` (thread and wake), `mdshared_term::Screen::feed`/`take_reply` | open |
| T3 | **Reflow on resize.** Re-wrap lines when the width changes, using the `wrapped` flag lines already carry | M | medium | none | `mdshared_term::screen` | open |
| T4 | **Combining marks, grapheme clusters, Unicode widths.** Cells that hold a cluster, not a `char`; widths from the Unicode data | M | medium | none (the table is generated into the source) | `mdshared_term::width`, `screen::Cell` | open |
| T5 | **The rest of xterm that people meet.** OSC 8 hyperlinks, OSC 52 clipboard, colour queries (OSC 4/10/11), synchronized output (2026), underline styles and colours, the kitty keyboard protocol | M each | low | none | `mdshared_term::parse`, `screen`, `input` | open |
| T6 | **A selection that follows rewrites, and words across wrapped rows.** | S | low | none | `mdshared_term::select` | open |
| T7 | **Colour schemes and IME at the cursor.** The 16 colours and the default pair as a scheme (with the palette's light and dark), and the IME composition window placed at the terminal's cursor with the preedit drawn in the cells | M | medium | none | T1, `textfield::Preedit`, winit `set_ime_cursor_area` | open |

## Left over from what exists

| Id | What | Size | Risk | Builds on | Status |
|---|---|---|---|---|---|
| X1 | **See the animations in a window.** mdmix runs the motion (the owner has seen it and likes it); still unseen: the spinner and bar on `cycle`, a caret on `blink`, a page switch cut short by a resize | S | medium | `docs/ANIMATION.md` | open |
| X2 | **Check the Linux reduced-motion reading on a real desktop.** KDE Plasma and GNOME: the three portal keys, the `SettingChanged` signal, the wake | S | low | `platform`, LINUX.md | open |
| X3 | **Motion for the rest.** `notice` bars, the command palette, the tree's disclosure, the tab strip, a menu's hover row | M | medium | `anim`, each widget | open |
| X4 | **A stack of toasts.** `anim::Transient` is one slot; several toasts each leaving on its own | S | low | `anim::Transient`, `overlay::Bubble` | open |
| X5 | **`pool`: cancel by key, and results in a chosen order.** | S | low | `mdshared_app::pool` | open |
| X6 | **`commands`: chord sequences (`Ctrl+K Ctrl+S`) and keymaps per context.** | M | medium | `commands`, `keys` | open |
| X7 | **Compare `mdshared-term` with a real terminal.** Run a known test suite (vttest, esctest) through `Screen` and note the differences; nothing was compared with xterm | M | low | `mdshared-term` | open |
| X8 | **The text field's caret on `Animator::blink`.** `textfield` has no blink of its own; give it an animator and `platform::caret_blink_time()` | S | medium | `anim::cycle`, `textfield` | open |
| X9 | **`system_is_dark` off the UI thread on Linux.** It blocks up to 500 ms and caches false on a timeout, as `motion_reduced` did; `platform::LastGood` and the listener are there to reuse. Owner decides: mdview and mdedit call it, and the first answer would come later | S | medium | `platform` | open |
| X10 | **A page switch that survives a resize.** Redraw the old page at the new size and `begin_with` it | S | low | W1, `anim::PageTransition` | open |

## The owner's numbering

The task that set out this work (2026-11-24) numbered the items differently. The same items:
A5 = L2, A9 = F6, A11 = W2, A12 = L1, A13 = L7 + L8, A14 = L9, A15 = W1, A18 = W3, A19 = W4 + L3,
A20 = W5, A21 = W6 + W7, A22 = W8, A23 = W9; M1 = M3 (seek bar), M2 = M4 (knob, fader),
M3 = M2, M4 = M5 (cover art), M5 = M1 (media session); F3 = L6, F4 = F3, F5 = F5, F6 = F4,
F7 = L5, F8 = F7 (deferred); T1 = T1, T3 = T2, T6 = row damage, part of T1, T7 = T7; the
`mdshared-term` gaps = T3 + T4 + T6.
