# Features added after the move

One section per feature that was added to the shared crates after both applications began to use
them. Each says what it is, how the applications use it, and what was not verified. The summary
for a release is in [next/shared-more.md](next/shared-more.md).

## Vue and Svelte highlighting (`mdshared-highlight`)

`.vue` and `.svelte` files, and the fence names `vue` and `svelte`, are highlighted by a line
scanner in `scan::sfc`, in the same family as the Kotlin, Swift and TypeScript scanners. The
bundled syntect set has neither language. Nothing was added to the binary but the scanner's own
code, and no dependency.

What it reads:

- Both: HTML tags (a capitalised name is a component and reads as a type), attributes, quoted and
  unquoted values, `<!-- -->` comments, `&entity;` references, `<script>` as TypeScript (it also
  reads plain JavaScript, as the `ts` scanner does), `<style>` as CSS, or SCSS or Less by `lang`.
- Vue: `{{ expression }}`; directive attributes (`v-*`, `:x`, `@x`, `#x`) are coloured as keywords
  and their quoted value is an expression, also when it runs over several lines.
- Svelte: `{expression}` in text and in tags (`{...props}`, `x={expr}`, `class="a {b}"`), the block
  tags `{#if}` `{:else}` `{/if}` `{@html}` (sigil and word as a keyword), and the directives
  `on:` `bind:` `class:` `style:` `use:` `transition:` `in:` `out:` `animate:` `let:`.

Design and budgets. The scanner keeps the contract of the others in `scan.rs`: one pass per line,
left to right, a small `Copy` state (`Sfc`) from line to line, so the incremental highlighter
(`Language::Scan(Lang::Vue | Lang::Svelte)`) checkpoints it like any scanner and the editor's
line limits (1,000 characters a line, 2 MiB a file) apply unchanged. The embedded TypeScript and
style code is the existing `generic` scanner, run over a slice of the line (`generic_at`); a block
comment or template literal left open in an expression is carried in the state. Plain CSS in a
`<style>` has no `//` comments (`url(http://…)` stays plain), so `Spec` gained a `line_comment`
flag that every existing language sets to true: their output is unchanged.

Not read: `<template lang="pug">` (read as HTML), `lang="sass"` and `stylus` (read as CSS),
Svelte 5 snippets beyond the keyword, strings that run over lines inside a Svelte expression,
and Vue's `<i18n>`-style custom blocks (read as template text). A `>` inside an unquoted
attribute value ends the tag.

Public API: `scan::Lang::{Vue, Svelte}` (new variants of a public enum; an application that
matches `Lang` exhaustively has to add them: neither application does), recognised by
`Lang::from_token` / `from_path`, so `Language::for_path`, `Language::by_token` and `tokens` find
them with no change to a caller. `Language::name()` is "Vue" or "Svelte"; `line_comment()` is `None`.

Run: unit tests in `scan_tests.rs` and `incremental_scan_tests.rs` (colours of tokens, tiling of
the spans, the two samples through the incremental path, edits that open comments and
expressions, hostile and unterminated input, linear time). Not run: either application, a real
`.vue` or `.svelte` project, a comparison of colours against an editor. Speed was checked only by
the linear-time tests, not by a benchmark.

## Access keys in menus (`mdshared_ui::menu`)

Opt-in, per menu. `Menu::new(items, x, y, right).access_keys(true)` makes the menu read its
labels for access keys: `Open &files…` marks `f`, `&&` is a literal `&`, a `&` before white space
or at the end of a label is a literal `&`, and only the first mark counts (a later `&x` loses its
`&` and marks nothing, as on Windows). The marked letter is drawn underlined, the width of a label
is measured without the `&`, and `Menu::access_key(typed: char) -> Option<T>` gives the first
enabled item whose key is the typed letter (equal, or equal in Unicode lower case; Turkish `İ`/`ı`
are not folded to `i`) the highlight and returns its id, for the application to act on as for
Enter. A menu that has not called `access_keys(true)` is exactly what it was: labels are drawn
verbatim, `access_key` returns `None`, and nothing is allocated or parsed.

Why per menu and not per label. Both applications build menus whose labels come from data: the
Recent menus (`e.label`, a file name, which may hold `&`), the editor's completion list
(`c.text`, source code) and its saved searches. Reading `&` in those would eat characters. The
literal labels of the two applications (read on 2026-11-15) contain no `&` at all, so a global
switch would have been harmless today, but the menus above would not be. For a label that
comes from data and sits in a menu that has the switch on, `menu::escape_access` doubles its `&`.

Underlines. They are shown always by default; `Menu::set_access_visible(false)` hides them, so
an application that wants the Windows convention (only after Alt, or when the menu was opened by
keyboard) tracks that itself and calls it when it changes: the shared code does not see the
keyboard. The line is drawn by the new `Painter::underline_range` at the layout's baseline plus 2
device px, 1 device px thick, in the label's colour; it is skipped when the label was elided.

Public API added: `menu::{Access, parse_access, escape_access, access_key_of, same_letter}`,
`Menu::{access_keys, set_access_visible, access_key}`, `gfx::Painter::underline_range`. No
signature changed and `Item` gained no field (a literal `Item { .. }` elsewhere still compiles).

What an application does: add `&` to the labels of the menus it opts in, call `.access_keys(true)`
where it builds them, and in its key handler for an open menu try `menu.access_key(ch)` for a
printable character with no Ctrl or Alt held (after Up, Down, Enter and Escape are handled),
treating `Some(id)` like a click on that item. Mark each item of a menu with a different letter.

Run: unit tests for parsing (`&&`, lone `&`, first mark wins, non-ASCII byte ranges), width
(the `&` is not measured, `&&` is, an opt-out menu measures it), activation (case, Unicode,
disabled items, duplicates, opt-out), and drawing (a menu without marks is bit-identical with
the switch on or off and with underlines shown or hidden; marks change pixels only in the marked
rows). Not verified: how the underline looks (its offset from the baseline was chosen from the
font metrics, not seen), keyboard handling in either application, and the menus' use of it.

