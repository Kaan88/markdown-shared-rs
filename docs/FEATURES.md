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
  is called once at once). `system_is_dark` had the same blocking shape; it was given the same
  treatment later (roadmap X9): the last answer is kept, only the first call waits (up to the old
  500 ms), and `platform::watch_system_dark(wake)` signals changes.

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

## A menu's look (`menu::Look`, 2026-11-24, `wk/menulook`, roadmap L11)

mdplay's toolbar is flat, and the rounded, shadowed drop-down clashed with it. The look of a menu
was eight numbers written into `menu.rs` and `submenu.rs`; they are now the fields of
`menu::Look`, which a menu takes by `Menu::with_look` (or `set_look`, for a menu an application's
own code made) and a cascade by `Cascade::with_look`, for every menu it opens:

| Field | `ROUNDED` (the default) | `FLAT` |
|---|---|---|
| `radius`, the menu's corners | 8 | 0 |
| `shadow` | yes | no |
| `row` | 32 | 26 |
| `pad`, above the first row and below the last | 5 | 4 |
| `inset`, from the menu's side to a row's and its highlight's | 5 | 1 (inside the line) |
| `hover_radius` | 5 | 0 |
| `separator` | 9 | 9 |
| `gap`, from a menu to its submenu | 2 (`submenu::GAP`) | 0 |

The struct is `non_exhaustive`, so an application starts from either and changes fields, and a
field added later breaks none. `Look::sane` puts back the rounded look's own value for any size
that is not finite, is negative or is out of reason, so a look read from settings cannot break a
menu. `row_span` and `submenu::beside` keep their signatures and meaning (the rounded look);
`row_span_with` and `beside_with` take a look. The colours stay the palette's.

Compatibility: a menu given no look is drawn with `Look::ROUNDED`, whose values are the old
constants; the existing menu and submenu tests, which pin the rows, the placement and the
pixels, pass unchanged. A test draws a menu with no look and with `ROUNDED` and finds the same
pixels. The menu tests moved to `menu_tests.rs` (the file would have passed a thousand lines).

