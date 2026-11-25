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

## Fonts warmed before the first paint (`Gfx::warm_fonts`, `mdshared_text::warm_fonts_with`, 2026-11-25, `feat/warm-fonts`, asked for by mdplay)

`Gfx::warm_fonts()` starts, on a thread named `mdshared-fonts` and at most once per process,
building the font context `shared_fonts()` keeps and laying out a sample line in each `Face`
(the UI face regular and semibold, the mono face, with the symbols U+2630, U+2713 and U+2026),
so the families are resolved and their fonts loaded there. A `Gfx` made after it
(`fonts_warming()` true) takes that context at its first text, not in `Gfx::new`, waiting if it
is still being built; from then on everything is as before. In `mdshared-text`: `warm_fonts()`
(build only), `warm_fonts_with(prime)` (build, then `prime` on the same thread),
`fonts_warming()`. If the thread cannot start or panics, in the build or in the priming (caught
on that thread), the first text builds on the calling thread as without it. Without a call
nothing changes: `Gfx::new` takes `shared_fonts()` as before. The thread runs at normal priority
(a lower one needs the Windows API, which `mdshared-text` does not use, and the window may wait
for it).

Measured on this machine (Windows 11, release, three runs, `cargo test -p mdshared-ui --release
--lib -- --ignored --nocapture first_measure_timing`), on a fresh thread: cold, `Gfx::new`
1.6-1.7 ms, the first measure in the UI face 15.7-16.4 ms, the first U+2630 0.64-0.66 ms; after
a finished warm, `Gfx::new` 0.06-0.09 ms, the first measure 0.07-0.08 ms, U+2630 0.03 ms.
Building the context alone (`mdshared-text`'s `warm_timing`, `first_paint_probe`) is about 1 ms:
fontique is lazy, and the cost is the first lookup of a family. Inside foobar2000 it was not
measured here.

Run: `cargo test --workspace --features mdshared-ui/host,mdshared-ui/a11y` (in `fonts.rs`: a
warmed context is built and primed once and taken once, a second start is a no-op; never
started, a panicking build and a panicking priming give nothing to take; `shared_fonts` after
`warm_fonts` works on a fresh thread; `FontContext: Send` at compile time. In
`gfx_warm_tests.rs`: a primed context lays out every face at two sizes to the same widths as a
cold one; a `Gfx` on a fresh thread after `Gfx::warm_fonts` (called twice) measures as a cold
one); `cargo clippy --workspace --all-targets --features mdshared-ui/host,mdshared-ui/a11y --
-D warnings`; `cargo fmt --all --check`; on Windows only. Not seen in a running app.