## The snap-layouts flyout on a hand-drawn maximize button (`mdshared_ui::snap`)

Built, as an opt-in, Windows-only API: `snap::set_maximize_button_rect(window: &winit::window::Window,
rect: Option<gfx::Rect>)`. Off Windows it does nothing. **Not verified with a real pointer on
Windows 11**: whether the flyout appears and behaves, and how the application's button feels with
it, are untested. Only the message handling is tested (below).

Where hit-testing is done. Nowhere in the shared crate or in winit: winit 0.30.13 (also the
patched fork both applications pin) does not handle `WM_NCHITTEST` at all, so `DefWindowProc`
answers it, and with winit's `WM_NCCALCSIZE` (the client area is the whole window) every point
is `HTCLIENT`. The applications resize and drag themselves: a press on their own edge band or
title row calls `drag_resize_window` or `drag_window`, which post `WM_NCLBUTTONDOWN` with
`HTLEFT`..`HTBOTTOMRIGHT` or `HTCAPTION` to the window. The shared crate touched the window handle
only for `DwmSetWindowAttribute` (`corners`, `platform`); nothing subclasses a window.

What it does. The first call with `Some(rect)` replaces the window procedure of that window with
one that calls winit's for everything and changes only:

| Message | Handling |
|---|---|
| `WM_NCHITTEST` | winit's answer first; only if it is `HTCLIENT` and the point is in the rectangle, `HTMAXBUTTON`. Edges, title row, everything outside the rectangle: unchanged. |
| `WM_NCMOUSEMOVE` on `HTMAXBUTTON` | passed on (Windows' default handling is what shows the flyout) and also sent as `WM_MOUSEMOVE` at the client position |
| `WM_NCLBUTTONDOWN`, `WM_NCLBUTTONDBLCLK`, `WM_NCLBUTTONUP` on `HTMAXBUTTON` | sent as `WM_LBUTTONDOWN` (with `MK_LBUTTON`) and `WM_LBUTTONUP`; not passed on, since the default handling maximizes on its own |
| `WM_MOUSELEAVE` while the pointer is on the button | dropped: Windows sends one when the pointer passes from client area to the button, and winit would turn it into `CursorLeft` |
| `WM_NCMOUSELEAVE`, or a client `WM_MOUSEMOVE` after a visit to the button | a `WM_MOUSELEAVE` for winit first (so it reports `CursorLeft`, and `CursorEntered` with a fresh `TrackMouseEvent` on the next move), then the message |

The application then sees the usual `CursorEntered`, `CursorMoved`, `CursorLeft` and `MouseInput`
events for its button and calls `set_maximized` as before. The press holds winit's mouse capture
as any press does, so the release arrives as a client `WM_LBUTTONUP`, also when the pointer has
moved off the button.

Why not `SetWindowSubclass`. It is exported by name only from comctl32 version 6; a program
without that version's manifest that imports it by name does not start (the first attempt did
exactly that: `STATUS_ENTRYPOINT_NOT_FOUND` in the test binary). The window procedure is replaced
with `SetWindowLongPtrW(GWLP_WNDPROC)` and chained with `CallWindowProcW` instead; per-window state
is a thread-local list (a window's messages run on its thread), dropped on `WM_NCDESTROY`. It never
restores the old procedure, so it must not be combined with another replacement of the same
window's procedure that restores it.

Public API: `snap::set_maximize_button_rect`. The only manifest change is the `windows-sys` feature
`Win32_System_Threading` (`GetCurrentThreadId`, to refuse a call from another thread); Cargo.lock
is unchanged.

What an application does, per window, on Windows: after laying out the caption buttons (every
resize, maximize and change of scale, as the rectangle is in logical pixels from the client origin
and is converted with the window's current scale), call `set_maximize_button_rect(&window,
Some(button))`; call it with `None` when the button is not drawn or something covers it (a menu or
dialog over the title row, a full-screen view). Keep the rectangle clear of the resize band
(`window::EDGE`): over the rectangle the pointer is non-client, so winit does not set the resize
cursor there. The application's own button must not also react to the non-client press, which it
does not, as it never sees one. While the window is maximized the same rectangle applies.

Run: unit tests for the rectangle (scale, rounding outwards, empty or non-finite input), the
point packing (negative screen coordinates), and one test with a real hidden window of its own that
sends the messages with `SendMessage` and checks what the window procedure behind receives: the
hit-test inside, on each edge of and outside the rectangle; the move, press, double press and
release translated; the leave held back and released; the pointer returning to the client area;
presses on `HTCAPTION` and moves off the button untouched; `None` ending a visit; destruction.

Not verified: the flyout on Windows 11 (it also needs the window to have `WS_MAXIMIZEBOX`, which
winit sets for a maximizable window); the real order in which Windows sends `WM_NCMOUSELEAVE`,
`WM_MOUSELEAVE` and the first client move (both orders are handled, by reasoning and not by
observation); behaviour with touch, pen, or a high-DPI monitor change in the middle of a hover; the
interplay with a window dragged by `drag_window` that starts on the button's row; either
application. If the hover or the click misbehaves, calling `set_maximize_button_rect(&window,
None)` returns the window to what it was, bar the replaced procedure that then passes everything on.

## Window backdrop on KDE Plasma (Wayland) (`mdshared_ui::blur`)

Built, compiled for Linux, **never run or seen on Linux**. On KWin under Wayland the "Window
backdrop" setting asks the compositor to blur behind the window with `org_kde_kwin_blur`
(`blur::available(&Window)`, `blur::set(&Window, bool)`), on the window's own connection like the
shadow. Both return false on Windows, X11, GNOME, wlroots compositors and wherever the global is
missing. `frost::BlurSystem(&window)` is the `frost::System` for it; `Frost::wants_alpha()` says
when the window's pixels must carry alpha. The blur shows only through pixels with alpha, so on
Wayland the app must present with `set_premultiplied_alpha(true)` (patched softbuffer) while
`wants_alpha()`. `Backdrop::HELP` says what happens off Windows (a different string off Windows
only). Details, limits and what each app does: [LINUX.md](LINUX.md#window-backdrop-mdshared_uiblur)
and [next/linux-frost.md](next/linux-frost.md).

## Animation (`mdshared_ui::anim`)

Opt-in, and described in full in [ANIMATION.md](ANIMATION.md). `anim::Animator` holds values
that move to their targets, named by the caller and asked for while drawing
(`value`, `toggle`, `presence`), with tweens along easing curves (the CSS cubic-bezier among
them), springs, and the Windows 11 timings in `anim::fluent`; `end_frame()` gives the deadline of
the next frame, or none when everything has arrived, so an idle window still does no work.
`anim::PageTransition` cross-fades, slides or pushes a region from its old content to its new;
`anim::Transient` and `anim::pop` are for what appears and disappears. The painter gained
`offset`, `snapshot` and `fade`. `platform::motion_reduced()` reads the system's wish for less
motion on Windows, macOS and Linux, and an animator told of it moves nothing.

Widgets with motion, each beside the function that existed: `segments::draw_segments_animated`,
`scroll::draw_scrollbar_animated` and `scroll::SmoothScroll`, `check::draw_tick_animated`,
`overlay::Bubble::draw_presence`, `overlay::draw_backdrop_presence` and `dialog_bounds`,
`menu::Menu::draw_in_presence`. No signature changed and no existing function draws differently;
`Painter` gained a private field, which no application can have named.

No dependency was added. Run: the unit tests, on Windows; `cargo check` and clippy of the crate
for Linux. Not verified: anything in a window (how the motions look, the wiring into a winit
loop), either application, macOS, and the Linux portal read on a real desktop.

### The review's fixes (2026-11-24, `6027946..bc8fa9f`)

An Opus review of the three animation commits found ten problems; all are fixed, and a second
review of the fixes found five more, also fixed (`bc8fa9f`). The API changed where a fix needed
it: `anim` is not on `main`, so neither Markdown application can see it, but **mdmix uses it as of
`da70818` and must be adapted** (list below).

Design decisions:

- *Values are kept as `f64`.* `Animatable` would otherwise need a breaking change the day a file
  manager or a terminal scrolls past sixteen million pixels. Springs are stepped on the distance
  to the target, so `Spring` keeps its `f32` API; widget values stay `f32`; `f64` is animatable.
- *`AsId`, and `Id` is not `Hash`.* An `Id` passed as a name is itself, not hashed again; anything
  that hashes still works. The two impls can only coexist if `Id` is not `Hash`.
- *Settle precision per type*: `Animatable::precision()`; a spring ends at the coarser of its own
  threshold and the type's, so an `Rgb` spring stops asking for frames when the colour stops
  visibly changing.
- *A delay is a wait*, kept under reduced motion and on `Motion::SNAP`: a tooltip written the
  obvious way (`presence(id, shown, ENTER.delayed(d), EXIT)`) keeps its delay. A delay too long
  for the clock waits for ever and asks for no frame. `SNAP` with an unchanged target leaves a
  motion in flight alone.
- *`keep` moves the value*, and `end_frame` moves every unfinished track to the frame time, so a
  kept value never asks for frames without moving.
- *The cubic bezier only exists checked*: `Easing::cubic_bezier(..) -> Option` for values from
  outside, `Easing::bezier(..)` (const, clamps, never panics) for constants.
- *A page switch ends when the bitmap size, the scale or the region changes under it*; `apply`
  takes the painter and the region (which also makes the documented example compile).
- *What repeats is a clock*: `Animator::cycle(id, period)` and `Animator::blink(id, on, off)` (wakes
  only at the toggles), `Periodic`; the spinner and the indeterminate bar are drawn from `cycle`,
  pixel for pixel as before at the same phase.
- *Linux reduced motion never blocks*: read on a helper thread that stays to listen for the
  portal's `SettingChanged`; the last good answer is kept, a stale reading loses to a newer one,
  and `platform::watch_motion_reduced(wake)` signals changes (a wake given after the first answer
  is called once at once). `system_is_dark` has the same blocking shape and was left alone, since
  the Markdown applications call it (roadmap X10).

**Breaking changes for mdmix** (old form → new form):

1. `Easing::CubicBezier(a, b, c, d)` → `Easing::bezier(a, b, c, d)`, or `Easing::cubic_bezier(..)?`
   for a curve read from outside. A `match` binds one `Bezier`; `curve.points()` gives the numbers.
2. `PageTransition::apply(&mut anim, &mut bitmap)` → `apply(&mut anim, &mut p, region)` with the
   painter that drew the page and the rectangle given to `begin`; or `apply_dev(&mut anim, &mut
   bitmap, dev_rect, scale)`.
3. `Animatable::parts`/`from_parts` take `[f64; 4]` (was `[f32; 4]`): only for a type mdmix
   implements it for (`as f64` in, `as f32` out; optionally `fn precision()`).
4. `Id` is no longer `Hash`: a name `(id, "part")` → `id.with("part")`; an `Id` as a map key → key
   by the name. The compiler names each place.
5. An `Id` given as a name is no longer hashed again (`anim.get(Id::new("a"))` reads what
   `anim.get("a")` reads). Compiles unchanged.
6. A bare float literal given to `value`/`value_from`/`set` is an `f64` when nothing else types
   it: `anim.value("x", 240.0, m).round()` fails with E0689 → `240.0_f32`.
7. `Snapshot::fits` is false for a bitmap of another size than the one it was taken from.
8. `SmoothScroll` takes `impl Into<f64>`; no break expected, except an argument whose type was
   inferred from the old `f32` parameter (`x.into()`): give it a type.
9. A delay is kept under reduced motion and on `SNAP` (behaviour): drop `.delayed` when
   `!anim.animates()` if skipping it was intended. `SNAP` with an unchanged target no longer
   finishes a motion in flight.
10. In a debug build, one name given two different targets in one frame panics.
11. Linux: `motion_reduced()` answers false until the helper thread's first reading; call
    `watch_motion_reduced(wake)` once and read again when woken.

Also seen, not breaking: `Rgb`, `Rect` and point springs settle sooner; `set_frame_interval`
below 1 ms is 1 ms; `Animator::len` counts cycles and blinks.

Run on Windows: `cargo fmt --all --check`, workspace clippy with warnings as errors, the same for
`mdshared-ui` with `recycle`, `cargo test --release -p mdshared-ui` (389 passed, 7 ignored, 4
doc-tests; 31 tests added); the Linux clippy with a stub `pkg-config` (type-checked, nothing
linked). Not verified: anything in a window; the Linux portal thread on a real bus; macOS (not
compiled); mdmix against this branch (break 6 is the one whose reach is unknown); the full
workspace test run.

## Pieces for the next applications (2026-11-24)

A first batch of general pieces for a music player, a file manager and a terminal, built in one
go. Each is listed with its entry points in [CATALOGUE.md](CATALOGUE.md); what is still missing
is in [ROADMAP.md](ROADMAP.md). All of it is additive: no signature changed, no existing
function draws differently, and neither application uses any of it yet. No dependency was
added (`mdshared-term` has none at all; the painter's paths use `zeno`, which the icons
already used).

| Piece | Notes on the design |
|---|---|
| `mdshared_app::fmt` | Binary units under everyday names (a KB is 1024 bytes), as `plan::size_text` already wrote them. `plan::thousands`, `plan::size_text` and the private copy in `split::answer` now call it, and a test holds their output to the code they had (`size_text` still goes on in MB past a gigabyte, as it did). Dates take an offset in minutes from the caller: no time zone is read. `natural_cmp` is a total order (ties broken by leading zeros, then the plain string order) |
| `mdshared_ui::select` | Rows are kept as sorted runs, so "select all" is one run. The rules are those of a Windows file list. `TypeAhead` takes the time as an argument and compares letters with `menu::same_letter` |
| `mdshared_ui::table` | Spare width goes to columns with a `flex`; too little is taken from them down to their minimums, and then the table is wider than its view. Taking hold of an edge freezes every column at its laid-out width and takes the flex from the dragged one. The settings line holds the widths the columns start from and the sort (`1:240,2:90;sort=2:desc`) |
| `mdshared_ui::grid`, `rows::Rows::band` | `Grid` mirrors `Rows`. The rubber band is in the frame's coordinates; a caller whose view scrolls during a band keeps the press point in content coordinates |
| `mdshared_ui::splitter` | The cut holds on to a ratio, or to the size of pane A or B in px (a sidebar keeps its width when the window grows). Side by side, the drawing is `pane::draw_splitter`, pixel for pixel |
| Painter shapes (`gfx::shapes`) | `image` averages in premultiplied alpha when shrinking and computes only the clipped pixels; it keeps nothing, so a large picture drawn every frame should be scaled once by its owner. Arcs are in turns clockwise from the top, with round ends |
| `mdshared_ui::slider` | `Scale::Db` is for a gain: the track is linear in decibels over 60 dB, with the minimum of the slider (silence) at the bottom. The marks without an end ask the animator for a value that never arrives, so they move while drawn and the window sleeps when they are not |
| `mdshared_ui::commands` | Ids are `u32` (as in `palette::Entry`), names are the stable keys of the settings file (`key.<name>=<shortcuts>`), only overrides are written, unknown names in the file are kept and written back |
| `mdshared_app::pool` | Work is replaced by key while queued, ordered by priority, and dropped or discarded by generation. A panic loses one result and is counted. Dropping the pool does not wait for running work |
| `mdshared-term` | Parser and screen are apart (`Perform`). No reflow on resize; zero-width characters are dropped; widths come from a small hand-written table. The crate doc lists the gaps |

Run on Windows: the unit tests of every crate, clippy with warnings as errors, and those of
the `recycle` feature. Sheets were rendered and looked at once at scale 1:
`target/specimen/shapes.png` and `slider.png` (`cargo test -p mdshared-ui shapes_sheet --
--ignored`, `slider_sheet`).

Not verified: any of it in a window or under a real pointer and keyboard; how the table
header, the slider and the splitter feel; Linux (the whole workspace was type-checked and
linted for it from Windows, nothing was run) and macOS (not built here; CI builds and tests
both); `mdshared-term` against a real terminal or a program running in a pseudo-terminal (there
is none here); `pool` under real load.

## The roadmap's foundations and music-player pieces (2026-11-24, `feat/anim`)

The roadmap's music-player phase and most of its foundations, one commit each. Each module has a
provisional row in [CATALOGUE.md](CATALOGUE.md), and the ROADMAP rows name their commits. All of it
is additive: mdview's and mdedit's code and executables do not change. mdplay, pinned at
`09c67cd`, still builds and passes its tests against this branch.

These pieces need the system:

| Piece | Notes on the design |
|---|---|
| `media_session` (feature `media-session`) | The player gives a `NowPlaying` and reads `Command`s after a wake: SMTC on Windows, MPRIS on Linux |
| `notify` (feature `notify`) | Windows toasts under an AppUserModelID. `Notifier::new` writes `HKCU\Software\Classes\AppUserModelId\<id>` (its name and icon), because an unpackaged executable has no other way to name its toasts. On Linux it uses the desktop's notification server over zbus. A tag replaces the previous notification; at most three buttons |
| `tray` | Windows: `Shell_NotifyIconW` on a hidden window and thread of its own, with no winit; the icon comes back after Explorer restarts (`TaskbarCreated`); the menu is the system's own. Linux: a StatusNotifierItem with a `com.canonical.dbusmenu` menu |
| `window_state` | The saved place is one settings value. `restore` uses the window's own monitor, else the nearest one, scaled and kept on the work area |

The rest is pure models and drawing, with no OS involved: `focus`, `submenu`, `viewport` with
`scroll::hscrollbar`, `cursor`, `tokens`, `spectrum`, `meter`, `waveform`, `seek`, `knob`,
`cover`, `sections` and `thumbs`.

What was run, on Windows:
- the workspace's tests;
- clippy with warnings as errors, for the workspace and for each of the `recycle`, `media-session`
  and `notify` features;
- the Linux type-check with a stub `pkg-config`.

What was run once on Windows 11, with no one clicking:
- the `tray` example: the shell took the icon;
- the `notify` example: its toasts were read back from the system's list.

Not verified:
- the Linux backends on a real bus (type-checked only);
- macOS, where these pieces answer `Unsupported`;
- Wine;
- the tray's menu and the toasts' buttons pressed by a person;
- `focus` and `submenu` under a real keyboard and pointer in an application.

## Rich text pasted as Markdown (`mdshared_app::html_md`, `mdshared_ui::platform::clipboard_html`)

`html_md::to_markdown(html)` turns the HTML a browser, a word processor or mdview put on the
clipboard into Markdown: headings, bold and italic (also as `font-weight` and `font-style` on a
span, as Google Docs writes them, and not for its `<b style="font-weight:normal">` wrapper),
strikethrough, links, images (not `data:` ones), nested and numbered lists, inline code, fenced
code blocks with the language of a `language-…` class, block quotes, rules, hard breaks.
Unknown tags keep their text; scripts and styles go; a table's cells become `a | b`, a row to a
line. Text that Markdown would read as a mark, a tag or a character reference is escaped (`\*`,
`\<b>`, `\<?`, `\&copy;`), also when the HTML splits it between two tags
(`&lt;<span>script&gt;`); inside inline code nothing is marked or escaped. A link to
`javascript:`, `vbscript:` or `data:` keeps its text only. It answers `None` when there is nothing Markdown can say that the plain text does not, and
for `white-space: pre` HTML (a code editor's coloured copy), so the caller pastes the plain text.
Hand-written (no HTML crate), panic-free on any input, nesting capped at 16 for its prefixes, the
output capped at eight times the input; tested with fixed cases, 25,000 generated inputs and every
cut of a real copy. Not handled: Word's lists (paragraphs with `mso-list` styles stay paragraphs
with their bullet character), tables as Markdown tables.

`platform::clipboard_html()` reads the HTML: on Windows the fragment of the "HTML Format"
(`cf_html::fragment`: the header's byte offsets, else the fragment comments, else everything from
the first tag), at most `cf_html::MAX_BYTES`; on Linux and macOS through `arboard`'s `get().html()`
(`text/html`, `public.html`), the whole document a browser offers. Read only when a caller asks;
nothing changes for an existing caller. Used by mdedit's "Paste rich text as Markdown" (issue #8).
Not verified: Linux and macOS reads (compiled by CI only).

## A drop-down list and a toggle switch on their own (`mdshared_ui::dropdown`, `toggle`, 2026-11-25)

Roadmap P4, for Purge. `toggle::Toggle` is a switch's value and the press under way on it:
Space turns it over, and a click does when the button goes up on the switch it went down on.
`dropdown::Dropdown<T>` holds options (a value and a label each), the chosen one, and, while it
is open, the `menu::Menu<usize>` of its options. The menu is the model's own, not the caller's,
because the keys of an open drop-down (Up, Down, Enter) act on the menu's highlight; the caller
reaches it through `menu()` and draws it last with `draw_dropdown_menu`. It is placed with
`Menu::around` round the field grown by `MENU_GAP` (4 px) above and below, so it hangs 4 px
under the field, left edges level, and flips above or leftwards where there is no room.

The keys follow a Windows combo box: closed, Space, Enter, Alt+Down, Alt+Up and F4 open it, Up
and Left, Down and Right, Home and End change the choice without opening, and a letter chooses
the next option that begins with it; open, Escape and Tab close with no change. A click opens
it on the release, as the settings dialog does.

Both draw the settings dialog's pixels. Its switch and its drop-down field were moved, unchanged,
to `toggle::switch` and `dropdown::field` (crate-private), and the single-page and the
categorised forms now draw with them. That the forms still draw what they drew is tested
against frozen copies of the old code: `settings::unchanged` compares the single-page form with
its frozen copy, and for the categorised form the tests of `toggle` and `dropdown` hold frozen
copies of its old switch and field (from `feat/anim`, 6e0b9c0). They draw the form's rows (at
rest, under the pointer, pressed, disabled; light and dark; scales 1 and 1.5), wipe each control
to the ground beside it, draw it again with the frozen code and compare every pixel; nudging
the shared code by one px or one colour step makes them fail. Four more tests do the same with
the standalone widgets, which shows they are the forms' controls; those passed before the move
too, but after it they compare shared code with itself. The focus ring is the one
difference: the dialog paints its own with the row's colour inside it; these use
`focus::draw_ring`, which needs no background.

Added to `menu::Menu`: `min_width(w)` (a drop-down's list is at least as wide as its field;
off unless asked) and `highlight(i)` (the keyboard highlight set by index and brought into view).

The list never covers its field. It is placed as `Menu::around` places a menu, but where it fits
neither below nor above the field it goes to the side with more room, cut to that room, and
scrolls, instead of being pushed over the field (`submenu::around_clear`, crate-private, used
through a crate-private placement of `Menu`; `Menu::around` itself is unchanged). While the list
is open, Up and Down stop at the first and the last option, as WinUI's ComboBox does; a plain
menu still goes round.

Not here: an editable combo box, the labels beside the controls, the menu's closing fade, the
wheel changing a closed drop-down. For a screen reader the caller builds `a11y::combo_box` and
`a11y::switch` (feature `a11y`, added later by W8) from `Dropdown::selected_label` /
`Toggle::on` and `Dropdown::is_open`, with the name and rectangle it has.

Not verified: both were only drawn into bitmaps by tests; neither has run in a window, and the
specimen sheets (`toggle_sheet`, `dropdown_sheet`) have not been seen by a person.

## The Purge fast track's smaller pieces (W5b, X8, P1, P2, P3, P5, P6; 2026-11-25)

Roadmap rows of Purge's readiness checklist; each was written by an agent, reviewed, and then
audited with the rest of the fast track by Claude Fable (the audit's fixes are listed last). All
provisional; none is on `main` yet. Run on Windows only, headless, unless said; no person has
seen any of them.

- **W5b, the system's high-contrast colours** (`tokens`, `platform`). `SystemColors` holds the
  eight `GetSysColor` colours a contrast theme sets; `Tokens::from_system_colors` maps them as
  WinUI does (window, window text, Highlight and HighlightText as accent and on_accent, grey
  text as faint and the lines, button face as the bars when the text reads on it).
  `Tokens::from_system` uses them under Windows high contrast; Linux keeps the toolkit's own
  high-contrast palettes. `hover` and `on` are kept at 4.5:1 for `text`, `faint` and `accent`
  (`legible_fill`), which keeps them close to the window, so a selected row is drawn with
  `tokens::selected(pal, high_contrast)`: Highlight with HighlightText. `allowed_backdrop` turns
  Mica, Acrylic and frost off under high contrast; `platform::watch_contrast(wake)` wakes on a
  switch or a change of the theme's colours (a hidden top-level window on its own thread); the
  host takes that wake. HotLight reaches links through `link::Look::ink`; ButtonText reaches
  nothing (the palette has no field for it). Tested over five Windows themes (Aquatic, Desert,
  Dusk, Night sky, High Contrast Black) by their colours; no contrast theme was switched on.
- **X8, the caret blink** (`textfield`). `TextField::blink_caret(anim, id, blink, focused)`
  gives `caret_on` from `Animator::blink`, with the half-period from
  `platform::caret_blink_time()` (`GetCaretBlinkTime`; INFINITE gives a steady caret). The blink
  restarts on input and when focus arrives; unfocused, with a selection, or under reduced motion
  it asks for no deadline, so the window sleeps. `focused` includes the window's own focus.
  Tested on a manual clock; no application calls it yet, so it has not blinked in a window.
- **P1, a selector bar** (`segments`). `selector_rects`, `selector_at`, `selector_key` (Left,
  Right, Home, End; no wrap), `draw_selector` and its animated form: the two-or-three-option
  switch's look with any count. Purge's tabs are underlined with a count in a second style;
  the selector is a filled pill, so Purge may draw its own over `selector_rects`.
- **P2, the InfoBar** (`infobar`). Four severities with their glyphs, a semibold title, a
  wrapped message, up to two actions and a close button; `InfoBar::parts` for keyboard focus;
  `draw_infobar_with(Look { hover, pressed, focused, high_contrast })`; `stack_layout`. The
  dark error ink is `DARK_ERROR_INK` (WinUI's `#ff99a4`, 4.5:1 or more on every dark surface);
  the palette's `danger` read at 2.6:1. Under the system's contrast theme the caller sets
  `Look::high_contrast` (a palette has no flag), and the card takes the page with the ink as its
  frame.
- **P3, the expander** (`expander`). `Expander` (toggle, `header_hit`, `key`), `draw_header`,
  and `draw_expander_animated`, whose body grows through the animator and snaps under reduced
  motion. Its header takes one string at 32 px; Purge's Settings headers hold a title, a hint and
  a status, so Purge draws those and keeps the model.
- **P5, the hyperlink button** (`link`). Accent text (or `Look::ink`), underlined on hover, the
  hand cursor, Enter and Space without Ctrl or Alt, elided and clipped to its rectangle.
- **P6, the anchored flyout** (`flyout`). A card placed by the menu's geometry round an anchor,
  any content in `content_rect()`, `pointer_down` (a press on the anchor closes and is kept
  from it), Escape, its own focus scope that gives the focus back to what had it (as WinUI),
  `set_anchor` after a scroll. No a11y builder: report it as a non-modal `a11y::dialog`. Purge's
  use is a TeachingTip (centred, with buttons): the content and the centring are Purge's.
- **The audit's fixes** (Claude Fable read all of the above; fixed by Claude Opus 5.5):
  `tokens::selected`; the InfoBar's high-contrast flag and dark error ink; `link::Look::ink`;
  `Dropdown::set_field`, which moves an open list after a scroll or a resize (it stayed where it
  opened); a drop-down list that would show less than one row beside its field covers the field
  instead of a sliver that still takes every key; `Expander::key`, `link::key` and
  `SelectableText::key` take `&KeyName` as the others do; the module docs that said W8 was not
  built now name the `a11y` builders.

## Read-only selectable text (`mdshared_ui::seltext`, P7)
A paragraph that cannot be edited but can be selected, for Purge's log output and the paths in
its details pane. `SelectableText` holds the text, an anchor and a caret as byte offsets on char
boundaries, and the drag in progress; `draw` paints the selection behind a `Painter::paragraph`.
No editing, no caret, no clipboard and no clock: Ctrl+C answers `Action::Copy(text)` and the
caller puts it on the clipboard, and the context menu's items carry the same `Action`s.
The lines come from the new `Painter::paragraph_lines`, which reads the parley layout
`paragraph` already builds and returns each line's byte range, its y and height, and the x of
every char boundary from the same run advances. Hit testing and the highlight therefore use the
drawn glyph positions rather than a second shaping of the text. The model keeps the lines last
given to `press` or `drag` for the keys; the caller must give it fresh ones with `set_lines` after
a resize or a font change.
Two clicks select a word with the classes `textfield` uses; three select the whole logical line
between its newlines, not one wrapped piece, which is what a log line needs. Shift with the
arrows, Home, End and Up/Down extends the selection, and Up and Down keep the x they started
from. Alt is refused.
Run on Windows: `cargo test --release -p mdshared-ui` (seltext's 13 tests and its ignored sheet,
the new gfx test and the rest of the crate), `cargo fmt --all --check` and the workspace's clippy
with warnings as errors. The pixel test draws a selection in both palettes at scale 1 and 1.5 and checks the
highlight under the selected space and the page under an unselected character.
Not verified: a real pointer and keyboard in a window; a person looking at
`target/specimen/seltext.png`; right-to-left paragraphs (their stops follow parley's runs but no
test exercises them); Linux and macOS (not run here). The UI Automation Text pattern is not done.

A review against parley 0.12.0 found faults, fixed in a second commit (`paragraph_lines` and
`seltext` were not pinned by any application yet, so both changed):
- `paragraph_lines` takes the colour the paragraph is drawn in, so it reads the cached layout
  `paragraph` draws instead of shaping the text again in black. It is linear in the text: a
  table from byte offset to stop replaces counting chars from the line start for every cluster
  (a 100,000-char line went from 307 ms to 2 ms in release).
- `Line` gained `edges`, one flag per stop: the chars inside a parley cluster ("e" and its
  accent, a joined emoji, a flag) carry the cluster start's x and `false`. A click lands only on
  an edge, and Left and Right step one cluster.
- The caret remembers whether it is at the end of a wrapped line or the start of the next (the
  same byte offset). A click past the end of a line, End, and Up or Down landing at a line end
  keep it on that line; `caret_line` reports which line it is on.
- Two clicks take the char under the pointer rather than the char after the nearest boundary,
  and a line break is a word class of its own. A lone `\r`, U+2028 and U+2029 end a line for
  three clicks, Home and End, as `\n` does.
- Lines that do not fit the text are dropped where they come in, so lines of another text no
  longer panic in `press`, `drag` or the keys.
- Left and Right drop a selection at its left or right end; Shift+Up on the first line and
  Shift+Down on the last go to the ends of the text; Ctrl+Home and Ctrl+End were added; a
  Shift+click after two or three clicks extends from the far edge of the word or line.
- `draw` gives a selected empty line a highlight as wide as a space, and places the highlight
  from the rounded corner of the area as the glyphs are, so they agree at a fractional x.
  Select all is disabled on empty text.

## The window host for one window (`mdshared_ui::host`, feature `host`, roadmap W1 at Purge's scope)

`host::run(Config, app)` owns the winit loop, the softbuffer surface, the bitmap, the `Gfx` and
the window's one `Animator`; the application implements `host::App` (`start`, `event`, `draw`,
`message`, and with `a11y` `accessibility`) and keeps its models. Nothing is called back into a
widget. The application asks for things through the `Cx` each call is given. The module is a
directory: `host.rs` (the events, the trait, the config, the proxy), `host/cx.rs`,
`host/input.rs` (winit's input reduced), `host/frame.rs` (damage, deadlines, the present) and
`host/run.rs` (the loop).

- **Events** are winit's reduced to logical px: pointer move and leave, press and release,
  wheel (a notch is `WHEEL_LINE` = 48 px; winit's sign, so a wheel turned away from the user is
  +dy; Shift turns it sideways by `viewport::wheel_delta`), key presses as `keys::KeyName` with
  the modifiers and the typed text, input-method text and composition, modifiers, focus, scale,
  resize, theme, `Contrast`, the close request, `Wake` for a timer, and `Action` for an
  assistive technology. `Event` is `non_exhaustive`. A press carries its click count: presses of
  one button within `platform::double_click_time` and inside the system's double-click
  rectangle (`platform::double_click_size`: `SM_CXDOUBLECLK` by `SM_CYDOUBLECLK` device px,
  half each way; 4 x 4 off Windows), stopping at 3.
- **Damage:** `Cx::request_redraw(rect)` rectangles are joined into one; the frame draws that
  rectangle (the painter is clipped to it) and presents only it with `present_with_damage`
  (the whole buffer when its age is not 1). Resize, scale, a change of the backdrop or its
  palette (`Cx::set_backdrop`), and the window being seen again repaint everything.
- **Hidden windows:** a window covered (`Occluded`) or minimized is not drawn, and the
  animator's deadline does not wake it; timers still do. winit 0.30.13 never sends `Occluded` on
  Windows, so a minimized window is known by a `Resized` of zero width or height, or
  `is_minimized() == Some(true)`, and `occluded || minimized` is what every check reads.
- **One wake-up:** the animator's `end_frame` deadline (a caret's `blink_caret`, anything in
  motion) and the application's `Cx::wake_at` timer fold into one `WaitUntil`; with neither the
  loop waits. At the animator's deadline the host redraws what the frame said was moving
  (`Cx::animating`), else the whole window; at the timer the application gets `Event::Wake`.
  `Cx::wake_at` works from any call; an `Animator::wake_at` asked outside a draw waits for the
  next frame, as `Cx::anim`'s doc says.
