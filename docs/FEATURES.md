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