What was run, on Windows: the nine commands of AGENTS.md, and after the last change (set_look) fmt, the workspace's clippy and mdshared-ui's tests again; two mutants (the shadow drawn whatever
the look, the submenu's gap ignored) each failed a new test. The sheet
`menu_look_sheet` (target/specimen/menu-look.png, -150) shows the rounded, a soft and the flat look,
light and dark. mdplay's mockup draws its menu bar, toolbar menus and right-click menu in three
looks built on it.

Not verified: seen by a person (the owner has not looked yet); Linux and macOS (CI not run);
mdview and mdedit were not rebuilt against the branch (they name no look, so nothing of theirs is
drawn differently).

## The look as a user setting (`appearance`, joined menus, 2026-11-24, `wk/menulook`, roadmap L12)

mdplay makes its whole look a setting (its D-024): a colour scheme, where the accent comes from,
the corners, the shadows, the menus' rows, menus joined to their control, the speed of motion.
The parts any application can use are here, so mdview, mdedit, Hub and mdmix can offer the same
choices with a few lines. Three pieces, each opt-in:

- **`appearance`**, the look as plain data. `Appearance` has the fields `scheme`
  (`Scheme::{Dark, Light, Warm, Cool}`), `accent` (`AccentFrom::{Theme, System, Cover,
  Custom(rgb)}`), `radius`, `shadow` (1 is the toolkit's strength, 0 none), `menu_row`,
  `joined`, `motion` (`Speed::{Off, Snappy, Fluent, Soft}`, factors 0, 0.66, 1, 1.5) and
  `submenu_delay_ms`. `Appearance::DEFAULT` is the toolkit's own look (DARK, its accent,
  `Look::ROUNDED`, Fluent, `submenu::DELAY`), so opting in changes nothing until the user does.
  `WARM` and `COOL` are two new light palettes taken from mdplay's prototype.
- **`Painter::shadow_with`** and **`menu::Look::shadow_strength`**: the popup shadow at a
  strength; `shadow` is `shadow_with` at 1, pixel for pixel.
- **`menu::joins`, `join_under`, `join_over`**: a menu drawn joined to the control that opens it,
  the control a tab of the menu (the popup's colour, its line round the tab's top and sides, the
  seam opened, the menu's corner squared under a tab at its edge, and a concave curve of up to
  `menu::FILLET` px where the tab's side meets the menu's top inside its width).

### Adopting it

```rust
use mdshared_ui::anim::{Motion, fluent};
use mdshared_ui::appearance::{self, Appearance};
use mdshared_ui::{menu, platform, submenu};

const NAME: &str = "myapp-look"; // one word: whose text it is
const VERSION: u32 = 1;

// At start: the text is one value of the settings file (unescape it if it was escaped).
let mut look = Appearance::DEFAULT;
let mut own_keys = Vec::new();
if let Some((_version, pairs)) = appearance::parse(&text, NAME) {
    own_keys = look.read_all(pairs); // the pairs that are not the look's: the app's own
}

// The palette: the scheme's (or the app's own for a scheme it draws itself), with the accent.
let system = platform::system_accent(); // ask again on ThemeChanged or when focus returns
let cover = cover_colors.map(|c| c.accent); // cover::colors, once per picture
let pal = look.palette(&look.scheme.palette(), system, cover);

// Menus and cascades take the look; the cascade also takes the delay.
let mlook = look.menu_look();
let mut m = menu::Menu::new(items, 0.0, 0.0, false).around(control).with_look(mlook);
let cascade = submenu::Cascade::new(nodes, control)
    .with_look(mlook)
    .with_delay(look.submenu_delay());

// A joined menu, each frame it is open.
m.place_in(&mut p, area);
let joined = look.joined && menu::joins(control, m.rect());
if joined {
    menu::join_under(&mut p, &pal, &mlook, control);
}
m.draw_in(&mut p, &pal, area);
if joined {
    menu::join_over(&mut p, &pal, &mlook, control, m.rect());
}
// ...then the control's label over it, in pal.text.

// Motion: the animator's policy, and the Fluent durations at the user's speed.
anim.set_policy(look.motion.policy());
let enter = Motion::tween(look.motion.scale(fluent::FAST), fluent::DECELERATE);

// On a change: apply it as above, then save the look and the app's own keys in one text.
let mut pairs = look.write();
pairs.extend(own_keys.iter().cloned());
let text = appearance::to_text(NAME, VERSION, &pairs);
```

The text is a header line `name version` and a `key value` line per pair; values are escaped as
the settings file escapes them (`%`, line breaks), so any text fits. `appearance::KEYS` lists
the look's keys and values. `parse` never panics: it refuses a text over 64 KiB or with another
header, skips lines it could not have written, reads at most 256 pairs, and takes CRLF, a
byte-order mark and blank lines; `read` leaves a field when its value does not read and holds a
number to its range.

What stays the application's own:

- **Its palettes.** `Scheme::palette` is the toolkit's; an application with a dark of its own
  (mdplay) passes that as `base` for `Scheme::Dark`. `palette` only puts the accent in.
- **Its keys.** Anything else it wants in the same text (columns, a layout) goes into the pairs;
  `read_all` hands them back.
- **When to ask again.** The system's accent (`platform::system_accent`) when the system says
  its colours changed or the window regains the focus; the cover's accent once per picture.
- **The preferences page.** `Scheme::ALL`, `AccentFrom::SOURCES`, `Speed::ALL` and their
  `label`s fill a `settings::Form`; the ranges are `MAX_RADIUS`, `MAX_SHADOW`,
  `MIN_MENU_ROW`/`MAX_MENU_ROW`, `MAX_SUBMENU_DELAY_MS`.
- **Where its menus open.** `Menu::around(control)` opens flush under the control, which a joined
  menu needs; an application that wants a gap when menus are apart anchors to a rectangle a few
  px taller.

Not there: dark warm or cool schemes; fonts and text sizes (`tokens::Sizes`); high contrast
(`tokens::Tokens::from_system`, which an application in high contrast uses instead); acrylic
menus and the prototype's "unfold" entrance; a joined submenu or a menu joined above its
control (a flipped menu does not join, and `join_over` answers false); the tab fading with its
menu (the caller draws both inside one `anim::pop`).

`menu_look` maps the numbers onto `Look::ROUNDED`: this radius, row and shadow strength (no
shadow at 0), the highlight's corners two px less than the menu's and at most 5 (the prototype's
rule, and `DEFAULT` gives `ROUNDED` itself), the flat look's full-row highlight at radius 0; the
padding and gaps stay the rounded look's. The join differs from mdplay's mockup in two places: the
squared corner paints only the pixels the rounded fill did not cover, so a lit first row is kept,
and the curve takes the tab's own side line into it instead of standing beside it.

Compatibility: nothing existing changes. `Painter::shadow` draws what it drew (a test against a
copy of the old loop, light and dark, at scale 1 and 1.5); `Look::ROUNDED` and `FLAT` gain
`shadow_strength: 1.0` (the struct is `non_exhaustive`); everything else is new.

What was run, on Windows, on 86afe1b with these docs: the nine commands of AGENTS.md, all
passing (the workspace's tests 1248 passed, 0 failed, 22 ignored; the feature runs recycle 11,
media_session 7, notify 11, none failed);
mutants that drew the menu's shadow at strength 1 whatever the look, filled the squared corner
whole, left out the curve, dropped the width test of `joins`, let a cover source fall back to
black, dropped the text's size limit and left `read` unclamped each failed a test. The sheet
`appearance_sheet` (target/specimen/appearance.png, -150) draws the four schemes with a menu
joined to its title, in the toolkit's look and in mdplay's prototype's; the agent looked at it.

Not verified: seen by the owner; Linux and macOS (CI not run); used by an application (mdplay's
port is its own work); mdview and mdedit not rebuilt against the branch (they name none of it).

## Soft shadows and the settings form's style (`Painter::soft_shadow`, `settings::Style`, 2026-11-25, `wk/menulook`, roadmap L12)

mdplay's look is drawn after its HTML prototype, whose shadows are CSS `box-shadow`s. The
toolkit's shadow (`shadow_with`) is ten faint boxes, each blended and rounded down, so on a dark
page the stack sinks to black next to the box (the Look dialog on mdplay's dimmed page had 9 px
of pure black under it), and it reaches up about as far as down. These pieces draw what the
prototype draws; each is opt-in.

- **`Painter::soft_shadow(r, radius, &layers, k)`**, a CSS `box-shadow` in software. Each
  `Shade { dx, dy, blur, alpha }` is the box with its rounded corners blurred by a Gaussian of
  sigma `blur / 2` (as browsers draw it), computed exactly (the normal distribution across and
  down, less each corner's notch in strips), offset, its alpha times `k`; the layers stack as CSS
  stacks them, and each pixel is blended once and rounded, so a tail too faint to show leaves a
  dark page as it was. Outside the box only, as CSS; the box is drawn over it next. Recipes:
  `Shade::POPUP` (`0 8px 24px 55%, 0 2px 6px 35%`, the prototype's menus) and `Shade::TAB`
  (`0 2px 10px 45%`, a joined menu's control); `LIGHT_SHADOW` (0.45) is the prototype's factor
  on a light page; `soft_bounds` says how far it reaches (three sigmas). It honours the clip and
  the offset, the shadow at 150% is the one at 100% made larger, and nonsense (a number that is
  not finite, `k` of 0 or less) draws nothing.
- **`menu::Look::soft_shadow`**: the menu casts `Shade::POPUP` at its `shadow_strength` (times
  `LIGHT_SHADOW` on a light palette) instead of `shadow_with`, its joined control `Shade::TAB`,
  and `draw_in_presence` fades the whole shadow with the menu. Off in `ROUNDED` and `FLAT`.
- **`overlay::draw_dialog_card_with(p, pal, r, radius, soft)`**: the dialog card with other
  corners and, given `Some(k)`, the soft popup shadow at `k`; `draw_dialog_card` is it with 10
  and `None`.
- **`settings::Style`**, given by `Form::style` (or `set_style` while the form is open, for a
  look changed under it): the card's `radius` (out of 0..=32 it is 10), its `soft_shadow`,
  `neutral_segments` (the chosen segment of a segmented choice a grey pill, white on a light
  palette, on a track a shade off the card, instead of the accent) and `caps_headings` (11 px
  small capitals in the muted colour). `Style::CLASSIC` is the form as it was, and what a form
  given none draws. The struct is `non_exhaustive`: start from `CLASSIC` and change fields.

Against the browser: Edge's own `box-shadow`, rendered headless at 100% and 150% and read pixel
by pixel, is within 3 to 4 / 255 of the formula these follow (`soft_tests` holds the formula
against a brute-force 6 x 6 supersampled blur, within 2 / 255).

Cost: two numbers per layer for every column and row in reach, worked out once a call, a product
per pixel, and a few strips at the corners. mdplay's frames on the release profile, three runs
each: its Look dialog at 1650 x 1050 (150%) 5.7 to 6.1 ms against 5.55 with the old shadow, at
100% 2.55 to 2.80 against 2.57; its menus 1.15 to 1.25 against 1.22 to 1.28.

Compatibility: nothing existing changes. `ROUNDED` and `FLAT` gain `soft_shadow: false` (the
struct is `non_exhaustive`); `draw_dialog_card` is `draw_dialog_card_with(.., 10.0, None)`, and the
test that pins its pixels to a copy of the old loop passes unchanged; a form given no style draws
the same pixels as one given `CLASSIC` (a test, light and dark, at 1 and 1.5); every other
existing test passes unchanged.

What was run, on Windows, on 9bebfcd..b1211de with these docs: the nine commands of AGENTS.md,
all passing (the workspace's tests 1282 passed, 0 failed, 23 ignored; the feature runs recycle
11, media_session 7, notify 11, none failed); the new tests also in a debug build. Mutants that
drew the menu's or the tab's soft shadow as the old one, kept the old snapshot reach, turned the
soft shadow on in `ROUNDED`, dropped the light factor, rounded the blend down, took sigma as a
third of the blur, shadowed the corners' partial pixels whole, or ignored the style's radius,
its limits, its soft shadow, its capitals, its neutral segments or `set_style`'s answer each
failed a test. mdplay's shots show all of it (its own commit).

The review's fixes (2026-11-25): a cascade (`submenu::Cascade::draw_in_presence`,
`draw_animated`) fades and slides each soft-shadowed menu's whole shadow, as `Menu` does
(`Menu::presence_bounds`), where it took 18 px round them; a cascade without the soft shadow
takes what it took. A corner is cut into one strip per half deviation of its curve, the strips
spaced round the curve and each taking off exactly the area the curve leaves in it, where it
took a strip per pixel of radius whatever the blur: the shadow is within half a level of 255 of
a corner cut finely (strips a pixel high were up to 1.8 levels off at the dialog's corners), and
the Look dialog at 150% with its largest corners paints in 5.44 to 5.54 ms against 5.86 to 5.96
(mdplay's shots, opt-level 3 without LTO). `draw_dialog_card_with` takes a radius that is not
finite or is negative as the classic 10 for the shadow, the card and its footer alike. A form
in the classic style is held to the frozen copy of the form before styles, pixel for pixel.

Not verified: seen by the owner; Linux and macOS (CI not run); Wine; mdview and mdedit not
rebuilt against the branch (they name none of it).

## A menu's text size and one lead column, a lighter backdrop (`menu::Look`, `settings::Style`, 2026-11-25, `wk/menulook`, roadmap L12)

Measured against mdplay's HTML prototype, a web page's menus put the check mark and the icon in
one 30 px column with the label after it, at 13 px, where the toolkit's have a column for each at
13.5 px; and the prototype's settings panel does not dim the window as much as a modal dialog.
Each piece is opt-in.

- **`menu::Look::text`**: the labels' size in logical px; the menu is measured at it, so its width
  follows. `TEXT` (13.5) in `ROUNDED` and `FLAT`; out of 6..=48, or not a number, `sane` gives the
  default's.
- **`menu::Look::one_lead`**: the check mark and the icon share one `ONE_LEAD` (30 px) column from
  the row's left side, a checked item showing its mark there and any other its icon (muted); the
  label starts after it. A menu with neither marks nor icons starts its labels `NO_LEAD` (14 px)
  in. Off in both looks.
- **`settings::Style::backdrop`**: how much of the classic backdrop dims the window behind the
  card, 1 the classic `draw_backdrop`, 0 none, between them `draw_backdrop_presence` at that
  part. 1 in `CLASSIC`; not a number is 1.

Compatibility: nothing existing changes. `ROUNDED` and `FLAT` keep 13.5 and separate columns, and
`CLASSIC` the full backdrop; every existing test passes unchanged. The new tests fail with each
piece reverted (the lead's position, the text drawn and measured at the look's size, the
backdrop's part).

Not verified: seen by the owner; Linux and macOS (CI not run); Wine.

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

The list does not cover its field unless not even one row would fit beside it. It is placed as `Menu::around` places a menu, but where it fits
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
  Tested on a manual clock; `examples/host.rs` blinks its field's caret through it, run
  headless in CI only, so no person has watched it blink.
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
A paragraph that cannot be edited but can be selected, for the paths, command lines and registry
keys in Purge's details pane. It is one paragraph of the size such a pane shows, a few lines to a
few hundred: every press and drag is given the lines of the whole paragraph (`paragraph_lines`,
linear in the text, about 2 ms for 100,000 chars in release), `draw` builds them on every frame
that shows a selection, and nothing scrolls or is virtualised. It is not a log view; output that
grows without end belongs in a list of rows. `SelectableText` holds the text, an anchor and a caret as byte offsets on char
boundaries, and the drag in progress; `draw` paints the selection behind the paragraph it draws.
No editing, no caret, no clipboard and no clock: Ctrl+C answers `Action::Copy(text)` and the
caller puts it on the clipboard, and the context menu's items carry the same `Action`s.
The lines come from the new `Painter::paragraph_lines`, which reads the parley layout
`paragraph` already builds and returns each line's byte range, its y and height, and the x of
every char boundary from the same run advances. Hit testing and the highlight therefore use the
drawn glyph positions rather than a second shaping of the text. The model keeps the lines last
given to `press` or `drag` for the keys; the caller must give it fresh ones with `set_lines` after
a resize or a font change.
Two clicks select a word with the classes `textfield` uses; three select the whole logical line
between its newlines, not one wrapped piece (a whole path or registry value). Shift with the
arrows, Home, End and Up/Down extends the selection, and Up and Down keep the x they started
from. Alt is refused.
Run on Windows: `cargo test --release -p mdshared-ui` (seltext's tests, 28 then and 31 after the
audit's fixes below, its ignored sheet, the new gfx test and the rest of the crate), `cargo fmt --all --check` and the workspace's clippy
with warnings as errors. The pixel test draws a selection in both palettes at scale 1 and 1.5 and checks the
highlight under the selected space and the page under an unselected character.
Not verified: a real pointer and keyboard in a window; a person looking at
`target/specimen/seltext.png`; right-to-left paragraphs (their stops follow parley's runs but no
test exercises them; the keys move in logical order, and the one highlight rectangle per line is
wrong on a line that mixes directions); Linux and macOS (not run here). The UI Automation Text
pattern is not done.

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

The audit of the Purge fast track (2026-11-25) found more, fixed in a later commit:
- A path could not wrap. `Painter::paragraph` breaks between words only, so
  `C:\Program Files\Mozilla Firefox\...` or a registry key in a narrow card ran past the edge,
  where the clip cut it off out of reach of the pointer; that is P7's main use. `gfx` gained
  `Wrap::{Words, Anywhere}` and `Painter::paragraph_wrapped`, `paragraph_height_wrapped` and
  `paragraph_lines_wrapped` beside the old three, which now call them with `Wrap::Words` (a test
  checks their pixels, height, lines and cache entry against a copy of the old code; the layouts
  of the two modes are cached apart). `Anywhere` sets parley's `OverflowWrap::Anywhere`: a word
  that does not fit a line of its own breaks between two clusters. `SelectableText::with_wrap`,
  `set_wrap` and `wrap` choose it (the default is `Words`, as before), and the new
  `seltext::lines(p, pal, width, face, size, &model)` gives the lines `draw` draws, so a caller no
  longer has to repeat the colour and the wrap. Copy gives the text as it is, with no line break
  where it wrapped. A pixel test draws a registry key in a 130 px area, light and dark at scale 1
  and 1.5: it breaks into lines inside the area, the clip cuts nothing (the drawing equals the
  unclipped one, which has no ink right of the area, where breaking by words has), and a
  selection across the break copies the original text.
- On a line that is only a `\r\n`, Right and Left stood between the `\r` and the `\n` (parley
  marks both as edges there), and without lines the keys stepped into every `\r\n`. Both now
  step over it whole; a test walks Right and Left across three CRLFs, with lines and without,
  and checks End and triple clicks on them.
- The test of `paragraph_lines`' speed asserted 100 ms in release, which a slow runner may miss.
  It now takes the least of five timings at 20,000 and 100,000 chars and checks that five times
  the text takes at most twelve times as long (a quadratic pass takes 25 times), with 2 s as the
  bound for a hang.
- The module doc says what size of text it is for, and what right-to-left text does.

## One instance per session with no socket (`mdshared_app::single_mutex`, P8, Windows)
`mdshared_app::single` keeps one instance through a loopback TCP port, which Purge, an
uninstaller that runs elevated, must not open. `single_mutex` does it with kernel objects only,
which go away with the last process that holds them however it ended. The first instance creates
the mutex `Local\mdshared-single.<app id>.mutex` and keeps the `Guard`; it writes its process id,
and later its main window, into a 16-byte named file mapping beside it (the record). A second
launch finds the mutex there, reads the record, finds the window, hands it its command line in a
`WM_COPYDATA` and brings it forward. `Local\` keeps each session (console, Remote Desktop, fast
user switching) to its own instance.

API: `acquire(app_id) -> Instance::{First(Guard), Running(Running)}`; `Guard::publish_window(hwnd)`
(0 takes it back) and `held()`; `Running::pid()`, `window()` (the published window while it is
still one of that process, else the first visible, unowned, non-tool top-level window of the
process) and `wait_window(timeout)` (waits for the published one only, every 20 ms, and looks
through the process once at the deadline; `None` at once if the instance went);
`bring_forward(hwnd)` (`AllowSetForegroundWindow`, `ShowWindowAsync(SW_RESTORE)`,
`SetForegroundWindow`); `send(app_id, hwnd, &args)` with a 2 s limit (`SEND_TIMEOUT`); the unsafe
`received(app_id, msg, lparam)` for the window procedure; `windows_of(pid)`, `main_window_of(pid)`;
`MAX_BYTES` (1 MiB). The module is compiled on Windows only.

What an application does with it is in the module doc, "When nothing is handed over": when
`wait_window` gives `None` or `send` gives false, call `acquire` again (the instance may have
gone) and otherwise tell the user in a window of its own, or run anyway; never exit without a
word. `send` false after its 2 s does not mean the message was lost (Windows still delivers a sent
message to a thread that was busy), so a launch sends once: a retry can hand the same command line
over twice. No application uses the module yet, and a winit window does not receive the message
until its procedure is replaced (roadmap P8b), so for a winit application `send` gives false
today.

Not prevented, only not made worse: a process of the same user that creates the mutex or the
record first (a squatter). A first instance that finds the record's name taken tries again every
5 ms for 100 ms, since an honest mapping can be open for a moment (the instance before it closes
its mutex before its mapping; a second launch reading the record holds it open); a name still
taken after that is left alone, so the instance runs with no record, and a later launch learns
that the application runs and gets no process id and no window.

Run on Windows: `cargo test --release -p mdshared-app` with the module's 16 tests: two guards in
one process, the record, a record name held open for a moment (and the retry failing it when the
patience is zero, tried by hand) and one held by a squatter, the waits (gated by channels with
time limits, not sleeps), hidden windows of the test's own and one that counts as visible but is
layered, off screen and without a taskbar button, a `WM_COPYDATA` to the test's window, and a
second process (the test program started again) that finds the first, its window, and hands it
its arguments. Not verified: a real window brought forward by a launch from Explorer (it needs a
person at the desktop; the tests bring nothing forward), a winit window receiving the message
(P8b), an elevated first instance with a plain second launch, and `send` timing out against a
hung window. Not seen by a person.

## Accessibility for UI Automation (`mdshared_ui::a11y`, feature `a11y`, roadmap W8 at Purge's scope)

An AccessKit tree built from the same models that draw, served to UI Automation on Windows by
`accesskit_winit`; the module doc says how it is built (ids from author ids, every row of a list in
the tree, bounds cut to the viewport, updates of only what changed). The consumer is
`examples/purge_shape.rs`, driven by `tests/uia/purge_shape.ps1`. These are the limits and the
choices a rig or a reader meets, as found by the Fable audit of the Purge fast track and fixed or
written down after it (2026-11-25):

- **Several rows cannot be selected through UIA.** AccessKit 0.25.2 turns SelectionItem's
  `Select`, `AddToSelection` and `RemoveFromSelection` all into the same `Click`, so the window
  cannot tell them apart, and nothing here pretends to. A multi-select list still says so
  (Selection's CanSelectMultiple is true), and `Select` gives one row. A rig selects several rows
  as a person does: Ctrl+click on each, or Shift and the arrows, typed into the window with
  SendInput.
- **A row out of view has an empty rectangle, and IsOffscreen false.** Its bounds are left out so
  that it cannot take hits meant for the toolbar or the footer; AccessKit does not set
  IsOffscreen, so UIA reports false for every node. A rig that needs a row on screen calls
  ScrollIntoView (or SetFocus: in `purge_shape` a Focus request on a row moves the cursor there
  and scrolls it into view, as the keyboard does) and waits until the rectangle is not empty.
- **For Narrator:** `a11y::switch` carries the role description "toggle switch" (UIA
  LocalizedControlType), since AccessKit gives a switch UIA's Button control type. `a11y::tab`
  takes its place in the set and `a11y::tab_list` carries the count, so a tab says "2 of 4" as a
  list row does. A dialog's body is a `text` child before its buttons (`ConfirmBody` in
  `purge_shape`), and also its description (FullDescription); with only the description a reader
  that walks the dialog misses it. A field's hint is its placeholder (`Node::set_placeholder`,
  UIA HelpText): `purge_shape`'s search field gives "Search (Ctrl+F)". `a11y::column_header` makes
  a column's header (AccessKit gives it UIA's DataItem control type with the ARIA role
  "columnheader"); `purge_shape` puts "Name" and "Size" in a group above the list, and each row's
  name holds both columns ("Program 0005, 160 MB"). `a11y::clip` cuts a node's bounds to a pane,
  for the items of a scrolled pane.
- **A known cost: typing in the search.** Each key that narrows the search drops rows from the
  tree, and AccessKit's adapter handles every removed row on its own (its platform node is
  dropped and it is looked at for events to raise), so a first letter that hides most of 2,000
  rows costs that many removals. Found in AccessKit's source by the audit; not measured, and not
  redesigned (realizing only the visible rows would cost the reach the list is for).
- **Two top-level windows.** A UIA client that finds the window by process id also finds winit's
  hidden helper, class `Winit Thread Event Target`, with no name. `purge_shape.ps1` finds the
  window by its title and leaves that class out as well, and checks that the helper is there and
  is not what it found.

The script needs Windows PowerShell 5.1 (`powershell.exe`): it makes the interop assembly for
UIA's COM client with `TypeLibConverter` and `AssemblyBuilder.Save`, which only .NET Framework
has, and keeps it in `%TEMP%\mdshared-uia\Interop.UIAutomationClient.dll`. Its `-FarRow N`
(default 1500, from 40 to 2000) picks the far program; the Value section searches for that
program's name less its last two digits and works out how many programs that shows.

**C1, launch time** (2026-11-25, this Windows 11 laptop, release builds, no UIA client
connected): `tests/uia/first_frame.ps1 -Runs 10` ran `bare_window` and `purge_shape` with
`--first-frame` ten times each, alternately, after one untimed run of each. "First frame" is what
the example prints: from the start of `main` to the first frame drawn and presented into the
window while it is still hidden, and the window asked to show. It leaves out the process's start
before `main` and the moment the compositor puts the window on screen, which nothing in the
process waits for. "Launch to exit" is the script's clock from starting the process to its end,
the teardown included.

| | First frame, median | range | Launch to exit, median | range |
|---|---|---|---|---|
| `bare_window` (winit and softbuffer only) | 41.7 ms | 39.2 to 59.4 ms | 60.4 ms | 56.7 to 77.6 ms |
| `purge_shape` (the toolkit, 2,000 rows in the tree, the UIA adapter) | 63.9 ms | 62.4 to 94.4 ms | 87.4 ms | 85.3 to 121.2 ms |

So the toolkit's window costs about 22 ms over a bare window to its first frame here. In the
same session one run of `purge_shape.ps1` timed, from `Start-Process`, the bare window found by
its title after 467 ms and `purge_shape`'s names visible to UIA (UninstallButton found by its
AutomationId) after 395 ms: those include the UIA client's polling and the tree's first transfer
(2,000 rows), one run each, so they are coarse. Purge's own gate (400 ms with its real engine, on
its laptop) is Purge's to measure.

**C2, the UIA script:** `purge_shape.ps1` ran 50 checks, all passing (exit 0), with the default
`-FarRow` and again with 1234, 42 and 2000 (2026-11-25, this laptop, scale 1). It covers Invoke,
Toggle, SelectionItem, ScrollItem, ExpandCollapse, Value, RangeValue, the modal dialog, the
position in set, the hint, the column headers, focus events from SetFocus and from a Tab typed
into the window, the helper window and C3.

**C3, the size:** `purge_shape.exe` (release, feature `a11y`) is 3,631,616 bytes (3.46 MB),
under the 10 MB gate; `purge_shape.ps1` prints it and fails at 10 MB or more.

Run: 29 unit tests of `a11y` (`cargo test --release -p mdshared-ui --features a11y a11y`),
among them `clip` against a cut of each axis on its own over 2,000 pseudo-random rectangles, and
the tabs' place in the set. **Not tried with Narrator**, nor any other screen reader: what a
reader says is inferred from the UIA properties the script reads back. The look of
`purge_shape` (its column header now from `head_rect`, unchanged in place) was not seen by a
person. Linux and macOS: the tree type-checks there (the Linux clippy), nothing serves it.

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
  only once an assistive technology has asked (Windows' adapter stays active after that, for
  the life of the window, so the cost is paid from then on): after a call that asked for a redraw or said
  `accessibility_changed`, after a resize or a change of scale, after the frame when the call is
  followed by one (so a layout made in `draw` is read), and after a frame whose `draw` said
  `accessibility_changed` (an animation that moved something). A redraw asked for inside `draw`
  does not count; a redraw wholly outside the window draws nothing and the nodes are asked for
  at once. Before anyone has asked, nothing is built: one who arrives is served the last tree
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
  At scale 1 only. CI's Windows job runs the example for two seconds. The probe is
  `tests/uia/host.ps1`; it is not run by CI (it needs a desktop). softbuffer 0.4.8, already in the lock, became an
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

## The settings form in motion, and a number row as a slider (`settings::Form::draw_animated`, `show_as_slider`, 2026-11-25, `feat/l10-settings-motion`, roadmap L10)

`Form::draw_animated(p, pal, window, caret_on, anim, id)` draws the form `draw` draws, with
motion from the window's `anim::Animator` (the caller begins and ends the frame; `id` names the
form in it). A toggle row's knob slides and its fill fades in (`fluent::MOVE`, the motion of
`toggle::draw_toggle_animated`); the chosen segment of a segmented choice slides to the new
one, in the classic and the neutral look (held as a place inside the control, so a scroll is
not a motion; a label reads as chosen while the pill is under its middle); a row's hover fill
(its hairline fading out under it), a category's hover fill and the open dropdown's hover row
(`Menu::draw_in_animated`) fade by `document::HOVER`; the scrollbar widens under the pointer
(`scroll::draw_scrollbar_animated`); a slider row's thumb glides (`slider::draw_animated`).
Names in the animator are made from `id`, the row's index and the row's id, so a row replaced
by another starts at rest and two rows with one id cannot ask one name twice.
`Form::set_high_contrast(on)` makes the highlights (hover fills, scrollbar, menu row, chosen
segment) change at once in `draw_animated`; a switch's knob and a slider's thumb still move.
It changes nothing in `draw`.

`draw` and `draw_animated` share one drawing (`draw_with`); with no animator it takes exactly
the paths it took before, and the existing freezes (`settings::unchanged`, `settings::frozen`,
the style tests, the toggle's and the dropdown's frozen copies) pass unchanged. Not done: the
form has no check box, so "ticks" has nothing to animate (the dropdown's menu draws its
chosen mark as the menu draws it); the stepper's and the Close button's hover change at once.

`Form::show_as_slider(id) -> bool` draws the number row `id` (`Row::number`, with its `step`
and `unit`) as a slider instead of a stepper: the value and unit, then a 160 px track
(`Form::slider_rect(i)`) drawn by `slider::draw`. No `Row` variant and no field was added; the
value stays `Value::Number` and `set_value` clamps it as a number's. The marks are ids kept by
the form: they outlast `set_rows`, and an id with no number row marks nothing until one comes;
the call says whether such a row is there now. A form never told of a slider draws and answers
as on main. Keys: Left and Right a step (Ctrl a page), Page Up and Page Down a page (five steps
or a tenth of the span), Home and End the ends, `+` and `-` a step, each an `Outcome::Changed`
landing on a multiple of the step from the low end; Up and Down still move the keyboard, as
from every row of this form. A press on the track moves the value there at once (`Changed`)
and takes the thumb; a press on the thumb takes it where it is. `Form::pointer(x, y) ->
Outcome` is `pointer_move` with a drag's changes reported as they happen; with `pointer_move`
(a `bool`) the change is reported by `release`. A drag ends at `set_rows` (the row in hand
may now be another) and when the keys open a dropdown. The wheel scrolls the body over a
slider, as over a stepper. The paged form (`settings::pages`) draws such a row as its stepper.

The arithmetic is whole numbers in `i64`, so the keys are exact over the whole `i32` range,
and a row built by hand with `min` over `max` is read with its ends swapped instead of
panicking. The thumb's place is an `f32` fraction of the track (the slider widget on a track
from 0 to 1): a press or a drag lands on the nearest whole number on the step's grid, held
inside the ends, so over a span of more than about 2^24 a pointer cannot reach every number.

Run: `cargo test --workspace --features mdshared-ui/host,mdshared-ui/a11y` (23 tests in
`settings/motion_tests.rs`: the animated form equals `draw` at rest in both themes and under
`Policy::Never` after each kind of change; it differs in motion for the switch, a row's hover,
the segment in both looks, the slider's thumb, the scrollbar and the dropdown menu's row, and
equals `draw` again once settled; under high contrast a hover, the scrollbar and a choice are
at once while a switch still slides; a form never told of a slider draws as one without the
call, and a told one otherwise; the marks follow ids through `set_rows`; the slider's keys,
press, drag and release give the values and outcomes stated; a drag ends when `set_rows` puts
another slider at its index or a row above it, and when the keys open a dropdown; crossed ends
do not panic; a range around 16 777 217 (2^24 + 1) steps exactly by the keys and stays inside
its ends under the pointer, and the whole `i32` range does not overflow; odd windows do not
panic), `cargo clippy --workspace --all-targets --features mdshared-ui/host,mdshared-ui/a11y
-- -D warnings`, `cargo fmt --all --check`, on Windows only. `settings_motion_sheet` (ignored)
writes the slider row and a frame in motion, light and dark, at 1 and 1.5. Not seen by a person
in a running app; no application uses it yet.

## The tree's disclosure turns (`Painter::icon_rotated`, `glyph::draw_disclosure`, 2026-11-25, `feat/x3-disclosure`, roadmap X3)

The last open part of X3. The toolkit draws no tree rows: the applications draw them and swap
`glyph::RIGHT` and `glyph::DOWN` for a folder's chevron. Now the chevron can turn between the two.

- **`Painter::icon_rotated(glyph, size, cx, cy, degrees, c)`** (`gfx/turned.rs`). The glyph's
  path is turned clockwise about the centre of its square and then stroked, as `icon` strokes it,
  so it stays crisp; no bitmap is resampled. At 0 degrees, or a whole number of turns, it calls
  `icon`: the same cached mask and the same pixels. A turned mask is not cached, so an animation,
  which asks for a new angle every frame, does not fill the icon cache. The blend that `icon`
  did is now one private function both use, moved unchanged.
- **`glyph::draw_disclosure(p, cx, cy, size, open, c)`**: `RIGHT` turned by `open` times 90
  degrees. At `open` 0 or less (and NaN) it is `p.icon(RIGHT, ..)`, at 1 or more
  `p.icon(DOWN, ..)`, pixel for pixel, so an application that switches to it draws the same rows
  at rest. `RIGHT`'s points turned a quarter are `DOWN`'s, so the turn ends where `DOWN` begins.
- **`glyph::disclosure_turn(anim, id, expanded)`** and **`draw_disclosure_animated`**: the turn
  as a value of the animator under `Id::new(id).with("disclosure")`, moving by
  `DISCLOSURE_MOTION`, which is `fluent::MOVE` (167 ms, point to point, the same both ways).
  Chosen because the chevron stays on screen and moves, which is what `MOVE` is for; `ENTER` and
  `EXIT` are for things that appear and leave, and `anim::document` has no motion for a part of a
  control. 167 ms is also the expander body's time, so the two end together. Reduced motion and
  `Policy::Never` turn it at once.
- **`expander::draw_expander_turning`**, opt in: `draw_expander_animated` with the chevron
  turning. `draw_header` and `draw_expander_animated` keep the swap; they now draw the chevron
  through `draw_disclosure` at 0 or 1, which is the same `icon` call they made.

Run: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --features
mdshared-ui/host,mdshared-ui/a11y -- -D warnings` and `cargo test -p mdshared-ui --lib --features
host,a11y` on Windows (debug). The new tests: a turn of 0, -0, 360, -360 and 720 degrees is
`icon` for every glyph in `ALL`; `RIGHT` turned 90 (and -270) degrees matches `DOWN`, and 270
(and -90) matches `UP` (ink within 3%, centroid within a quarter pixel, box within a pixel, no
pixel more than 32 of 255 apart); turns of 9 to 200 degrees match a second drawing of `RIGHT`'s
points turned by hand and drawn upright (no pixel more than 4 apart) and are neither resting
chevron; turned masks leave the cache's size alone; the clip and the offset hold; nonsense
angles and sizes draw nothing. The disclosure at rest is the plain `RIGHT` and `DOWN` icons,
light and dark; between, it is `RIGHT` turned `open` times 90 degrees and neither rest. On a
`ManualClock` at 16 ms a frame: the frame that asks still shows the old state and asks for
another; the turn rises steadily and rests at 1 on the 10th to 12th frame with no frame asked;
back as long; a row first seen open is open; reduced motion and `Policy::Never` jump with no
frame asked; the turn does not touch a value under the row's own name.
`draw_expander_turning` and `draw_expander_animated` give the same pixels and body rectangles
at rest and the same frames asked, and differ on the frames of the turn. All at scale 1 and 1.5.
An `#[ignore]`d sheet, `disclosure_sheet`, writes the turn in eleven steps at 10, 12 and 16 px.
Only an agent has looked at that sheet. **Not seen by a person**, and not wired into mdview or
mdedit: no tree in a running application has turned its chevron yet. Linux and macOS: not run
(nothing here is platform code).

## The paged settings form in motion (`settings::pages::Form::draw_animated`, 2026-11-25, `feat/pages-motion`, roadmap L10)

Roadmap L10 gave the single-page `settings::Form` an opt-in animated draw, but mdview and mdedit
draw the paged form, `settings::pages::Form`, which had none. It now has the same opt-in:
`pages::Form::draw_animated(p, pal, window, caret_on, anim, id)` and
`pages::Form::set_high_contrast(on) -> bool`, in `settings/pages/motion.rs`. Where the paged
form has the single-page form's pieces it moves as that form does, through the same helpers
(`settings/motion.rs`'s `Fx`, which gained a constructor and `under(part)`, a view with every
name under one more part; nothing it did changed): a toggle row's knob slides and its fill
fades in (`fluent::MOVE`); the chosen segment of a segmented choice slides to the new one (a
disabled row's paler fill too; a label reads as chosen while the fill is under its middle);
a page's hover fill in the list fades by `document::HOVER`; the open dropdown's hover row fades
(`Menu::draw_in_animated`). Names in the animator are made from `id`, the page, the row's index
and the row's id (the list's from `id` and the place in the list), so a row replaced by
`set_pages`, or the row in the same place on another page, starts at rest, and two rows never
ask one name in a frame. Under high contrast the list's hover, the menu's row and the chosen
segment change at once; a switch still slides. A page shown is drawn at rest at once.

The paged form has no row hover fill and no scrollbar (a page never scrolls), so neither has
anything to animate; the hover of the ×, the steppers, the buttons, a text field's edge and a
dropdown's field change at once, as the single-page form's Close button and steppers do. A
number row the single-page form shows as a slider stays a stepper here. pages.rs gained only
the hooks: a private `high_contrast` field, `draw` calling a shared `draw_with(.., None)`, and
a branch on the animator at the category fill, the switch, the segmented choice and the menu.
With no animator `draw_with` takes the paths `draw` took. `settings/pages/frozen.rs` is a copy
of `draw` and `draw_row` as they stood on main (6ec1dee), for tests only.

Run: `cargo test --workspace --features mdshared-ui/host,mdshared-ui/a11y` (12 tests in
`settings/pages/motion_tests.rs`, on `anim::ManualClock` in 16 ms frames: `draw` equals the
frozen copy in 18 states (every page, a hover on every kind of target, a held press, the
keyboard's rings, an open menu with a hover row, a disabled row) in both themes at scale 1 and
1.5 and at 520 x 360; `draw_animated` equals `draw` once settled in all 18 states in both
themes, and on every frame under `Policy::Never` after each of them and after values set from
outside; it differs in motion for the switch (light and dark), the segment (light and dark),
the disabled row's segment, the list's hover coming in and going out and the menu's row, and
equals `draw` again once settled; a page shown mid-slide is at rest at once and the slide
finishes when its page comes back; a row put in a moving switch's place by `set_pages` starts
at rest; under high contrast the list's hover, a segment and the menu's row are at once while a
switch slides; odd windows (empty, 1 x 1, NaN wide, a negative height) and odd pages do not
panic), with `settings/pages/tests.rs`, `settings::unchanged`, `settings::frozen` and the style
tests unchanged and passing; `cargo clippy --workspace --all-targets --features
mdshared-ui/host,mdshared-ui/a11y -- -D warnings`; `cargo fmt --all --check`; on Windows only.
Found, not changed: `pages::Form::draw` (on main as now) overflows an integer in `gfx.rs` in a
debug build when the window is infinitely wide. Not seen by a person in a running app; mdview
and mdedit do not call it yet.

## Number rows dragged left and right (`settings::Form::make_draggable`, 2026-11-25, `feat/drag-numbers`, roadmap L10)

Asked for by mdplay for its Look dialog, which is the single-page `settings::Form`.
`Form::make_draggable(id) -> bool` lets the value of the number row `id` be dragged, as the
number fields of a drawing or a sound program are, while the row stays its stepper: the − and +
buttons, the keys and the drawing are unchanged. A press on the value's text, then a horizontal
drag, moves the value one `step` for every 4 logical px right of the press (`PX_PER_STEP`), down
for every 4 px left, counted towards zero; with Shift one step per 16 px (`FINE_PX_PER_STEP`),
never a fraction of the step, so the value stays on the numbers the buttons reach. Shift
changed during the drag carries on from the value and place it had, with no jump. The value is
held inside the row's ends; the place follows the pointer, so after a drag past an end the
pointer comes back over that stretch before the value leaves it. A hand-built row with `min`
over `max` is read with its ends swapped, a step of 0 or less as 1, in `i64` arithmetic over
the whole `i32` range; a place that is not a number moves nothing. `Form::pointer` reports
each change as `Outcome::Changed` when it happens, and `Form::pointer_with(x, y, mods)` does so
with the pace following Shift; with `pointer_move` (a `bool`) the change is reported by
`release`, as for a slider. Escape during the drag puts back the value it started from
(`Changed` when it had moved) and ends it, and the release after does nothing; the release, a
new press, `set_rows` (no report: the row in hand may be another) and a dropdown opened by the
keys end it too. Other keys act as without a drag. `Form::cursor_at(x, y) -> Option<Cursor>`
answers `Cursor::ResizeEw` over a draggable value and all through its drag, `None` elsewhere,
with a dropdown or a colour picker open (its card may cover the rows), before a layout and in a form never told; the host lists it first in
`cursor::pick` and passes the result to `CursorState::set`. `ResizeEw` is winit's
`CursorIcon::EwResize`, which winit shows on Windows as `IDC_SIZEWE`; a host setting the
cursor itself loads `IDC_SIZEWE`. The marks are ids kept through `set_rows`, as the slider
marks are; a row that is also `show_as_slider` is a slider (its thumb is dragged). No `Row`
variant, no field and no `Value` was added, no timer runs and no clock is read. The paged form
(`settings::pages::Form`, mdview's and mdedit's) has no such marks: its focus, press and
pointer code is its own, it has no `pointer -> Outcome` to report a live drag through, and
mdplay does not draw it, so it was left alone. The code is `settings/drag_number.rs`;
settings.rs gained a private `Drag::Number`, a `draggable` field and one-line hooks in
`set_rows`, `open_menu`, `key`, `pointer_move`, `press` and `release` (and `pointer` in
`slider_row.rs`). mdplay passes the pointer with `pointer_move` today, so to see the value
change during a drag it calls `pointer` or `pointer_with` instead.

Run: `cargo test --workspace --features mdshared-ui/host,mdshared-ui/a11y` (12 tests in
`settings/drag_number_tests.rs`, no clock: a drag right and left moves by the step per 4 px,
for a step of 1 and of 25; the ends hold, and infinities and NaN do not move or panic; Shift
gives 16 px per step at the press and mid-drag, without a jump either way; Escape puts the
value back and reports it, under `pointer` and under `pointer_move`, and Escape with no drag
still closes; `pointer_move` reports on release; a click on the value and the buttons and keys
of a marked row answer as before; an unmarked row in forms with no mark, another row's mark
and marks on a toggle and a missing id gives the same outcomes and pixels as the frozen copy of
the old form (`settings::frozen`) through a press, four moves, Escape and a release; a marked
form draws as an unmarked one at rest, hovered and during a drag (against one given the same
value by `set_value`), light and dark; `set_rows` ends a drag and the mark outlasts it; marks
by id, rows added later, a slider wins; crossed ends, a step of 0 and the whole `i32` range
with a step of `i32::MAX`; the cursor over the value, not over the buttons, the label or an
unmarked row, held through the drag, not with a dropdown open nor before a layout), with
`settings::unchanged`, `settings::frozen`, the motion and style tests unchanged and passing;
`cargo clippy --workspace --all-targets --features mdshared-ui/host,mdshared-ui/a11y -- -D
warnings`; `cargo fmt --all --check`; on Windows only. Found, not changed: the stepper's own
buttons and keys (`number_by` in settings.rs, on main) panic on a hand-built row with `min`
over `max`, as `i32::clamp` does. Not seen by a person in a running app; mdplay does not call
it yet.

Claude Fable's review (2026-11-25) found nothing blocking. Fixed: a Shift change seen at a
pointer place that is no number no longer switches the pace without a new anchor (it jumped
back); the test that a release ends the drag now checks the value. Noted for later:
- With `pointer_move` (changes reported on release), a key during a drag and a drag back to
  the reported value can leave the app holding a value the form no longer shows; a new press
  or a key-opened dropdown mid-drag drops an unreported value. Hosts on `pointer` or
  `pointer_with` are not affected; the slider's drag has the same hole on main.
- With `pointer`, a key during a drag and a release without a move report the same value
  twice.
- A drag does not check again that its row is still draggable (`show_as_slider` mid-drag),
  and there is no call to cancel a drag whose release never comes.
- `cursor_at` answers `ResizeEw` over a draggable value during other drags (the scrollbar, a
  text selection, a slider).
- A hand-built row whose value is outside its ends snaps to the end on the first move.
- Untested: a new press and a key-opened dropdown ending the drag, other keys during a drag;
  no fixed-seed run over press, move, key and release. The `///` of `key`, `press`, `release`
  and `pointer` do not mention the drag. The release and feature runs of AGENTS.md were not
  run.

## A colour row with an in-form picker (`colour_picker`, `settings::Form::show_as_colour`, 2026-11-25, `feat/colour-row`, roadmap L10)

Asked for by mdplay: its Look dialog keeps a custom accent and each look file's
`colour.<name>` as text rows holding six hex digits. mdplay's Look dialog is the single-page
`settings::Form` (it is the form that takes `settings::Style`, which the dialog uses; the paged
form has no style), so the colour row is that form's. The paged form (`settings::pages::Form`)
was not given one: it is a separate 2100-line form with its own rows, hits and drawing, and
nothing asked for it; it draws such a row as its text field.

- **`colour_picker`** (new module, model apart from drawing): `Hsv` and the conversions
  `hsv_to_rgb` / `rgb_to_hsv` (to and from `gfx::Rgb`), `parse_hex` (six digits, either case,
  `#` or not), `HexForm::of(text)` / `format(rgb)` (a colour goes back in the form its text
  had), and `Picker`: the colour as HSV, the colour it opened with, the part with the keyboard
  (`Part::{Square, Hue, Hex}`) and the part held. `press`, `drag`, `release` on the
  saturation-brightness square and a hue strip; keys: on the square Left/Right saturation,
  Up/Down brightness, 1% (Shift 10%); on the strip Left/Down and Right/Up the hue, 1 degree
  (Shift 10), round the circle; the hex field takes the editing keys and a complete hex moves
  the markers; Tab and Shift+Tab go round the three; Enter is `Outcome::Accept`, Escape
  `Outcome::Cancel` (`cancel()` puts the opening colour and markers back); a press on the old
  swatch goes back to it. The colour is kept as HSV and the hex made from it, so a drag through
  grey or black keeps the hue (and through black the saturation); a hex typed or set keeps them
  too when it is grey or black, and the same colour leaves the markers where they are.
  `draw_picker` draws on the flyout's card (`flyout::draw_card`: the toolkit's shadow and
  popup colours), the square and the strip from a `Cache` (bitmaps per size in device pixels
  and, for the square, per hue, copied with `Painter::blit`), a white ring with a dark edge on
  the square and a white handle on the strip, readable on any colour, the old and the new
  colour side by side, and the hex in `Face::Mono`. Software painter only.
- **A strip, not a ring**, for the hue: it is the square's width under it, so the card stays
  small and rectangular; one axis is one number, so press, drag and arrows mean the same on it;
  it costs a row of pixels to compute where a ring needs each pixel's angle and two
  antialiased edges.
- **`Form::show_as_colour(id) -> bool`**, the opt-in, after `show_as_slider`: no `Row` variant
  and no `Value` variant or field; the value stays `Value::Text`. The marks are ids kept
  through `set_rows`; the answer says whether a (non-secret) text row of that id is there now.
  The row's control keeps its rectangle; a 28 px swatch takes its left end and the text field
  the rest, typing in it as before (the swatch follows; a text that is not a colour shows an
  empty swatch with a slash). A click on the swatch (on the release, as the form's buttons
  act), or Enter, F4 or Alt+Down on the row, opens the picker in a popover placed as a flyout
  under the control (above it when there is no room, always inside the window), drawn after
  everything else. While open it takes the keys and the pointer: each colour picked is put
  into the row's text at once, in its form, and reported as `Outcome::Changed` with the text
  (a press and a key at once; a drag by `Form::pointer` at once, by `pointer_move` at the
  release, as for a slider). Enter keeps it and closes; Escape or a press anywhere outside
  the card (the swatch too, which then does not reopen it) puts back exactly the text it
  opened with and reports it when it differs from what was last reported. The wheel does
  nothing while it is open. `set_rows` closes it with no report. `set_value` on the open row
  moves the markers and counts as reported. Also: `Form::colour(id) -> Option<Rgb>` (any text
  row), `colour_open`, `swatch_rect(i)`, `picker_rect`, `picker_layout`.
- settings.rs gained only hooks: the module line, one private field, `close_colour()` in
  `set_rows`, `colour_value_set` in `set_value`, the colour row's field in `text_area`, the
  swatch at the top of the text row's drawing, the picker after the dropdown menu, and one
  early return each at the top of `key`, `press`, `release`, `pointer_move` and `wheel`, plus
  one line for the swatch's hover; `Form::pointer` (slider_row.rs) one early return. Each is a
  no-op for a form never told (`ids` empty, no picker open).

Timings (release build, Windows, `colour_picker_draw_time`, the median of 50 whole frames of
an 1100 x 700 form with nine colour rows and eight toggles, the picker open over it; the
fastest and slowest in brackets): at scale 1.0, light 2.00 ms (1.94 to 3.15), dark 2.64 ms;
dragging the hue (a new square every frame) light 2.26 ms, dark 2.81 ms; at 1.5, light 4.54 ms
(4.38 to 6.90), dark 5.46 ms; dragging the hue light 5.11 ms, dark 5.79 ms. All under the 8 ms
asked for; the square is a cached bitmap, so at rest the picker costs two copies.

Run, on Windows only: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets
--features mdshared-ui/host,mdshared-ui/a11y -- -D warnings`; `cargo test --workspace
--features mdshared-ui/host,mdshared-ui/a11y` (all passed; the two clipboard tests of
`platform::explorer_tests` failed on one run and passed on the next, because another program
held the clipboard). New tests: 18 in `colour_picker_tests.rs` (every hex of a sample of
636 056 (every third value of each channel), 153 600 more with one channel at each value, and
all 256 greys come back as themselves; hex to HSV to hex is the same text in all four forms
over 20 000 colours; `hsv_to_rgb` within 1 of the textbook sector formula over 100 000
colours; press and drag on the square and the strip with the edges held; a drag through grey
and black keeps the hue and comes back; the arrows with and without Shift, round the circle;
Tab, Enter, Escape; typing a hex moves the markers only when complete, keeps the field's text,
writes the row's form; cancel and the old swatch; nonsense numbers, points and layouts; the
square's corners and the strip's hues in pixels at 1 and 1.5, light and dark; the cache draws
what a fresh one draws; the marker and the ring in pixels) and 13 in `settings/colour_tests.rs`
(a form never told, with hex text rows, draws and answers pixel for pixel as the frozen form
`settings::frozen` through hover, a click where the swatch would be, Enter, typing, Alt+Down
and the wheel, light and dark at 1 and 1.5; the marks follow ids and only mark non-secret text
rows; the swatch shows the colour and follows typing; a click opens the picker under its row,
inside the window, over the form; picking on the square and the strip reports the text in the
row's form (`FFFFFF`, `#0055ff`), at once by `pointer`, at the release by `pointer_move`;
arrows, Tab staying inside, Enter, F4, Alt+Down, Escape closing the picker and then the dialog;
Escape, a press outside and a press on the swatch cancel to the exact old text, also one that
was not a colour; typing in the picker's field moves the markers and the row, and `set_value`
moves the picker; `set_rows` closes it and the wheel waits; it opens above a row near the
bottom and stays inside a window smaller than it; odd windows and points; `draw_animated` draws
it as `draw` does at rest). `settings::unchanged`, `settings::frozen`, the style and motion
tests pass unchanged.

The sheet `colour_sheet` (ignored) writes `specimen/colour_{light,dark,hc_light,hc_dark}_{100,150}[_keys].png`
under `CARGO_TARGET_DIR` (else the workspace's `target`): the Look form with the picker open
from a click, and from the keyboard with the hue strip ringed. An agent looked at the dark and
the light ones and the high-contrast dark one: the swatch and the field sit in the row, the
popover hangs under the accent row over the rows below with the flyout's shadow, the square
runs white to the hue to black, the ring marker and the strip's handle read on the colours, the
old and new swatches and the hex in mono; in high contrast the card's edge and the ring are
clear. That is not the owner's approval: **not seen by a person**, not in mdplay, not on Linux
or macOS, not under Wine.

Claude Fable's review (2026-11-25): one blocking finding, fixed. A swatch took a press before
an open dropdown did, so a menu item lying over a swatch opened the picker instead of being
picked, and a press on a swatch did not close the menu; the open dropdown now takes the press
first (tested). Also fixed: a press in the picker clears a hex-field selection whose release
was lost. Noted for later:
- Escape after the app's `set_value` on the open row puts back the text from before the
  picker opened, and reports it.
- The old swatch in the popover gives back the colour, not the text: a row that held no colour
  is left at the fallback grey, and `#AaBbCc` comes back in small letters.
- A digits-only hex picked to digits only forgets its capitals for the next opening.
- The row's own field still draws as focused (underline and caret) under an open picker, and
  IME composition still goes to it. A person should look.
- A double click on the swatch opens the picker and cancels it at once.
- No public call closes the picker except `set_rows` (the dropdown has the same gap).
- `draw_picker` trusts a hand-built `Layout` (its fields are public); a huge one overflows or
  allocates.
- Tests: black keeping its saturation is not proved (saturation is already 0 there); no test
  has a told form with Tab order or `Form::pointer`; `Picker::form`, `Layout::hue_marker` and
  `Cache::hex_stops` have no direct test. The release and feature runs of AGENTS.md were not
  run.

## A second launch's hand-over reaching a winit window (`mdshared_ui::handover`, 2026-11-25, `feat/handover`, roadmap P8b)

`single_mutex::send` hands the first instance's window a `WM_COPYDATA`, which is sent, not
posted, so winit's `with_msg_hook` never sees it and a winit window refused it. `handover::listen
(&window, app_id, wake)` replaces the window procedure once (with `SetWindowLongPtrW`, chained
by `CallWindowProcW` as `snap` does, so either may come first): a `WM_COPYDATA` that
`single_mutex::received` takes for `app_id` is queued, answered with 1 (so `send` gives true) and
calls `wake`; everything else goes on to winit. `handover::take(&window)` gives the queued
arguments, oldest first. The entry goes with the window (`WM_NCDESTROY`). Called from another
thread, and off Windows, `listen` gives false and `take` nothing. Wanted by Barbican; Purge does
not need it.

Run: `cargo test --workspace --features mdshared-ui/host,mdshared-ui/a11y` (in `handover.rs`,
with a window of the test's own: before `listen` a hand-over is refused; after it two are
queued in order, answered and wake once each, not passed on; a second `listen` keeps the first
id; another application's hand-over passes on and is refused; the window's destruction removes
its entry); `cargo clippy --workspace --all-targets --features mdshared-ui/host,mdshared-ui/a11y
-- -D warnings`; `cargo fmt --all --check`; on Windows only. Not seen in an application.

Asked for by Purge, whose Mica and Acrylic strength is a setting and which re-keyed the frost
itself because the veil was fixed (Mica 0, Acrylic 150, `frost::veil`). `Frost::set_veil(Some(v))`
keeps `v` of 255 of the chrome's flat colour over the backdrop in place of the backdrop's own,
for Mica and Acrylic alike; `None`, the default, goes back to it. The next `sync` makes the key
with it and says the look changed when the key did. The host's `Cx::set_backdrop_veil` passes it
on and redraws the whole window on a change; the window system is asked nothing new. Without a
call nothing changes.

Run: `cargo test --workspace --features mdshared-ui/host,mdshared-ui/a11y` (in `frost.rs`: the
veil takes the backdrop's place for Acrylic and Mica, a change is a change of the look and then
settles, the same key under another backdrop is no change, `None` goes back, no key without
focus, the system asked nothing more); `cargo clippy --workspace --all-targets --features
mdshared-ui/host,mdshared-ui/a11y -- -D warnings`; `cargo fmt --all --check`; on Windows only.
Not seen on a frosted window.

## While a colour picker is open (`settings::Form`, 2026-11-26, `fix/colour-open`, asked by mdplay)

Two follow-ups to the colour row, from mdplay's Look dialog, whose picker card covers three rows:

- `Form::cursor_at` answers `None` while a colour picker is open, as it does with a dropdown
  open: the card may cover a draggable value, and a press there cancels the picker rather than
  starting a drag. It answers as before once the picker closes.
- While a picker is open the keys go to it, so a focused text field draws no caret and no
  selection; it keeps its accent underline. The selection stays in the field and shows again
  when the picker closes.

A form never given `show_as_colour` draws and answers as before. Tests (`settings::colour_tests`):
`the_rows_field_shows_no_caret_while_its_picker_is_open` (the row's pixels are the same in both
blink phases while open, and differ again after Escape) and `no_drag_cursor_while_a_picker_is_open`
(the arrow over a draggable value goes while the picker is open and comes back after Escape),
both seen failing without the change. Left for later, from Fable's review: an input-method
composition sent to the row while its picker is open is not shown; hover stays as it was when the
picker opened, as under a dropdown; the caret test does not check that the card leaves the field
uncovered. Not seen by a person.

## The pool: cancel by key, and results in the order asked for (`Pool::cancel`, `Pool::drain_ordered`, 2026-11-25, `feat/pool-cancel`, roadmap X5)

`Pool::cancel(&key)` does for one key what `set_generation` does for a generation: queued work
under it is dropped unrun, running work under it sees `Token::cancelled` and its result is thrown
away when it ends, and a result of it waiting to be drained is thrown away; other keys are left
alone, and work submitted under the key afterwards runs as usual. It says whether there was
anything under the key. `Pool::drain_ordered()` is `drain` with the results that wait ordered by
priority, highest first, then by submission (a piece submitted again under its key counts from
then), whatever order they finished in; it orders only what waits and does not hold a result
back for one still running. Nothing changes for a caller that uses neither: `drain` still gives
the order of finishing.

Run: `cargo test --workspace --features mdshared-ui/host,mdshared-ui/a11y` (in `pool.rs`: three
pieces finishing c, a, b come out b, a, c by `drain_ordered` and c, a, b by `drain`; cancel
drops a queued key unrun, tells a running one (its token says so) and throws its result away,
throws away a waiting result, leaves another key's result, answers false for a key with
nothing, and the key runs again when submitted again; the pool tests ran 30 times in a row
without a failure); `cargo clippy --workspace --all-targets --features
mdshared-ui/host,mdshared-ui/a11y -- -D warnings`; `cargo fmt --all --check`; on Windows only.

## No one-frame hover flash on the title row (`chrome`, 2026-11-26, `fix/hover-flash`, asked by md, roadmap D4)

mdview measured a one-frame flash on its caption buttons. On the first frame of a hover
(`t = 0`, the pointer already on the control), `draw_win_button_animated`,
`draw_tab_animated`, `draw_new_tab_button_animated` and `button_fill_animated` drew the full
hover look; the next frame dropped to the fade's first step. They now decide the look at the
ends by the fade (`t > 0`), not by the pointer, as `Menu::draw_in_animated`, `check` and
`dropdown` already did. A press and an open menu still show at once; a held press shows only while the pointer is on the button, also on the frame it leaves. A quiet tab hovered at
`t = 0` draws the rest look with its close button, which shows at once as before.

A window that does not animate (`Policy::Never`, reduced motion) is unchanged: there the fade is
at its end on the first frame. Test (`chrome::tests::the_animated_pieces_are_the_plain_ones_at_rest`,
extended): the first frame of a hover is not the full hover, and the caption buttons, the new-tab button and the tab (left of its label) are at rest
there; the way back starts from the full hover. Seen failing without the change. Not seen by a
person.

## A selection that follows its text, and words across a soft wrap (`mdshared_term::select`, 2026-11-26, `feat/t6-select-rewrap`, roadmap T6)

A selection already held absolute line numbers, so it stayed on its text while the screen
scrolled. It had no way to notice a program writing over the cells under it: the module doc left
that to the caller, comparing `Screen::take_dirty` with `Selection::range`, which is wrong for a
selection in the scrollback (its rows are not screen rows) and wrong for a row that was only
repainted. It now holds a 64-bit fingerprint (FNV-1a) of its own text, taken when it is made,
extended or clipped, and `Selection::is_intact(&screen)` says whether the cells still give that
text. `Selection::follow(&mut self, &screen)` is the one call after each `Screen::feed`: false
means the text was rewritten, erased or its first line is gone from the ring, and the caller
drops the selection, as terminals do; true means it is still there, also when it scrolled into
the scrollback, where its line numbers still point at its text and nothing about what it covers
changed on the way. A selection that reaches past the last line of the screen is clamped to it
and kept, which a resize does. A press that has not been dragged is empty, and `follow` says
false for it because it holds no text: the caller holds the press itself until the drag. `clip`
takes the text of an end it cuts as what the selection holds, so the part that survived a ring
drop is not mistaken for a rewrite, and a clip that cuts nothing leaves the text it holds alone,
so a rewrite is still noticed after it.

Two consequences of comparing text rather than cells, both deliberate and in the module doc: a
program that writes the same characters over them again does not drop the selection (copying
would give the same text), and a rewrite that happens to fingerprint to the same 64 bits is not
noticed (no real output does).

Words now cross a soft wrap: a double click on `~/src/a-b` on one row of a wrapped path selects
the whole path through `.rs` on the next, because the two rows are one line of text. A hard line
break still stops a word, and so does a cell of another class. `Unit::Line` already joined wrapped
rows and is unchanged; its crossing and its stopping at a hard break are now tested by name.

Test (`select::tests::a_word_crosses_a_soft_wrap_and_stops_at_a_hard_break`): a word that starts
on the first of two wrapped rows runs to the last cell of the second, from either row, and the
word after the hard break stops at it; seen failing without the change, where it stopped at the
row's last column. `a_full_row_ended_by_a_hard_break_stops_a_word_at_the_row_edge` is the same
question with the cells touching: a row filled to all 12 columns and ended by a hard break does
not join the row below, from either side; seen failing when `!prev.wrapped()` was deleted from
`word_at` (the word below reached up into the row above) and when `!line.wrapped()` was (the word
above reached down). `a_line_crosses_a_soft_wrap_and_stops_at_a_hard_break` passed before the
change as well, since the line unit already joined wrapped rows.
`a_selection_follows_its_text_into_the_scrollback`: a selection made on a screen row is intact
and followed after two more lines push it into the scrollback under the same number, and a
selection whose line the ring has dropped is not.
`a_selection_whose_first_line_left_the_ring_is_dropped_even_when_its_text_is_empty`: a line
selection of an empty line scrolls out of the ring, its text is still empty (`is_intact` says
so) and `follow` still says no, because the line is gone; seen failing when `follow` ignored
what `clip` said. `a_clip_that_cuts_nothing_leaves_a_rewrite_to_be_noticed`: a clip that cuts
nothing leaves the old text in place, so the rewrite after it is still seen; seen failing when
`clip` re-sealed on every call. `a_rewrite_or_an_erase_of_the_selected_cells_drops_the_selection`:
erasing the rest of the selected row keeps it, overwriting a selected cell, erasing its line and
clearing the screen drop it, and writing the same characters again keeps it. The two `follow`
tests fail to compile against the module before this branch, which had no `follow` and no
`is_intact`; the two new ones fail against the branch's own first commit. `a_second_click_takes_a_word_and_grows_by_words`
was changed to the new expectation: the path it double clicks is one word over the wrap.

### Known limits, for later

Not fixed here, and none of them is in the tests above:

- **A block selection of a wrapped word is a rectangle over the wrong text.** A double click with
  Alt held (a `SelectionKind::Block`) on a word that crosses a soft wrap takes `range()`'s
  smallest and largest columns, so it covers a rectangle between two rows instead of the word.
- **A wide character that wrapped early stops a word at the wrap.** When a wide character does
  not fit in the last column, that column is blanked and the line wraps; a word selection sees
  that blank cell and does not cross, although the text is continuous there.
- **`Selection::start` with a column of `END` overflows in a word selection.** `word_at` adds
  one to the column (`b + 1`) and a column of `usize::MAX` wraps in a debug build; this was there
  before the branch. No caller makes one today: a press comes from a cell.
- **A large selection is lost as soon as its first line leaves the ring.** `follow` compares all
  the text it holds, so a drop from the top of a selection that spans the scrollback takes the
  whole selection, where xterm keeps the part that is left. `clip` keeps that part, at the price
  of calling it after every feed and losing rewrite detection across the call.

Run: `cargo test -p mdshared-term` (51 unit tests and the crate's doctest pass; in `select::`
fourteen tests, the seven above among them) and the three commands below, on Windows only.
Compiled, not run, on Linux and macOS. Not seen by a person: no window draws a selection yet
(roadmap T1).

## A layout helper: rows and columns of boxes (`layout`, 2026-11-26, `feat/k5-layout`, roadmap K5, Barbican BT1)

`mdshared_ui::layout` is pure arithmetic: `Flex::row()` or `Flex::column()` with padding
(`Insets`), a gap between neighbours, a gap between lines, `Justify` along the main axis,
`Align` across it, wrapping and a pixel grid, and `layout(area, &items)` gives one `Rect` per
`Item` in their order. Nothing is kept between calls; a nested layout is a second call with one
of the rectangles as its area. It draws nothing and hits nothing.

How sizes are decided, as CSS's flexible box does it, cut down:

- `Item::fixed(px)` neither grows nor shrinks unless `.grow(w)` or `.shrink(w)` says so;
  `Item::flex(w)` has no length of its own and takes `w` shares of what is left. `.min` and
  `.max` bound the main size; when the least is more than the most, the least wins.
- Room to spare is shared among the growers by weight; missing room is taken from the
  shrinkers by weight times basis. Growing and shrinking count from the basis, not from the
  size raised to its least, as in CSS. An item that reaches a bound is frozen there and the rest
  share again (CSS's "resolve flexible lengths"). One difference: growers whose weights add up
  to less than 1 still share all the space; CSS gives them only that fraction.
- What is left when nothing can grow is placed by `Justify` (start, centre, end, space between,
  around, evenly). Items that do not fit start at the start and run past the end whatever the
  justification, so the first is never pushed out; the caller clips.
- Across the main axis an item without `.cross(px)` fills its line; one with it is placed by
  `Align` (the layout's, or the item's own). Without wrapping a cross size is held to the line.
- With `.wrap(true)` a line takes items while their natural sizes fit; an item longer than a
  line has a line to itself; each line resolves its own flexible lengths; a line is as thick as
  its thickest item, and a line where no item has a cross size shares the space left.
- Edges are snapped one by one to a grid (`.snap(step)`: 1 logical pixel by default, `1/scale`
  for device pixels, 0 for none), so neighbours share an edge and the boxes cover exactly the
  snapped length of the area: three shares of 301 px are 100, 101, 100.
- `natural(&items)` is the main length the items want (natural sizes, gaps and padding) and
  `cross_extent(main, &items)` the cross length they need at a given main length (an infinite
  main length: one line). A scroll area sizes its content with these.
- Nonsense in: negative or non-finite lengths are 0 (an item's `max` of NaN or infinity is no
  limit), lengths above `MAX_LENGTH` (1e9) are held there, an area with a non-finite place is
  empty; every rectangle out is finite with no negative size. The new enums are
  `#[non_exhaustive]`, and `Item`'s and `Flex`'s fields are private, so more can be added.

Run: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --features
mdshared-ui/host,mdshared-ui/a11y -- -D warnings`; `cargo test --workspace --features
mdshared-ui/host,mdshared-ui/a11y --no-fail-fast -- --skip explorer_tests`, on Windows only. The
37 tests in `layout_tests.rs` cover zero and negative space, least/most conflicts, shrinking,
every justification and alignment, wrapping (including an item longer than a line and lines
without cross sizes), snapping with no lost pixel (against the snapped length of the area for
500 pseudo-random splits), device-pixel steps, nonsense and huge input, a wrapping layout laid
out at its own `natural` length keeping one line (2000 cases), and the resolved lengths against
the rules they must keep (bounds, the line filled, equal growth or shrink per weight; 3000
pseudo-random lines). A pixel test draws seven flexible boxes at scale 1 and 1.5 in both
palettes and checks every device column belongs to exactly the box whose edges hold it. The
sheet `cargo test -p mdshared-ui layout_sheet -- --ignored` writes `target/specimen/layout.png`
and `layout-150.png` (a toolbar, a form row, wrapped chips, the justifications, equal shares);
I looked at the scale-1 sheet, the owner has not. Not used by any application yet; not seen in
a window.

Reviewed by `cc-reviewer` (Opus); its five blocking findings are fixed (a wrap break lost to the
float rounding of the padding, `cross_extent` of an infinite length, and three doc claims that
said more than the code did). Left for later: `layout_into` reuses the output vector but still
allocates small working vectors each call; with a snap step that is not a whole number, a box's
`right()` and the next box's `x` can differ in the last bit of a float, since `Rect` keeps a
width; CSS's rule for weights adding up to less than 1 is not followed (documented).

## A page switch that survives a resize (`PageTransition::fits`, `PageTransition::rebase`, 2026-11-26, `feat/x10-page-resize`, roadmap X10)

Until now a switch held a snapshot of the old page taken at one size, and `apply` ended it as
soon as the bitmap was another size: the frame then showed the new page plainly. An application
that wants the switch to go on now has two additions, both methods on `PageTransition` and
nothing else changed:

- `fits(&bmp)` says whether the saved old page is of the bitmap's size, i.e. whether `apply`
  would blend it. It is true when no switch runs (there is no old page to be wrong) and false
  after the window was resized either way.
- `rebase(&mut anim, old, scale)` is `begin_with` for a switch already running: it replaces the
  old page with one the application drew again at the new size, records this frame's scale, and
  leaves the animator's progress alone, so the switch goes on from where it was instead of
  starting its time again. The effect and the timing are kept. With nothing running it answers
  false and does nothing; an empty snapshot or reduced motion ends the switch; a snapshot of
  another rectangle than `apply` is given still ends it, as any mismatch does.

The frame that does it draws the old page again, laid out at the new size, takes
`Painter::snapshot` of the region, calls `rebase`, and then draws the new page and calls `apply`,
all in that same frame. An application that calls neither is as before.

Run: `cargo test --workspace --features mdshared-ui/host,mdshared-ui/a11y --no-fail-fast --
--skip explorer_tests` (in `anim/page.rs`:
`a_rebase_keeps_the_progress_and_an_empty_or_idle_one_does_not` checks `fits` idle, on the old
size and on another, that the animator's progress is the same before and after `rebase`, and the
empty, idle, reduced-motion and new-scale cases;
`an_idle_rebase_does_not_forget_a_finished_switch` runs a switch to its end and checks that an
idle `rebase` answers false and leaves the animator's finished track in place (seen failing with
the earlier body, which called `cancel` when idle);
`a_resize_mid_switch_draws_the_old_page_at_the_new_size_and_does_not_stretch_it` grows the
window from 60 by 30 to 80 by 40 half way, rebases, and compares every pixel with a switch that
began at 80 by 40 at the same moment, plus three sampled pixels: (57,10), the half-red mix of
the old page with the new one, (65,10), where the old page's fixed-width green block sits
against the new right edge (a stretched 40 px page would have it fifteen wide and reach x = 57),
and (75,35), the background outside the region. That pixel test covers `PageEffect::Crossfade`
only: the other effects keep their progress through a rebase by the same path, but no pixel test
of theirs goes through one. The first and third fail with `fits` and `rebase` neutered);
`cargo clippy --workspace --all-targets --features
mdshared-ui/host,mdshared-ui/a11y -- -D warnings`; `cargo fmt --all --check`; on Windows. Not
seen in a real window: no application or example drives `PageTransition`, so the resize path is
exercised by the tests only.

## A pseudo-terminal (`mdshared-pty`, 2026-11-26, `feat/terminal-pty-grid`, roadmap T2)

A new crate, because this is where the `unsafe` system calls for a terminal live and nothing
else should carry them. `Pty::spawn(&Command, cols, rows, wake)` starts a program with a
terminal of its own and returns at once; `take_output` gives the bytes it wrote (for
`mdshared_term::Screen::feed`), `write` queues bytes for it (keys, a paste, the screen's
replies), `resize` tells it the new size, `exit` says how it ended.

Decisions:

- **Three threads a terminal, one wake.** Read, write and wait each block in the system, so
  each has a thread; the caller's side never blocks. The wake is a plain closure, as in
  `mdshared_app::job`, and it is called once per batch: after a wake nothing more comes until
  the output was taken. A program that prints fast therefore costs one wake a frame, not one a
  read.
- **Output is held back at 4 MB** (`HIGH_WATER`). Untaken output stops the reading thread, the
  system's buffer fills, and the program waits. A flood costs a fixed amount of memory. Tested
  with about 9 MB printed while nothing was taken.
- **`exit` waits for the output.** It answers only when the program has ended, the output has
  ended and all of it was taken, so the last lines a program prints before it ends are never
  lost to a caller that closes the view on `exit`.
- **Windows: the console is closed when the program ends**, by the waiting thread. That is
  what ends the output pipe, and it ends anything else still attached to the console. The
  standard handles are passed as "none" on purpose: without that a caller whose own output is
  redirected (a test runner) hands its redirected handles to the program.
- **Dropping the `Pty` ends the program.** On Windows the console is closed and then the
  program is terminated: closing alone was measured to leave `cmd.exe` in `pause` alive for
  more than 30 seconds. On Unix the program gets `SIGHUP`.
- **The command line is built here.** Arguments are quoted by the C runtime's rules; the
  program is looked for on the `PATH` the program will get (`.exe`, `.com`, `.cmd`, `.bat`);
  a `.cmd` or `.bat` runs through `cmd.exe /d /s /c`, and an argument with a quote, a percent
  sign or a line break is refused for it, since no quoting makes those safe for `cmd.exe`.
- **Unix: the standard library starts the child** (it forks and executes safely from a program
  with threads); the only step added between the two is a new session with the slave as its
  controlling terminal. `TERM` is `xterm-256color` unless the command sets it. After the
  program ends the output is called ended 50 ms later even if a background child still holds
  the terminal open.
- **Dependencies:** `windows-sys` 0.59 and `libc`, both in the lock already, for the calls
  themselves. `mdshared-term` is a dev-dependency only (the doc example and one test).

Run on Windows 11: `cargo test -p mdshared-pty` (17 tests and the doctest), which start real
`cmd.exe` processes: output and exit code, typed input, environment and folder, the size seen
by the program before and after a resize, kill, drop, the flood, a batch file with arguments,
a `Screen` on the other side; and `cargo clippy -p mdshared-pty --all-targets -- -D warnings`.

Not verified: **the Unix side was never run.** It was compiled and linted from Windows for
`x86_64-unknown-linux-gnu`, `aarch64-apple-darwin` and `aarch64-linux-android` (roadmap T8).
Windows before 11 was not tried (ConPTY needs Windows 10 1809; before 11 22H2 closing the
console waits for the output to be read, which the reading thread does, untested). No
full-screen program (an editor, a pager) was driven through it by the tests, and nothing has
been seen in a window: that needs the grid drawing (roadmap T1) and an application.

## Terminal grid (`mdshared_ui::termview`, feature `term`, 2026-11-26, `feat/terminal-pty-grid`, roadmap T1)

Draws a `mdshared_term::Screen` into a rectangle with the toolkit's `Painter`. The application
owns the screen, the pseudo-terminal, the selection, the scroll offset, the focus and the
bitmap; nothing here keeps a rectangle or reads a clock.

Decisions:

- **Whole-pixel cells.** A cell is a whole number of device pixels: as wide as the font's "0"
  advance, rounded, and as tall as the font's line, rounded up. The grid starts at the area's
  rounded corner, so lines meet across cells and 200 columns do not drift. What is left over at
  the right and bottom is padding in the background colour.
- **Each character is shaped once and drawn in any colour.** `CellFont` keeps its own parley
  contexts and `TextEngine`. A character is laid out once per bold, italic and size and kept,
  up to 4096 shapes, and is drawn at its cell through the new
  `TextEngine::draw_layout_clipped_tinted`, so neither shapes nor glyph masks depend on colour.
  A glyph is centred in its one- or two-cell span by whole pixels, so a fallback glyph of
  another width sits centred, on the cell's baseline. Spaces are not drawn. The painter's own
  text path could not be used: its engine is private and its layout cache is keyed by colour,
  which a screen of true-colour text would flood. Measured: a full 200 by 60 frame of coloured
  glyphs is 3.0 ms, about 255 ns a cell (release, warm cache, Windows).
- **Box drawing and blocks as shapes.** U+2500 to U+259F come from a table of arms (light,
  heavy, double), not from the font. Lines sit at a position fixed by the cell size and run to
  the cell's edge; corners and tees are filled; double lines turn into each other, stay open
  inside and stop at a single line that crosses them; dashes, rounded corners (antialiased
  quarter rings) and diagonals are there; eighths, halves and quadrants split a cell at the
  same pixels everywhere; shades are 25, 50 and 75 % fills. Tested: every pair of line
  characters meets edge to edge at four cell sizes.
- **Colours.** `Scheme::from_palette`: the page and text colours as the default pair; Windows
  Terminal's Campbell sixteen on dark and One Half Light on light; the cursor in the text
  colour with the page colour under it; the selection as the toolkit's text selection shows it.
  The 256-colour cube and the grey ramp are xterm's and are computed. Inverse swaps, dim is
  half way to the background, hidden draws only the background. `bold_is_bright` is there and
  off. The struct is `#[non_exhaustive]` so that T7's named schemes can add fields.
- **Only the changed rows.** Each row is drawn alone, clipped to its band, background first. A
  row drawn by `draw_rows` is pixel for pixel the row `draw_screen` draws and nothing outside
  the bands is touched (tested at scale 1 and 1.5, both palettes). The cheapest correct path
  with this painter is to keep the last frame's bitmap and repaint the rows
  `View::damage(screen.take_dirty())` gives. While scrolled back, any change redraws the whole
  view, since `take_dirty` cannot say whether output moved lines into the scrollback. A change
  of selection, focus, scroll, scheme or size is the application's to redraw.
- **The cursor.** Block (filled, the character in `cursor_text`), bar or underline, as DECSCUSR
  asks; a hollow block in every shape without the focus; not drawn when the program hides it or
  it is scrolled out of view; both cells of a wide character. It blinks through
  `Animator::blink`, and only while focused, visible, in view and asked to blink: otherwise
  nothing is asked for, so an idle window stays idle. At rest and under reduced motion the
  animated drawing is the plain one pixel for pixel (tested).

Outside the module: `mdshared_text::TextEngine::draw_layout_clipped_tinted` was added (the
untinted `draw_layout_clipped` now shares its body; its pixels and its signature are
unchanged, tested), and `mdshared-ui` got the optional dependency `mdshared-term` behind the
feature `term`.

Not here: the SGR blink attribute (drawn steady), underline styles and colours, ligatures
across cells, Powerline glyphs as shapes, smooth scrolling, accessibility of the grid, named
colour schemes (T7). In the light scheme ANSI 7 and 15 barely show on the white page, as in
One Half Light.

Run on Windows: `cargo test -p mdshared-ui --features term termview` (unit and pixel tests in
both palettes), the sheet (`termview_sheet`, looked at by the agent that wrote it and by the
reviewing session, not by the owner), the speed test (`termview_speed`, release),
`cargo clippy -p mdshared-ui --features term --all-targets -- -D warnings`, and
`cargo check -p mdshared-ui` without the feature. Not run on Linux or macOS. In a window it
has been seen only through the application that uses it.

## `system_is_dark` off the UI thread on Linux (2026-11-26, roadmap X9)

`platform::system_is_dark` used to ask the settings portal on the calling thread with a 500 ms
limit and cache `false` when it did not answer. It now keeps the last good answer in
`platform::LastGood`, like `motion_reduced`, and only its first call waits (for the same 500 ms),
so an application that reads the theme once at start still gets the portal's answer.

- **The same state machine.** `platform::LastGood` (the last good answer, who is reading, who
  listens, the wake, and the stamp that drops a late answer) keeps the dark theme as it keeps
  the wish for less motion. `read_and_watch` is shared by the two: the caller passes the
  `LastGood` it fills, the portal namespaces whose `SettingChanged` signals concern it, and a
  function that reads the setting from an open bus. `motion_reduced` and `system_is_dark` differ
  only in those three arguments.
- **The first call is the old call's shape.** The first `system_is_dark` in the process starts
  the helper thread and waits on a condition variable for its first answer, up to 500 ms (the old
  limit): mdview and mdedit, which call it once at start, behave as before. If no answer comes,
  the call gives `false` and stores nothing, so a later answer still wins. `LastGood::take_first`
  marks the one waiting call and `wait_first` does the waiting; neither is public.
- **Every later call never blocks.** `start_reading` takes the lock, claims a reading when none is
  under way and none listens (at most once a second), and starts the helper thread;
  `ask_in_background` is that plus the last answer at once. A reading whose answer is `None` (no
  bus, an error, a method call that timed out) changes nothing, so a dark theme is never
  overwritten with `false` because a read failed.
- **A hung portal ends the reading.** The connection is built with
  `zbus::blocking::connection::Builder::session()` and `method_timeout(5 s)`, so a `ReadOne` or
  `Read` that never answers fails with `ErrorKind::TimedOut` after five seconds: `read_dark`
  returns `None`, the last answer stands, and a later call (at most once a second) starts
  another reading. Only method calls are limited: zbus puts the timeout around the reply future
  of `call_method`, while the `SettingChanged` signal comes from the connection's broadcast
  stream, so the listener is not cut off. `read_dark` has no deadline of its own; the
  connection's method timeout bounds the call.
- **The same listener.** The first reading stays subscribed to the portal's `SettingChanged`
  signal and reads `org.freedesktop.appearance` `color-scheme` again at each change; only that
  namespace's signals count. The last answer wakes the application.
- **New API:** `platform::watch_system_dark(wake)`, the counterpart of
  `watch_motion_reduced(wake)`. On Linux it stores the wake in the dark theme's `LastGood`, and a
  wake given after an answer is called once at once, on the caller's thread. On Windows and
  macOS it is dropped and never called, because winit's `ThemeChanged` already tells the window;
  those two systems read the theme directly, as before.
- No signature changed, no public enum or struct gained a variant or a field, and Windows and
  macOS behave exactly as they did. The old one-shot `imp::portal_prefers_dark` (500 ms limit)
  is now a test helper only and shares `read_dark` with the production path.

Tests, none of which needs a D-Bus (`platform::dark_tests`), five in all:
`a_fast_first_answer_is_returned_by_the_first_call`,
`a_slow_first_answer_gives_false_in_time_and_a_later_call_sees_it`,
`a_slow_or_absent_answer_does_not_block_and_returns_the_last_good_value`,
`a_timeout_does_not_overwrite_a_good_value_with_false` and
`a_later_answer_updates_the_value_and_wakes`.

Fail-before evidence: the tests are compile-level. Renaming `ask_in_background` (which did not
exist before the first commit) made the three non-blocking tests and the production call site
fail with E0425. The two first-call tests use `take_first`, `start_reading` and `wait_first`,
which are new too, so they also do not compile on the old code; on Windows the old entry point
reads the registry and would make any behavioural assertion pass, so a compile failure is the
honest evidence. The second-reading check counts on a channel with a 100 ms timeout, and making
`claim_reading` always return true made it fail.

Run on Windows: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --features
mdshared-ui/host,mdshared-ui/a11y -- -D warnings`; `cargo test --workspace --features
mdshared-ui/host,mdshared-ui/a11y --no-fail-fast -- --skip explorer_tests` (mdshared-ui: 1120
passed, 33 ignored; doctests pass; exit 0), and the five dark tests on their own. Earlier, on the
first commit of this work, the release workspace clippy and test, and the release clippy and
tests of `recycle`, `media-session` and `notify` plus the `a11y` clippy, also passed; the second
and third commits have only had the debug commands above. Not run: the release host, host+a11y
and term clippy/test commands, the host example, the release `a11y` tests, and the rest of the
matrix after that point. Linux: `cargo check -p mdshared-ui --all-targets --target
x86_64-unknown-linux-gnu` type-checked with a stub `pkg-config` (exit 0); nothing was linked.
**Not run on a real Linux desktop**: the portal thread, the `SettingChanged` listener and the
wake are type-checked only, and Linux CI runs the rest. macOS was not compiled.

## A scroll area, and a dialog whose body scrolls (`scrollarea`, 2026-11-26, `feat/k11-scroll-area`, roadmap K11, Barbican BT1)

`mdshared_ui::scrollarea::ScrollArea` is one model that ties the pieces a scrolling region
already had together: a `viewport::Viewport` (content and view sizes, both offsets), both
overlay scrollbars (`scroll::scrollbars`), the painter's clip and offset, the wheel and the
keys, and the reveal of a focused control. It is the dialog whose body is taller than the card;
it also suits a form, a pane or a page.

Decisions:

- **Content pixels, one space for everything.** `layout_column` lays the body out in a rectangle
  `Flex::natural(&items)` long — its own length, not the view's, so flexible items keep their
  natural sizes instead of squeezing into the card — and returns the rectangles with the
  content's top left at (0, 0). `scrolled(p, area)` clips the painter to the area and moves it
  by the area's top left less the offset, so a caller draws with those rectangles directly.
  `reveal`, `reveal_focus` and `to_content` speak the same space.
- **The offset is always inside the limits.** `set_view`, `set_content` and `set_offset` clamp
  at once, so content that shrinks pulls the view back in the frame it is told, and no question
  about what is visible can be off the end. `layout_column` sets both sizes before clamping
  once, so a view and a content that grow in the same frame keep the offset. A size that is
  negative or not finite is 0; a NaN component of an offset or a delta leaves that axis where
  it is; an infinite movement goes to the end (as `Viewport` clamps).
- **Wheel and keys answer whether they moved.** A wheel that does not move the area is one for
  a parent. `host::Event::Wheel` gives a notch away from the user as positive, which scrolls
  up, so both axes are subtracted; Shift turns a vertical wheel across (`viewport::wheel_delta`)
  and a notch away then scrolls left. Page Down and Space move a page (the view less `LINE`),
  Page Up and Shift+Space back; arrows move `LINE`; Home and End go to the vertical ends, with
  Ctrl to the content's top left and bottom right; Alt chords are left to menus. Deltas arrive
  in the caller's unit (`host::Event::Wheel` already gives pixels).
- **The focus is passed in, not held.** `reveal_focus(&focus, id, at, extent)` reveals the
  control the frame says has the focus, the first time the focus comes to it; the focus it
  last revealed is remembered (as the hash of `focus.focused()`, since the model holds no
  `K`), so giving the same focus again moves nothing and the wheel or a page key can scroll
  inside a control taller than the view instead of being pinned back. The application calls it
  every frame for each control it lays out (or once, while the focus is outside), so the area
  sees the focus leave and reveals the control again when it comes back. A resize or a
  relayout of the same focus does not reveal again; an application calls `reveal` itself for
  that. The body is still drawn by the application's own widgets.
- **Drawing is two calls.** `draw_bars` draws only the bars that are needed (a bar `None` when
  the content fits that way), clear of the corner when both show; `draw_bars_animated` is the
  same with each bar's fade, and at rest is pixel for pixel `draw_bars`.
- **Gliding is opt-in and stays outside.** The model's offsets are plain `f64`; an application
  that wants the wheel to glide keeps a `scroll::SmoothScroll` per axis and gives
  `set_offset` what it draws each frame, as `viewport` already documents.

Added: `scrollarea::{ScrollArea, Scrolled, draw_bars, draw_bars_animated, LINE}`; the example
`scroll_dialog` (feature `host`): a card whose body of fourteen rows scrolls with the wheel,
Page Up / Page Down, Home and End, with Tab moving the focus and the area revealing it when it
moves. No existing signature, field or drawing changed.

Tests (in `scrollarea_tests.rs` since the split below, 19 and the ignored sheet). Model: content shorter than the view,
exact fit and a pixel more (no scroll, no bars), a content that shrinks pulling the offset in,
a wheel notch away from the user scrolling up and Shift turning it across, the page keys and
Home/End, reveal above, below and taller than the view, `reveal_focus` only for the focused
id and only once (a page key and a wheel then leave the offset alone, even for a control taller
than the view), and a focus that leaves the area for a control outside it and comes back being
revealed again (the per-control calls see it leave), `layout_column` keeping a flexible item at
its natural size and the rectangles still in content pixels after a scroll, `layout_column`
keeping the offset when the view and the content grow in one frame, points between content and
area, a NaN component keeping its axis from a non-zero offset, and sizes, offsets and points
that are not numbers. Drawing, in
both palettes where colour matters: the body draws nothing outside the area, everything inside
it, and a mark at its translated place (so a missing move fails the test); a bar shows only on
the axis that overflows (vertical, horizontal, both, and none) with the thumb's pixel differing
from the page and the corner both bars leave to each other left as the page; a frame at offset
40 shows rows 2 to 5 and the page above and below; after `restore`, drawing outside the area is
unclipped and in place; and `draw_bars_animated` at rest is `draw_bars` pixel for pixel with no
frame asked for. Each new test was seen failing under its broken implementation: the old wheel
sign, `reveal_focus` revealing every frame, a focus that left and came back not revealing (the
earlier id-keyed code), `set_offset` without NaN keeping, `layout_column`'s two clamps, an
empty `Scrolled::restore`, `scrolled` without the move, and a horizontal bar drawn unshortened
— each failed its test, and the earlier round's mutations failed every model and drawing test
(13 of 14 in one pass; the exact-fit test needed `scrolls` to answer `>=` instead of `>`, and
failed then too). The file was restored afterwards.

The sheet `cargo test -p mdshared-ui --lib scrollarea_sheet -- --ignored` writes
`target/specimen/scrollarea.png`: the dialog at the top and scrolled to row 6, light above and
dark below; the writing agent looked at it.

Run on Windows 11: `cargo test -p mdshared-ui --lib scrollarea::` (19 pass, the sheet ignored);
`cargo run -p mdshared-ui --features host --example scroll_dialog -- --seconds 2` opened the
window, drew two frames at offset 0 and closed; `cargo fmt --all --check`, and the workspace
clippy and test commands listed in AGENTS.md (results in the commit message). Not run on Linux
or macOS. Not seen in a real window by the owner, and the wheel, the keys, Tab and the click
were not driven in the example by a person or a machine: only the drawing path ran.

Known limits for later:

- At a scale above 1, hit testing and drawing can differ by up to about one device pixel:
  `scrolled` rounds its move to whole device pixels on its own, while `to_content` works from
  the logical offset.
- The overlay bars are drawn over the body: the right edge of the rows (about 9 px where the
  thumb is) and, when both show, the bottom 12 px are covered.
- The example's own rough edges: a press on a scrollbar focuses the row under it, the bars are
  drawn active whenever a row is hovered, and the hovered row stays as it was after a wheel
  turn until the pointer moves.
- If an application stops calling `reveal_focus` while its area is hidden, the area cannot see
  the focus leave; when it reopens with the focus still on the same control, that control is
  not revealed. Call `reveal` directly when the area reopens, or keep offering the controls
  while it is hidden (which costs nothing).

The file was split when it passed the house limit: the test module (617 of its 1011 lines) moved
to `scrollarea_tests.rs` beside it, in the `submenu_tests.rs` pattern, leaving the model and the
two drawing functions in `scrollarea.rs` (397 lines). No test was edited: the body of the module
moved out of its `mod tests` wrapper unchanged but for one level of indentation, and
`cargo test -p mdshared-ui --lib scrollarea -- --list` gives the same 20 names before and after. Run on Windows 11: `cargo fmt --all --check`; the workspace clippy with
`host` and `a11y`; and the workspace tests with `--skip explorer_tests`, 1139 passed and 34
ignored in `mdshared-ui`. Not run on Linux or macOS.

Left for later: momentum and rubber-banding at the ends; dragging a thumb and the bar's hover
fade (`scroll::draw_scrollbar_animated` is ready for it, `draw_bars_animated` takes `active`
from the caller); a scroll area that also reports how much is left to a screen reader (roadmap
K12); and an application or Barbican screen using it, which is what would show it to a person
first.

## Escape closes the submenu, not the menu (`submenu::Cascade`, 2026-11-26, `fix/cascade-cancel-wait`, asked by mdview)

A cascade whose submenu is open, with the pointer moved onto another row of the main menu (or
onto its padding) less than `DELAY` ago, has a close waiting. `Cascade::key` made that waiting
change first, before acting on the key, so Escape then found only the root open and returned
`Outcome::Close`: the application closed the whole menu, where before `Cascade` its Escape
closed only the submenu. Left lost the menu too, because the waiting close ran first and Left
then fell through to `Outcome::Ignored`, which a menu bar takes as "move to the menu beside".

Two additions, both opt-in; a caller that uses neither behaves exactly as before:

- `Cascade::cancel_wait(&mut self) -> bool` drops the waiting pointer change without making
  it: nothing opens or closes, and `deadline()` is `None`. It puts the menus back on the open
  path: the pointer had lit the row it moved onto, though that row's submenu never opened, so
  the row that opened each open submenu is lit again, and the keys go to the innermost open
  menu. What is drawn changes when the pointer had lit another row, nothing else. Called before
  forwarding Escape or Left, those keys then close only the innermost submenu. It returns true
  when a wait was dropped, as `pointer` and `tick` return whether they changed anything, and
  false when nothing was waiting, in which case nothing is touched at all.
- `Cascade::keys_cancel_wait(on)`, a builder off by default, has `key` do exactly that for
  Escape and Left (it calls the same `cancel_wait`), and has Left close the innermost open
  submenu even when the keys are in the menu before it (where it is `Ignored` otherwise), so
  Left means "back one level" before "the menu beside"; when either key closes a submenu, the
  rows that opened the menus still open stay lit. Every other key still makes a waiting change
  first.

Known limit, with the switch on: where the keys land after Left depends on whether a wait was
dropped. If one was, they go to the innermost open menu, where `cancel_wait` puts them; if not,
they stay where the pointer put them, in the root. The same picture can therefore react
differently to the Down that follows. The next pointer move resets this.

Tests, in `submenu_tests.rs`:

- `cancel_wait_with_nothing_waiting_returns_false_and_leaves_the_keys_alone`: from a cascade
  with nothing waiting, `cancel_wait()` is false, the keys stay in the root and no lit row
  moves; Down then acts in the root, not in the submenu.
- `cancel_wait_drops_a_waiting_close_so_escape_closes_only_the_submenu` and
  `cancel_wait_drops_a_waiting_close_from_the_padding_too`: submenu open, a close waiting from
  a pointer onto another main-menu row (resp. onto the padding above the first row);
  `cancel_wait()` lights the row that opened the submenu again and gives the keys back to the
  submenu, then Escape closes the submenu only, the root stays open, and a second Escape is
  `Close`.
- `cancel_wait_then_left_closes_only_the_submenu_with_the_switch_off`: a context menu, the
  switch off; after `cancel_wait()` Left is `Changed` (the keys are back in the submenu), the
  submenu closes, the root lights Open recent, and with nothing open Left is `Ignored` again.
- `cancel_wait_lights_the_open_path_of_a_deep_cascade`: at depth 3, `cancel_wait()` lights Open
  recent and More (the rows that opened the open menus) and gives the keys to the innermost
  menu; Escape closes only More's submenu and the path stays lit.
- `with_the_switch_escape_drops_the_wait_by_itself` and
  `with_the_switch_left_drops_the_wait_and_closes_only_the_submenu`: the switch alone, no
  `cancel_wait` call.
- `the_switch_closes_one_level_at_depth_three_and_leaves_the_open_path_lit`: with the switch,
  Open recent then More open and the pointer on Print in the root; Left closes More's submenu
  only, and Open recent and More stay lit.
- `assert_path_lit` (a helper, not a test) checks the rule in each open menu: the lit row is the
  row that opened the next one.
- `without_the_wait_cancelled_the_pointer_change_runs_first_as_before`: the old behaviour kept
  as a guard: Escape returns `Close` and takes the menu, Left returns `Ignored` after the
  waiting close has run, and Down still makes the waiting change first with the switch off and
  on.

Fail-before evidence: in the first commit, with `key` restored to its old body (both methods
kept, so the tests compile), the two switch tests failed, Left giving `Ignored` where `Changed`
was expected and Escape giving `Close`; with `cancel_wait` made a no-op, the three tests that
call it failed on `deadline()`. For the review's commit, with the production hunks reverted (the
old `cancel_wait` body, the switch dropping the wait without `cancel_wait`, and neither closing
arm re-lighting the path), seven tests failed and the guard passed; the first was `the lit row
of menu 0 opened the next: left: Some(3), right: Some(1)` — Print lit while Open recent's
submenu is still open, the defect the review found. With the early return removed from
`cancel_wait`, `cancel_wait_with_nothing_waiting_returns_false_and_leaves_the_keys_alone`
failed: it returned true and moved the keys into the submenu.

Run on Windows, debug: `cargo fmt --all --check` (exit 0); `cargo clippy --workspace
--all-targets --features mdshared-ui/host,mdshared-ui/a11y -- -D warnings` (exit 0); `cargo
test --workspace --features mdshared-ui/host,mdshared-ui/a11y --no-fail-fast -- --skip
explorer_tests` (exit 0; mdshared-ui 1129 passed, 33 ignored; doctests pass). Not run: the
release commands, the `recycle`, `media-session`, `notify`, `host`, `host,a11y` and `term`
feature clippy/test lines, Linux and macOS. Not seen by a person in a window.

## Pictures kept at their drawn size (`mdshared_ui::picture`, 2026-11-26, `feat/k14-pictures`, roadmap K14, Barbican BT1)

`Painter::image` squeezes a picture into its rectangle on every call. An avatar, an icon or a
badge drawn every frame therefore averages the same source pixels again and again. A
`Picture` does that once: it keeps the pixels of the last size it drew, at that device size,
and a later draw at the same size, scale and corner radius only blends them.

- **The model and the cache are one small struct.** `Picture` holds the source `Image`, one
  prepared buffer and a counter the tests read. It is `Clone`, with a `Debug` that prints the
  sizes rather than the pixels; the buffer is straight RGBA with the rounded corners already
  folded into its alpha, so a draw needs a blend and no coverage test.
- **What prepares again.** The key is `round(dst.w * scale)` by `round(dst.h * scale)`, the
  display scale's bits and the device corner radius's bits. The prepared size is a property
  of the rectangle, not of where it sits: `dev_rect` rounds the left and right edges
  separately, so at 1.25 and 1.5 the placed device width can be a pixel different from the
  same rectangle moved by one logical px. The blit is clipped to the placed rectangle, so a
  placement a pixel narrower loses the cached picture's last column or row, and one a pixel
  wider leaves that column or row unpainted: nothing is painted past `dst`, into the next
  item of a row. A move, a clip change or an alpha change therefore does not prepare, even at
  fractional scales. A size, scale or radius change replaces the one prepared size; there is
  no cache of several sizes, which is the caller's decision if it wants one.
- **The same scaler, not a second one.** The scaling that was inside `Painter::image` moved
  to its own file, `gfx/painted.rs`, as `Scaled` (taps, the across pass and the down pass)
  and `blend_pixel`. `image` still uses it, so its pixels are unchanged; `prepare_image`
  uses the same sampling and folds `corner_cover` into alpha; `blit_rgba` reuses
  `blend_pixel` at 1:1. `gfx/shapes.rs` no longer holds the picture code (it was already
  over a thousand lines; it is 829 now, `painted.rs` is 511 and `picture.rs` is 619).
- **A prepared draw matches `Painter::image`** at the same device size, within 2 levels a
  channel: the difference is the 8-bit rounding of the prepared buffer, and the test checks
  scales 1, 1.5 and 2, a rounded corner, half alpha and a cut-out. An infinite or NaN radius
  rounds as far as half the smaller side, exactly as `Painter::image`'s `f32::min` chain
  does, and is tested against it.
- **Limits.** A target over 1024 device px a side or a million pixels is handed to
  `Painter::image`, which allocates only the visible part; a huge finite target therefore
  draws its visible corner and prepares nothing. A zero, negative, NaN or infinite rectangle
  draws nothing; a zero or NaN alpha draws nothing; an infinite or NaN radius rounds fully;
  a huge finite radius is cut to half the smaller side. `Scaled::take` does its arithmetic in
  64-bit and refuses a side beyond `i32::MAX` device px, so an absurd rectangle cannot
  overflow or ask for a buffer larger than the clip it draws into. The prepared buffer is at
  most a few MiB, but `prepare_image` also allocates 16 bytes for every source row the
  destination reads, so a tall source scaled down can take hundreds of MiB; the cap comment
  in `picture.rs` says so.
- **The decoder slot.** `Decoder` is a trait with one method, `decode(&[u8], Limits) ->
  Option<Image>`; any `Fn(&[u8], Limits) -> Option<Image>` is one, so an application writes a
  closure for GIF, WebP, ICO or SVG. The `Limits` are passed in so a decoder can read the
  header and refuse before allocating, as the built-in decoders do. `Picture::load(bytes,
  limits, Some(&slot))` reads PNG and JPEG through `mdshared_text::image::decode`; the slot
  is asked only when that says `Unsupported`, never for a damaged PNG or JPEG. The call is
  wrapped in `catch_unwind` (`AssertUnwindSafe`), so a panic in it is `Error::Damaged`, and
  an `Image` whose pixels are not `width * height * 4` or whose side is zero is `Damaged`
  too. A picture over the `Limits` is `TooLarge`. It is not a registry and there is no
  global: the slot is passed to the call that uses it.
- **New public API:** `picture::{Picture, Decoder}`; `Picture::{new, load, size, draw}`.
  `Painter::blit_rgba` and `prepare_image` are crate-private. No existing signature, public
  field or drawing changed.

Tests, eleven in `picture::tests`:

- `a_prepared_picture_draws_what_the_painter_draws_at_the_same_size` (three scales, radius,
  alpha and a cut-out, within 2 levels a channel).
- `a_second_draw_at_the_same_size_does_not_prepare_again` (draw, redraw, move: one prepare).
- `a_move_at_a_fractional_scale_does_not_prepare_again` (scales 1.25 and 1.5, whole and
  fractional moves, one prepare).
- `an_alpha_change_does_not_prepare_again` (the same size at alpha 1 and 0.35, one prepare).
- `an_infinite_or_nan_radius_rounds_as_the_painter_does` (each matches `Painter::image`,
  one prepare, the corner clear).
- `a_size_or_scale_change_prepares_again` (a wider target, the same device size at another
  scale, back again, a new radius: one prepare each).
- `the_clip_and_the_offset_are_honoured` (the inked box is exactly the clip's intersection;
  the clipped draw and the whole draw together prepare once; the offset moves them by whole
  pixels without preparing again).
- `a_picture_a_pixel_narrower_does_not_paint_past_its_rectangle` (at scale 1.5 a placement one
  device px narrower than the cached width leaves the pixel just past `dst` untouched).
- `zero_nan_and_huge_sizes_neither_panic_nor_prepare` (zero, negative, NaN and infinite
  rectangles and a zero, NaN alpha draw nothing; a huge finite target is not prepared but
  draws its visible corner; huge, infinite and NaN radii all round fully and prepare once).
- `the_decoder_slot_reads_a_format_the_toolkit_refuses` (a GIF-shaped closure; no slot gives
  the built-in error; a damaged PNG does not reach a slot that would take anything; a slot
  picture over the limits is `TooLarge`).
- `a_slot_that_panics_or_returns_a_broken_image_is_damaged` (a panicking decoder, a picture
  with one byte too few and one with a zero side are all `Damaged`).

Every new item is new API, so the tests do not compile on the code before this branch: that is
their fail-before evidence. Each behaviour was also checked by breaking it and watching the
test fail. Making the key check always true (prepare on every draw) failed
`a_second_draw_at_the_same_size_does_not_prepare_again`,
`a_size_or_scale_change_prepares_again`, `the_clip_and_the_offset_are_honoured` and
`zero_nan_and_huge_sizes_neither_panic_nor_prepare`. Keying on the placed device size, as the
first commit did, failed `a_move_at_a_fractional_scale_does_not_prepare_again`. Removing the
corner coverage from `prepare_image` failed
`a_prepared_picture_draws_what_the_painter_draws_at_the_same_size` (by 253 levels). Removing
the clip from `blit_rgba` failed `the_clip_and_the_offset_are_honoured`. Drawing the blit
past the placed rectangle, as the first commit did, failed
`a_picture_a_pixel_narrower_does_not_paint_past_its_rectangle` (the pixel outside `dst`
became the picture's). Making `load` never call the slot failed
`the_decoder_slot_reads_a_format_the_toolkit_refuses`; removing the `catch_unwind` failed
`a_slot_that_panics_or_returns_a_broken_image_is_damaged`.

Run on Windows: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --features
mdshared-ui/host,mdshared-ui/a11y -- -D warnings`; `cargo test --workspace --features
mdshared-ui/host,mdshared-ui/a11y --no-fail-fast -- --skip explorer_tests` (exit 0;
mdshared-ui 1131 passed, 33 ignored, doc-tests 5 passed and 1 ignored; the other crates all
passed). Not run: release builds, the rest of the seventeen-command matrix, Linux and macOS
(CI will). **Not seen in a real window**: no application or example draws a `Picture` yet, and
no person has looked at it; the evidence is pixel tests against `Painter::image`.

Known limits, for later: `Painter::dev_x`/`dev_y` saturate a product to `i32::MAX` and then
add the painter's offset, which overflows in debug for a coordinate near 2.2e9. That was
already true before this branch; `Picture` only inherits it. A tall source scaled down can
still make `prepare_image` allocate hundreds of MiB, as above.

## Everyday controls (roadmap K4, first slice): `controls`

Roadmap K4 is the everyday controls a settings page or a dashboard needs. It is large, so this
branch does its first slice and leaves the rest there: **not here** are the check box with a
label and a mixed state, the radio group, the chip with remove, the badge and count, the card,
the divider and the avatar. What is here is `mdshared_ui::controls`, a directory module with
three parts, all in the toolkit's immediate-mode shape: plain models and free `draw_*`
functions that change nothing.

- `button`: a push button in four kinds — primary (the accent fill), secondary (a quiet fill
  with an edge), subtle (no fill until the pointer is over it) and destructive (the danger
  fill) — and six states: rest, hovered, pressed (only while the pointer is still on it, as
  `overlay::draw_dialog_button` reads a press), focused (a ring through `focus`), disabled and
  default (the button Enter presses). The colours are one shared `Style` per kind, so the menu
  and split buttons in `menu_button` look the same. The filled kinds are drawn in
  `Face::Semibold`, as a dialog's primary button is, and the other two in `Face::Ui`.
- `icon_button`: a square subtle button with one glyph and a required name, which is its
  tooltip's text too; `IconButton::over(key, rect)` builds the `tooltip::Over` the caller hands
  to `Tooltip::hover`, and drawing stays `overlay::Bubble::tip`.
- `menu_button`: `MenuButton`, whose whole face opens a menu, and `SplitButton`, whose label
  acts and whose `CHEVRON_W`-wide chevron opens it. `menu_under(menu, button)` is
  `Menu::around(button)`; the menu itself, its items, its keys and its Escape stay the caller's.
  The keys are Windows' own: Space, Enter and Down open a menu button; Space and Enter run a
  split button's action while Down and Alt+Down open its menu.
- `field`: `FramedField` wraps a `TextField` with a frame, an optional leading icon, a clear
  button while there is text and an error message under the box. The field keeps no clock: the
  caller blinks the caret with `draw_blinking` and the animator, as it does for `TextField`.

Look decisions the owner should see:

- A disabled button of **any** kind is the one quiet `field` frame with a faint label, in
  semibold, whatever its kind — exactly `overlay::draw_dialog_button_disabled`'s behaviour,
  which ignores the role once a button cannot be pressed. The kind no longer shows.
- The destructive fill is `palette::danger`, and its label ink is **chosen per palette by
  contrast** (`destructive_ink`) out of the palette's own inks and the white `ON_DANGER`: white
  on the dark palette's red (5.6:1, where its `text` is 4.48:1), the palette's black on a light
  high-contrast red (8.9:1). Hover and press move the fill *away* from that ink (`shade`), so
  the label never reads worse than at rest; the old lightening of the light red on a press
  (4.0:1) is gone. Under high contrast the destructive button is an outlined danger button:
  the page as its fill, the window text as its label and `danger` as its frame, since the
  system's contrast palettes guarantee the text on the page, the hover and the press. The
  toolkit's two high-contrast palettes are recognised by identity, like `infobar`'s, and the
  `high_contrast` flag covers a system contrast theme on an ordinary palette.
- A default button is marked with a line just inside its edge, drawn with `frame` so it is one
  device pixel at any scale: the label's ink on the two filled kinds (white on the dark red,
  black on the light HC red — 3:1 or better on the fill by construction), the accent on the
  others.
- The framed field always reserves the clear button's room at the right, so the text does not
  shift when the first character is typed or the field is cleared. Focused, the frame and a
  2 px line along the bottom are the accent; with an error, both are `danger`.
- **Escape does not clear the field by default.** The brief asked for it "if that is the
  existing convention"; it is not — in this toolkit Escape closes dialogs (`settings`), popups
  (`dropdown`), the colour picker and the command palette. `set_escape_clears(true)` turns it
  on for a search box, and with an empty field Escape is still `Outcome::Unused`, so the
  caller's Escape still closes what it closes.

**New public API:** `controls::button::{Button, Kind, Look, draw, key, hit, cursor, width,
button_rect, HEIGHT, RADIUS, PAD, TEXT, ON_DANGER}` where `Look` also carries `open` (drawn as
pressed whatever the pointer does) and `high_contrast`; `controls::icon_button::{IconButton,
draw, rect, SIZE, ICON}` (`new(icon, name)`, and the name is required); `controls::menu_button::{
MenuButton, SplitButton, Half, Halves, SplitLook, Press, draw, draw_split, key, menu_under,
menu_button_w, split_button_w, chevron_rect, label_rect, halves, hit, CHEVRON, GAP, CHEVRON_W}`;
`controls::field::{FramedField, Look, Hit, Outcome, draw, draw_blinking, HEIGHT, ERROR_H,
ICON_W, CLEAR, CLEAR_PAD, PAD, RADIUS, TEXT, ERROR_TEXT}` with `press` holding the clear button,
`release` doing the clearing, `clear_held`, and `Look::high_contrast`; and `a11y::menu_button`
under the `a11y` feature. No existing signature, public field or drawing of an existing widget
changed; the new module is declared in `lib.rs` and the example in `Cargo.toml`. No dependency
was added.

Tests, 48 and one `#[ignore]`d sheet, all over the four palettes (light, dark, and both
high-contrast ones):

- `button_tests.rs` (17): the rectangle, the hit, the keys (the Mac's Control included), the
  cursor, `every_kind_and_state_draws_differently_in_every_palette`,
  `the_four_kinds_differ_at_rest_in_every_palette`,
  `each_kind_and_state_is_filled_in_its_exact_colour` (the fill rules written out a second time
  and pinned at a pixel), `an_open_button_is_drawn_pressed_wherever_the_pointer_is`,
  `a_press_counts_only_while_the_pointer_is_on_the_button`, the ring outside, the quiet
  disabled frame, the default mark, `a_default_line_reads_against_its_fill_and_is_one_device_pixel_wide`
  (scales 1, 1.25 and 1.5), `the_destructive_label_reads_against_its_fill_in_every_state`
  (rest, hover, press), `the_four_palettes_are_told_apart_and_the_high_contrast_flag_is_honoured`,
  the empty rectangle, and the sheet.
- `icon_button_tests.rs` (5): the square and its required name, the hit and keys, the glyph and
  six states in the four palettes, the quiet disabled look, the empty rectangle, and the
  tooltip model's text and anchor.
- `menu_button_tests.rs` (13): the width and the chevron's room (12 px from the left, 6 px to
  the chevron), the keys, the split keys, the two halves and their hit,
  `the_menu_opens_around_the_button_below_and_above_near_an_edge`, the chevron and hover
  pixels, `a_menu_button_whose_menu_is_open_is_drawn_pressed`,
  `a_split_chevron_whose_menu_is_open_is_drawn_pressed`, the halves lighting on their own, the
  ring and default mark, `the_split_border_and_default_mark_are_one_device_pixel_wide` (scales
  1, 1.25, 1.5), the quiet disabled split, and an empty rectangle.
- `field_tests.rs` (14): the height and the error band, the clear button's place and when it
  shows, the icon's and the clear button's room, `the_clear_button_empties_the_field_on_release_over_it_not_on_press`
  (a drag off it clears nothing), `the_clear_button_keeps_the_fields_real_focus` (a `Focus`
  model: the field keeps the keyboard and Tab continues from it), the error's frame and
  message, `the_error_message_and_its_frame_read_on_every_palette`,
  `a_high_contrast_palette_takes_the_lines_own_colour_for_the_resting_edge`, the focused edge,
  Escape on, off and repeated, the editing keys (a repeated Backspace still deletes), the
  placeholder, the quiet disabled field, and an empty rectangle.
- `controls_sheet` (`#[ignore]`): four kinds in seven states, the icon, menu (open) and split
  (chevron open) buttons and three framed fields, in the four palettes, at scales 1, 1.25 and
  1.5, written to `target/specimen/controls.png`, `controls-125.png` and `controls-150.png`
  (`cargo test -p mdshared-ui controls_sheet -- --ignored`).

Fail-before evidence. Every test is new API, so it does not compile before the branch. The
first forty were shown to fail with their behaviour removed, as the commit before the review
records: every public model and drawing neutered failed 34 of them; the six that assert
"nothing is drawn" or "a disabled control takes no mark" failed with the `sane` guards and the
disabled branches removed; and the destructive contrast test failed with `ON_DANGER` set to
black.

The tests the review added or rewrote were shown to fail with the review's fixes reverted in
one mutation: `state` without `open`; the destructive ink and fills back to white and the
popup mixes; the default mark back to the mixed line drawn by `stroke_path`; the split's
border back to `stroke_path`; `error_ink` back to `danger` always; the field's resting edge
without the high-contrast branch; the clear button clearing on the press; and
`a11y::menu_button` without its expanded state. Fourteen controls tests failed —
`an_open_button_is_drawn_pressed_wherever_the_pointer_is`,
`each_kind_and_state_is_filled_in_its_exact_colour`,
`the_destructive_label_reads_against_its_fill_in_every_state`,
`a_default_line_reads_against_its_fill_and_is_one_device_pixel_wide`,
`the_four_palettes_are_told_apart_and_the_high_contrast_flag_is_honoured`,
`a_menu_button_whose_menu_is_open_is_drawn_pressed`,
`a_split_chevron_whose_menu_is_open_is_drawn_pressed`,
`the_split_border_and_default_mark_are_one_device_pixel_wide`,
`the_clear_button_empties_the_field_on_release_over_it_not_on_press`,
`the_clear_button_keeps_the_fields_real_focus`,
`the_error_message_and_its_frame_read_on_every_palette`,
`a_high_contrast_palette_takes_the_lines_own_colour_for_the_resting_edge`,
`the_field_shows_its_placeholder_while_empty_and_not_after` and
`the_focused_field_draws_the_accent_edge_and_a_line_along_the_bottom` — and the a11y node test
failed. Every new test is in that list.

Run on Windows (branch `feat/k4-controls`, `CARGO_TARGET_DIR=E:/mds-target/k4`):
`cargo fmt --all --check` (clean); `cargo clippy --workspace --all-targets --features
mdshared-ui/host,mdshared-ui/a11y -- -D warnings` (exit 0);
`cargo test --workspace --features mdshared-ui/host,mdshared-ui/a11y --no-fail-fast -- --skip
explorer_tests` (exit 0; mdshared-ui 1207 passed, 35 ignored; the other crates all passed);
`cargo run -p mdshared-ui --features host,a11y --example controls -- --seconds 2` (exit 0, the
timer woke it and it closed); the sheet test wrote the three PNGs.

Not run: the rest of the seventeen-command matrix (the `recycle`, `media-session`, `notify`,
`a11y`, `host`, `term` single-feature forms and the Linux type-check), release builds, and
everything on Linux and macOS (CI will). **Not seen by a person**: the example has opened once
for two seconds with no pointer or keyboard on it, and no application draws these controls yet;
the sheet PNGs were generated and looked at by the agent, which is not the owner's approval of
how they look.
## Review of the controls (0b64318): the fixes

A review found three blocking problems and a list of smaller ones; the second commit answers
every item.

- **A button whose menu is open is drawn pressed.** `button::Look::open` selects the press fill
  without the pointer having to be on the button, and `state` reads it for both buttons and
  split halves; the menu button and the split chevron each have a test, and the example uses it
  (the menu button and the chevron stay lit while their menu is open, from the keyboard or with
  the pointer inside the menu).
- **Contrast.** The destructive label is chosen per palette (see above), hover and press move
  the fill away from it, the default mark is the label's ink on filled kinds, and the field
  takes `infobar::DARK_ERROR_INK` on a dark palette that is not high contrast. Under high
  contrast (`Look::high_contrast`, or the two high-contrast palettes by identity) the
  destructive button is the outlined danger button and the field's edge is the palette's
  `line`; the old destructive default mark was under 2:1 in the dark palette. The contrast
  tests run over all four palettes and the hover and press fills.
- **The field's error** is `DARK_ERROR_INK` on a dark non-high-contrast palette (8:1 on the
  page rather than 2.9:1), `danger` elsewhere; its message, glyph and frame have a contrast
  test of their own. The message is drawn under the box in the `height()` band; the module doc
  says to lay out with `height()` each frame or to reserve `HEIGHT + ERROR_H` always.
- **Smaller library fixes:** the menu button's label sits 12 px from the left and 6 px from the
  chevron (it was 6 and 6 by centring); the field's focus line is placed with `p.px()`; the
  split's frame and the default mark are `frame` lines, one device pixel at 1.25 and 1.5 (a
  test scans the column); the clear button is *held* by a press (`Hit::Clear`, `clear_held`)
  and clears on a release over it (`release`), so a drag off it cancels; a preedit the clear or
  Escape drops is the application's to reset with its IME, as the module doc says; `key` takes
  the repeat flag (a repeated Escape clears once; a repeated Backspace still deletes); the keys
  check `mods.control` as `toggle` does; Space acts on the key-down because `host` has no
  key-release event, which `button::key` documents; the doc sketch checks a key on the focus,
  not the hit.
- **Accessibility:** `a11y::menu_button` (a button with UIA ExpandCollapse, its `open` state and
  a menu popup) for the menu button and the split chevron; the icon button's name is now
  required (`IconButton::new(icon, name)`), so a screen reader never gets an unnamed button.
- **Example:** a chevron press focuses the split's main half (one Tab stop, declared) instead
  of an id the focus model never saw; a menu button clicked while its menu is open closes it
  instead of reopening; `split_rect` takes the union to the chevron's right edge; the a11y
  tree's focus follows the real focus; a held Enter or Space activates once.

Not run for the fix: the rest of the CI matrix, release builds, Linux and macOS; the example
still has not been driven by a person, and the high-contrast panels of the sheet have not been
seen on a real contrast theme.