- **Every animated value on every frame.** A partial frame still ends the animator's frame, and
  `end_frame` forgets the names it was not asked for. The host keeps it so, because the
  deadline of the next frame comes only from `end_frame` and the commonest partial frame (a
  caret's blink, which draws only the field) needs it; `App::draw` and the module doc say that
  every animated value is asked for on every frame whatever the damage (or named with
  `Animator::keep`).
- **Workers** send with `Proxy::send` (from `Cx::proxy`), which is `EventLoopProxy` underneath.
  After `Cx::exit` what arrives is dropped.
- **The window:** undecorated, a minimum size, the system shadow on Windows, the dark title-bar
  mode and corners from the config; the 8 px edge band of a restored window resizes it
  (`window::edge_at`, `drag_resize_window`) with the resize cursors, except over the snap
  rectangle and the places the application gives `Cx::set_no_resize` (a scroll bar along the
  right edge), and not before the pointer has been seen (it starts at 0,0, the corner); the
  title row is the
  application's (`Cx::drag_window`, `title_double_click`, `toggle_maximize`, `minimize`);
  snap layouts by `Cx::set_maximize_button`, re-sent after a change of scale.
- **Backdrop, Windows only:** `Cx::set_backdrop(Backdrop::Mica, &pal)` runs `frost::Frost` (so
  `platform::set_backdrop`) with `tokens::allowed_backdrop(want, high_contrast)`, and Frost
  still refuses it when the system shows none; `Cx::frost(p, rect)` marks chrome for it within
  the damage. The host reads high contrast at start, on `ThemeChanged`, when the window gains
  the keyboard, and when `platform::watch_contrast` wakes it (the host takes that one wake; an
  application of the host does not call it), and sends `Event::Contrast` on a switch, and on
  every wake of the watch while high contrast is on (one contrast theme for another, or the
  theme's colours arriving after the switch, which the watch reports apart). Off
  Windows there is no backdrop: stock softbuffer presents Xrgb8888, so the keyed-out chrome
  showed black on KDE Wayland; the host reports none (`Cx::frosted` is false) until it presents
  with alpha, and does not read the contrast there (the portal read can wait half a second).
- **First frame:** created hidden, drawn and presented, shown, presented again. mdview's rules;
  no DWM cloak, as mdview uses none. It is drawn as focused (winit's `with_active`, true by
  default, activates a window when shown), so the chrome does not flip at launch; if the window
  has no keyboard once shown, it is drawn again unfocused and the application gets
  `Focus(false)`.
- **Accessibility** (`host,a11y`): the tree's types (`accesskit`) are in plain `[dependencies]`
  and build everywhere, so `App::accessibility`, `Event::Action` and `access_rect` exist on every
  system; the adapter (`accesskit_winit`) is Windows-only. The host owns an `a11y::Tree`: the
  application adds its nodes in `App::accessibility(cx, tree)` and answers the focus, and the
  host begins and ends the tree, so AccessKit gets only what changed and the activation handler
  is served the whole tree. On Windows `Adapter::with_mixed_handlers` is made before the window
  is shown. The nodes are asked for once after the first frame, before the window is shown, then
  only while an assistive technology listens: after a call that asked for a redraw or said
  `accessibility_changed`, after a resize or a change of scale, after the frame when the call is
  followed by one (so a layout made in `draw` is read), and after a frame whose `draw` said
  `accessibility_changed` (an animation that moved something). A redraw asked for inside `draw`
  does not count. Nobody listening, nothing is built: one who arrives is served the last tree
  and the nodes are asked for again at once; asked before the first build, the handler answers
  nothing and the loop builds the first tree. Elsewhere the tree is not asked for.
- **What is not reduced** (touch, pen, key releases, a dropped file) reaches `App::raw` as
  winit's own event before the host's work. Touch is not mapped to the pointer: whether Windows
  also sends a winit window the mouse messages it promotes from touch was not tried. A wheel's
  `precise` is true for pixel deltas and for line deltas that are not whole notches (a precision
  touchpad on Windows). Motion named wholly outside the window redraws the whole window, so the
  animation's deadline is not lost.
- **A UIA client** finds two top-level windows of the process: the window, and winit's
  `Winit Thread Event Target`, a hidden helper with no name. A rig that finds the window by
  process id leaves that class out, or finds it by name.
- **Checked by hand after the audit (2026-11-25):** a UIA probe on the example, three runs: the
  window, the button by AutomationId `Hello` and the field are found with bounds where they are
  drawn, and Invoke arrives as a Click, also when the probe connects after the tree went stale.
  At scale 1 only. CI's Windows job runs the example for two seconds. The Cargo lines follow `wk/a11y`'s layout (its working
  copy of 2026-11-25), so the two merge easily; softbuffer 0.4.8, already in the lock, became an
  optional dependency.

Deliberately not here (row W1b): tabs and several windows, the Linux client-drawn shadow and
corners, a backdrop off Windows (it needs a present with alpha), macOS specifics, key releases,
regions with their own damage.

Run: 34 unit tests of the pure parts with `host,a11y` (33 with `host`): the damage join
against a plain min and max over random rectangles, the outward device rounding against fine
sampling, the folding of an animation's or a blinking caret's deadline with a timer, the covered
and minimized window (a minimized window wakes for timers only and comes back drawn whole), the
click count with its cap and the double-click rectangle, the reduction of positions, buttons and
wheels, the copy into the buffer, nonsense input. The example `examples/host.rs` (a title row
over Mica, a button with a tooltip after 600 ms, a blinking field, a worker ticking every second)
was compiled and run headless on Windows with `--seconds N`, after the rebase onto `feat/anim`:
a full first frame, then frames of only the footer (24,180 600x24) on each tick and only the
field (24,128 320x32) on each caret change, and the close on the timer. In one run the window
did not get the keyboard at launch: it printed `focus false` and drew once more, whole. In
another a script minimized it for 2.5 s with `ShowWindow`: the ticks kept coming and no frame
was drawn until the restore, which drew one whole frame. With `host,a11y`, a UI Automation
script found the window (framework `mdshared-ui`), the button by AutomationId `Hello` and the
field as an Edit, and its Invoke reached the app as a Click. **Not seen by a person**: the look,
Mica and frost, the drag, resize and snap, the tooltip (no pointer rested on it), the
first-frame rules (no white flash, no flip). Linux (`host` and `host,a11y`) and macOS were only
type-checked; the KDE Wayland fix (no backdrop off Windows) was not run there.
