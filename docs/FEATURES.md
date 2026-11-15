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
