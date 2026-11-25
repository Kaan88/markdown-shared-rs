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
| How the work is run | Since 2026-11-25 (the owner): the coordinating agent, three Claude Opus jobs, one Claude reviewer (high effort, read-only) and two DeepSeek Flash jobs on safe items, all run through opencode; risky items stay with the coordinating agent. Before that (2026-11-24): at most three agents |

`feat/hosted` (the worktree `mdshared-hosted`, uncommitted) is a Win32 "hosted mode" for a
component that draws into a window whose loop belongs to another program. It is not the base of
the window host W1, which is for the applications' own winit loops; its placement rules (screen
coordinates, the nearest monitor, `WM_DPICHANGED`, never restore minimised) are the ones W3 keeps.

## The order of work

| Phase | What | Items |
|---|---|---|
| P | **Purge fast track** (the owner, 2026-11-25), ahead of the rest: merge `feat/anim` to `main`, then W8 (Windows UIA at Purge's scope), W1 (one window), W5b, X8, P1-P7, P8 last and optional; the consumer is `examples/purge_shape.rs`; the checks C1-C3 below | W8, W1, W5b, X8, P1-P8 |
| 0 | The animation module's review fixes | done on `feat/anim` (`6027946..bc8fa9f`); the breaking changes for mdmix are listed in [FEATURES.md](FEATURES.md), "The review's fixes" |
| 1 | **Music player**, one item at a time, in this order: the seek bar, cover art and its colours, meters with the waveform and spectrum, the knob and fader, JPEG and the thumbnail cache, the media session; then sectioned lists, cursors, the horizontal scrollbar and theme tokens; then the menu's look, which mdplay's mockup asked for (the owner, 2026-11-24), and the look as a user setting | M3, M5, M2, M4, F6, M1, L2, W4, L3, W5, L11, L12 |
| 2 | **mdmix's further pieces**, when mdmix asks for them (it is made usable from what exists first, by its own session): submenus, window state, tray and notifications, focus | L1, W3, W6, W7, W2 |
| 3 | **File manager** | F1, F2, F3, F4, F5, L5, L6, L7, L8, L9, L4 |
| 4 | **Terminal** | T2, T1, T3, T4, T6, T7, T5 |
| any | When an application asks for it | W1 (once a second application would share it), W8 (after W2), W9 (before a first release), the X items |

## Shell and window

| Id | What | Size | Risk | Deps | Builds on | Status |
|---|---|---|---|---|---|---|
| W1 | **A shared window host.** The loop every application rewrites: winit events reduced to the toolkit's own (pointer, key, wheel, text, focus, scale), the softbuffer present, damage rectangles, timers (caret blink, tooltip delay, the animator's deadline in one `WaitUntil`), the hidden first frame. `docs/PLAN.md` step 4. The applications plug regions into it | XL | high | none (winit, softbuffer are there; softbuffer becomes a real dependency behind a feature) | `anim::Animator::end_frame`/`wake_at`/`blink`, `window`, `chrome`, `corners`, `shadow`; two real loops to read: mdview's and mdedit's `app.rs` | done at Purge's scope (one window): `host`, feature `host`, written by Claude Opus 5.5; the rest is W1b |
| W1b | **The rest of the window host.** Tabs and several windows; on Linux the client-drawn shadow and the corners it cuts (`shadow`, `client-margin`), and a backdrop, which needs a present with alpha (stock softbuffer presents Xrgb8888, so frosted chrome is black); on macOS the native frame's specifics, the menu bar and the open-documents event; key releases; regions with their own damage instead of one rectangle | L | high | none | `host`, `shadow`, `menu_bar`, `open_documents`; mdview's and mdedit's loops | open |
| W2 | **Focus model.** Which widget has the keyboard, Tab order, focus rings on keyboard use only, focus scopes for a dialog or a menu, return of focus when one closes | M | medium | none | `settings::Form` (has its own focus), `textfield`, `slider::Look::focused`, `check::Tick::focused` | done: `focus` (86efedd), written by DeepSeek Flash, reviewed |
| W3 | **Window-state persistence.** Position, size, maximized and the monitor they belong to, as settings lines; restored on screen, on a monitor that still exists, at the right scale | S | medium | none | `mdshared_app::settings`, `place::work_area` | done: `window_state` (9dcb256), written by DeepSeek Flash, reviewed (the fallback made the nearest monitor) |
| W4 | **Cursors.** One enum of the cursors the widgets ask for (resize east-west for `table` and `splitter` edges, text, hand, grab, not-allowed) and the hit that picks one, so the loop sets it in one place | S | low | none | `table::HeaderHit`, `splitter::Splitter::hit`, `window::edge_at` | done: `cursor` (fcee44c), written by DeepSeek Flash, reviewed |
| W5 | **Theme tokens.** The palette as named tokens with an accent taken from the system (optional), high-contrast variants, and sizes (radius, row height, text sizes) that the widgets read from one place. Opt-in; its defaults reproduce `LIGHT` and `DARK` exactly | M | medium | none (the look changes only for an application that opts in) | `theme::Palette`, every `draw_*` | done: `tokens` (ae06eee); the system's accent and high contrast in `platform`; Linux portal reads unverified |
| W6 | **Tray icon and its menu.** For a player or a control panel that keeps running with its window closed | M | high | decided: hand-written per system | `menu::Item` for the items' data, L1 | done: `tray` (bc1295e); Linux unverified, macOS open |
| W7 | **Notifications.** A system toast (output switched, track changed, copy finished), with actions where the system has them | M | high | decided: `windows` WinRT feature, objc2-user-notifications 0.3, zbus | `platform`, zbus | done: `notify`, feature `notify` (c3f3f8d); Linux unverified, macOS open |
| W8 | **Accessibility.** A tree of roles, names and values for screen readers (UI Automation, AT-SPI, NSAccessibility), fed from the same models that draw | XL | high | decided: AccessKit behind `a11y` | every model; W2 first | Purge scope (Windows UIA) done on `feat/anim`: module `a11y`, `examples/purge_shape.rs`, `tests/uia/purge_shape.ps1` (50 checks pass on the host, 2026-11-25, after the audit's fixes); several rows cannot be selected through UIA (AccessKit 0.25.2 makes Select, AddToSelection and RemoveFromSelection one click; a rig uses Ctrl+click or keys), see FEATURES; the tree builds on Linux and macOS, nothing serves it there; not tried with Narrator; AT-SPI and macOS adapters open |
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
| L11 | **A menu's look.** The corners, the shadow, the row height, the padding, the hover's shape, the separator's gap and the submenu's gap of `menu::Menu` and `submenu::Cascade` as an opt-in `menu::Look`, for an application whose own chrome is square and flat (mdplay: its toolbar has no shadows, and the rounded, shadowed drop-down clashed with it). The default is today's look, pixel for pixel; a flat preset beside it | S | low | none (the look changes only for an application that opts in) | `menu`, `submenu`, `tokens::Sizes::radius_popup` | done on `wk/menulook` (not merged into `feat/anim`): `menu::Look`, `Menu::with_look`/`set_look`, `Cascade::with_look`; seen in mdplay's mockup, not yet by the owner |
| L10 | **Settings form with motion and the new widgets.** `settings::Form` draws its own switches, ticks and scrollbar; give it an animator and a slider row | M | medium | none | `settings`, `slider`, `anim`; must stay pixel-identical when not opted in | open |
| L12 | **The look as a user setting.** A scheme (dark, light, warm, cool), where the accent comes from (the theme, the system, the cover, a colour), the corners, the shadows' strength, the menu's rows, menus joined to the control that opens them, the speed of motion and the submenu's delay, as plain data an application saves in one setting and applies (mdplay makes its whole look a setting, D-024); the menu's shadow strength and the joined tab with its fillets drawn by `menu` | M | low | none (the look changes only for an application that opts in) | `tokens::accented`, `theme`, `menu::Look`, `anim::fluent`, `mdshared_app::settings` | done on `wk/menulook`, not merged into `feat/anim`; for mdplay's look, D-024: `appearance` (86afe1b), `menu::joins`/`join_under`/`join_over` (09ea256), `Painter::shadow_with` and `Look::shadow_strength` (e8a9df8); the prototype's soft shadows `Painter::soft_shadow` (9bebfcd), `Look::soft_shadow` (b4eb980) and the form's `settings::Style` (b1211de); not yet seen by the owner nor used by an application |

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

## Purge (fast track, 2026-11-25)

Purge (`Kaan88/purge`), an uninstaller, ports its WinUI 3 UI to `mdshared-ui` once this toolkit meets
the checklist in section 0 of Purge's `docs/UI_RUST_ASSESSMENT.md`. Its sections 2, 3 and 6 say why
each piece is needed. "Ready" means:
- every item below and W8, W1, W5b and X8 is on `main`;
- `feat/anim` is merged;
- Windows CI is green;
- C1-C3 are measured.

The rule "a piece reaches `main` once one application really uses it" cannot be met by Purge, because
Purge starts only after the merge. The proposed consumer instead is **`examples/purge_shape.rs`**. It
grows with each item, and C1-C3 measure it. It is:
- one window;
- a 2,000-row multi-select virtualized list with a column header;
- a search field (Ctrl+F);
- a grouped collapsible list;
- a modal dialog;
- a progress bar.

Purge's scope narrows two existing rows:
- **W8:** Windows UI Automation only. AT-SPI and macOS stay open as a row of their own. Author ids
  become UIA AutomationIds. A virtualized list reports its full row count and ScrollItem works.
- **W1:** one window only. No tabs and no several windows.

| Id | What | Size | Risk | Deps | Builds on | Done when | Status |
|---|---|---|---|---|---|---|---|
| W5b | **The system's high-contrast colours.** Extends W5 | S | medium | none | `tokens`, `platform::system_high_contrast` | Under Windows HC the tokens use the active scheme's `GetSysColor` colours (window, text, highlight, hotlight, gray text, button face and text); Mica, Acrylic and frost are off; switching HC while running repaints | done |
| P1 | **Selector bar with N options** (tabs) | S | low | none | `segments` | `segments` with any count; arrow keys; each option one UIA tab or radio item | done: `segments` selector bar (bar_width, draw_selector, selector_rects, selector_at, selector_key), this commit |
| P2 | **InfoBar.** Extends `notice` | S | low | none | `notice`, `overlay` | Info, success, warning and error colours and glyphs; a title, a message and up to two action buttons; a close button; several stacked; in a page, not only at the top | done: `infobar` (not seen by a person) |
| P3 | **Expander** | S | low | none | `anim::Transient`, `tree`'s disclosure | A header with a chevron; an animated body that respects reduced motion; the UIA ExpandCollapse state | done: `expander`, written by DeepSeek Flash (opencode), 2026-11-25 |
| P4 | **Dropdown (combo box) and toggle switch, standalone** | S | low | none | `settings::Form`'s choice and toggle, `menu::Menu::around`, `anim` switch | Usable outside `settings::Form`; the dropdown opens a `menu` under itself; keyboard operable | done |
| P5 | **Hyperlink button** | S | low | none | `cursor`, `text` | Underlined on hover; the hand cursor; Invoke in UIA | done |
| P6 | **Anchored flyout card** | S | medium | none | `menu::Menu::around`, `overlay`, `focus` scopes | Any content (text, a check box, a button) in a card placed like `Menu::around`; Esc and a click outside close it; it is its own focus scope | done |
| P7 | **Read-only selectable text** | M | medium | none | `text`, `textfield`'s selection | Selection by drag, double click and Shift in a paragraph that cannot be edited (wrapped, and monospace); Ctrl+C; Copy in a context menu; the UIA Text pattern is optional | done: `seltext`, 6da5b98; paths wrap inside the area (`Wrap::Anywhere`) since the audit's fixes. Sized for a details pane, not a log view (no scrolling) |
| P8 | **Single instance through a named mutex,** in `mdshared-app` (optional, last) | S | medium | none | `mdshared_app::single` (loopback TCP, which Purge must not use) | A second launch brings the first window forward through a named mutex and `FindWindow`, with no TCP listener | done: `mdshared_app::single_mutex` (Windows only), the window found through a record in a named file mapping rather than `FindWindow`, and the command line handed over in `WM_COPYDATA`; written by Claude Opus 5.5, 2026-11-25. No application uses it yet; until P8b a winit window does not take the hand-over, so `send` gives false there, and the module doc says what a launch does then. Brought forward a real window: not seen by a person |
| P8b | **The hand-over reaching a winit window.** winit's `with_msg_hook` sees posted messages only, and `WM_COPYDATA` is sent, so a winit application needs its window procedure replaced to call `single_mutex::received` | S | low | none | P8, `snap`'s replaced procedure | A function in `mdshared-ui` that replaces the procedure once and queues each hand-over for the loop, with a wake, tested with `SendMessage` as `snap` is | open |

Checks for the ready toolkit (Purge's gates):

| Id | Check | Pass |
|---|---|---|
| C1 | Launch time of `purge_shape`, first frame and names visible; a bare winit+softbuffer window as the reference | Names visible within 400 ms on Purge's laptop with the real engine (Purge measures that one); here: the numbers on this machine, against the bare window. Measured 2026-11-25 with `tests/uia/first_frame.ps1` (10 runs each, alternately, this Windows 11 laptop): first frame, median, 63.9 ms against the bare window's 41.7 ms; names visible to UIA after 395 ms in one run of `purge_shape.ps1` (FEATURES, W8) |
| C2 | The UIA script from W8 (`tests/uia/purge_shape.ps1`), run on the host | Every action W8 lists works. Run 2026-11-25: 50 of 50 checks pass |
| C3 | Size of the `purge_shape` release exe | Under 10 MB; `purge_shape.ps1` fails at 10 MB or more. 2026-11-25: 3,631,616 bytes (3.46 MB) |

## Fast-tracked for the md apps (2026-11-25)

mdedit and mdview take up the toolkit's motion and the pieces below. The work in the apps starts
once `feat/anim` is merged into `main`; neither app moves its pin before then. Asked for by the
mdedit session on the owner's word; the new items come from a Claude Fable review of
`feat/anim` against both apps.

Already on the roadmap, fast-tracked (the rows stay where they are, with their ids):

| Id | For the md apps | Toolkit status |
|---|---|---|
| X3, L10 | Motion for the palette, the tab strip, a menu's hover row, notice bars and the Settings form | open |
| X4 | A stack of toasts, so that messages stop replacing each other | open |
| L1 | `submenu::Cascade` in place of both apps' hand-made submenus and their dwell timer (mdedit `ui.rs:582-588`, `commands.rs:2544`) | done in the toolkit; the apps adopt it |
| L3 | A scrollbar for mdedit's plain preview and side-by-side | done in the toolkit; mdedit adopts it |
| W2 | `focus`: a keyboard for mdedit's Files and Functions panes | done in the toolkit; mdedit adopts it |
| W3 | `window_state`: restore onto a monitor that still exists | done in the toolkit; the apps adopt it |

New, fast-tracked:

| Id | What | Size | Risk | Builds on | Status |
|---|---|---|---|---|---|
| D1 | **Animation damage groups.** A tag set while a region draws, and `end_frame` saying which tags still move. Without it an app that redraws only damaged regions must guess what to redraw or `keep`, and a toast at rest whose track was dropped on a caret-blink frame fades in again | M | medium | `anim::Animator` | open |
| D2 | **`presence_bounds()`** on `Menu`, `Cascade`, `Bubble` and the dialog card. `draw_in_presence` touches the rectangle grown by 18 px, plus the 6 px drop, plus a device pixel; both apps redraw only `menu.rect().inset(-20.0)` (mdedit `app.rs:1435`, mdview `app.rs:853`) | S | low | `anim::presence`, `menu`, `submenu`, `overlay`, `dialog` | open |
| D3 | **A fade that keeps frost's alpha mark**, with a test. `Snapshot::crossfade` assumes alpha 255 on both sides (`anim/region.rs:134`); frost marks chrome with 254 (`frost.rs:31`), so a popup over frosted chrome turns opaque halfway through its fade | S | medium | `anim::region`, `frost` | open |
| D4 | **`chrome::*_animated`**: hover fades for the caption, bar and tab buttons | S | low | `chrome`, `anim` | open |
| D5 | **One preset set for document apps** (menu, tip, toast, dialog), so both apps tune their durations in one place | S | low | `anim` presets | open |
| D6 | **A shared tooltip model**: the delay, blocked after a press, the anchor; both apps carry the same `Tip` struct and timer | S | low | `overlay::Bubble` | open |
| D7 | **Wake on a change of Windows' animation setting**, through the hidden window `platform::watch_contrast` already has | S | low | `platform` | open |

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
| X8 | **The text field's caret on `Animator::blink`.** `textfield` has no blink of its own; give it an animator and `platform::caret_blink_time()` | S | medium | `anim::cycle`, `textfield` | done |
| X9 | **`system_is_dark` off the UI thread on Linux.** It blocks up to 500 ms and caches false on a timeout, as `motion_reduced` did; `platform::LastGood` and the listener are there to reuse. Owner decides: mdview and mdedit call it, and the first answer would come later | S | medium | `platform` | open |
| X10 | **A page switch that survives a resize.** Redraw the old page at the new size and `begin_with` it | S | low | W1, `anim::PageTransition` | open |

## The owner's numbering

The task that set out this work (2026-11-24) numbered the items differently. The same items:
A5 = L2, A9 = F6, A11 = W2, A12 = L1, A13 = L7 + L8, A14 = L9, A15 = W1, A18 = W3, A19 = W4 + L3,
A20 = W5, A21 = W6 + W7, A22 = W8, A23 = W9; M1 = M3 (seek bar), M2 = M4 (knob, fader),
M3 = M2, M4 = M5 (cover art), M5 = M1 (media session); F3 = L6, F4 = F3, F5 = F5, F6 = F4,
F7 = L5, F8 = F7 (deferred); T1 = T1, T3 = T2, T6 = row damage, part of T1, T7 = T7; the
`mdshared-term` gaps = T3 + T4 + T6.
