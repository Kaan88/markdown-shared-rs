# Next release: shared-more (branch `f/shared-more`)

Three additions to the shared crates. Nothing in either application changes until it adopts them;
every existing caller compiles and behaves as before. Details and design notes are in
[../FEATURES.md](../FEATURES.md).

## 1. Vue and Svelte highlighting (`mdshared-highlight`)

**Built.** A line scanner (`scan::sfc`) for single-file components: HTML tags, attributes, comments
and entities; `<script>` as TypeScript; `<style>` as CSS, or SCSS and Less by `lang`; Vue
`{{ }}` and directive values (`v-*`, `:x`, `@x`, `#x`, also across lines); Svelte `{expr}`,
`{#if}` `{:else}` `{/if}` `{@html}`, braces in tags and quoted values, and `on:` `bind:` `class:`
`use:` and the other directives. Incremental by the same state-per-line design as the other
scanners; limits unchanged.

**Public API added.** `scan::Lang::Vue` and `Lang::Svelte`, found by `Lang::from_token` and
`from_path`, so `Language::for_path`, `Language::by_token` (fence names `vue`, `svelte`) and
`tokens` resolve them. A new variant of a public enum: code that matches `Lang` exhaustively must
add them (neither application does). Internal: `Spec.line_comment`, `generic_at`.

**What each app does.** Nothing: `.vue` and `.svelte` files open highlighted in the editor, and
```` ```vue ```` / ```` ```svelte ```` fences in the viewer. After taking the new shared rev, the
editor's language list or status text, if it has a fixed list of names, may want "Vue" and
"Svelte".

**Run.** `cargo test --release --workspace` (new: both languages by token and path, colours in
both themes, spans tile the source, the incremental path equals the whole-text path, edits that
open comments and expressions, unterminated and hostile input, linear time).
**Not verified.** Either application; real-world components; speed against the other scanners
(only the linear-time tests ran, no benchmark); `lang="sass"`, `stylus`, `pug` (read as CSS or
HTML).

**Release note.** Vue and Svelte files, and `vue` and `svelte` code blocks, are now syntax
highlighted.

## 2. Access keys in menus (`mdshared-ui::menu`)

**Built.** Opt-in per menu. `Menu::new(..).access_keys(true)`: `&` marks a letter (`Open &files…`),
`&&` is a literal `&`; the letter is underlined, the width ignores the `&`, and
`Menu::access_key(char)` finds the first enabled item with that key (Unicode lower case).

**Public API added.** `menu::{Access, parse_access, escape_access, access_key_of, same_letter}`,
`Menu::{access_keys, set_access_visible, access_key}`, `gfx::Painter::underline_range`.

**Labels with `&` in the apps (read 2026-11-15).** No string literal in either app's `Item::new`
calls contains `&`. But labels built from data do go into menus: the Recent menus (`e.label`, a
file name), the editor's completion menu (`c.text`, source text) and saved searches. So the switch
is per menu and off by default: nothing changes for any existing menu. In a menu that turns it
on, data labels go through `escape_access`.

**What each app does.** In the places that build the menus to opt in (viewer
`bins/mdv-app/src/overlays.rs`, editor `bins/mdedit/src/overlays.rs`): add marks to the literal
labels, give each item of a menu a different letter, call `.access_keys(true)`. In the key handler
for an open menu, after Up, Down, Enter and Escape: for a printable character without Ctrl or Alt,
`if let Some(id) = menu.access_key(ch) { act on id as for Enter }` and redraw. For the Windows
convention of showing underlines only after Alt, call `set_access_visible(false)` when the menu
opens by mouse and `true` on Alt or when it opens by keyboard (the shared code cannot see the
keyboard); the default is always visible.

**Run.** Tests for parsing, width, activation (case, non-ASCII, disabled items, duplicates, opted
out), and drawing (an unmarked menu is bit-identical with the switch on or off). **Not verified.**
How the underline looks (its distance from the baseline is computed from font metrics and was not
seen), and any keyboard handling in the applications.

**Release note.** Menus can show underlined access keys (`Alt` letters) that pick an item with one
key press.

## 3. Snap-layouts flyout on the maximize button (`mdshared-ui::snap`)

**Built, opt-in, Windows only.** `snap::set_maximize_button_rect(window, Option<Rect>)`. Over the
rectangle (logical px, from the client origin) the window answers `WM_NCHITTEST` with
`HTMAXBUTTON`, so Windows 11 shows the snap-layouts flyout; the non-client mouse messages for it are
translated back to client ones (move, press, release, leave), so the application's own button
still gets `CursorMoved`, `CursorEntered`, `CursorLeft` and `MouseInput` and maximizes by itself.
Edge resizing, title-row dragging and every point outside the rectangle are answered exactly as
before. No winit edit was needed: winit does not handle `WM_NCHITTEST`. `SetWindowSubclass` could
not be used (comctl32 version 6 only: a binary importing it by name did not start), so the window
procedure is replaced and chained.

**Public API added.** `snap::set_maximize_button_rect`; `windows-sys` feature
`Win32_System_Threading` enabled (Cargo.lock unchanged).

**What each app does.** After laying out the caption buttons, on every resize, maximize or scale
change, call `snap::set_maximize_button_rect(&window, Some(rect_of_maximize_button))`; call with
`None` while a menu or dialog covers the title row. Keep the rectangle out of the resize band.
Then try it on Windows 11 by hand.

**Run.** Unit tests, and a test with a real hidden window and `SendMessage` that checks the
hit-test and each translated message. **Not verified.** The flyout appearing and snapping (needs a
real pointer on Windows 11); the order of `WM_NCMOUSELEAVE` and the first client move on a real
system (both handled); touch and pen; the hover and click feel of the two applications' buttons;
Linux and other targets (`cfg`-ed to a no-op, not compiled for them).

**Release note.** On Windows 11, hovering the maximize button shows the snap-layouts flyout.
