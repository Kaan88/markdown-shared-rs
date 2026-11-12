# Plan: the shared code of mdview and mdedit

Written 2026-11-12. Nothing here is built. This is the plan the user asked for before any work
starts; the decisions marked **Open** are theirs to make.

## Why this repository exists

The viewer's plan (`markdown-viewer-rs/docs/PLAN.md`, section E, decided 2026-11-11) says the
editor is a second project that "shares the shell, text and highlighting code", and that before it
starts "the shared parts of `bins/mdv-app` (window chrome, `gfx`, `textfield`, menus, theme,
`platform`) move into a library crate both binaries use". This repository is where that crate
lives, so that:

- the editor depends on a library, not on a copy of the viewer's files;
- a fix to the chrome or the text path reaches both applications;
- nothing the editor needs is linked into `mdview.exe`. The viewer's numbers (24 MiB private
  memory, about 80 ms to first frame, a 7.3 MB exe, measured 2026-11-12) are the budget this
  extraction must not move.

## What moves, and what does not

State of the viewer on 2026-11-12 (`a7f24d7`). Line counts are from `wc -l`.

| Code today | Lines | Depends on | Moves? |
|---|---|---|---|
| `crates/render/src/text.rs` (`TextEngine`: parley layout, swash raster, glyph cache, subpixel order) | 668 | parley, swash | Yes |
| `crates/render/src/bitmap.rs` (`Bitmap`: BGRA fills, blits, clip) | 545 | none | Yes |
| `crates/render/src/highlight.rs` (syntect, the two themes, the TOML scanner) | 324 | syntect | Yes |
| `shared_fonts()` (one font enumeration per thread) | small | parley | Yes |
| `bins/mdv-app/src/gfx.rs` (`Painter`: logical px, device snapping, clip, shadows, text and icons) | 531 | `Bitmap`, `TextEngine` | Yes |
| `bins/mdv-app/src/theme.rs` (chrome palettes) | 97 | `gfx` | Yes |
| `bins/mdv-app/src/menu.rs` (popup menu) | 235 | `gfx`, `theme` | Yes |
| `bins/mdv-app/src/textfield.rs` (single-line editor) | 618 | `gfx` | Yes |
| `bins/mdv-app/src/platform.rs` (clipboard, URLs, file manager, system theme, DWM, dialogs) | 466 | windows-sys, rfd | Yes |
| `bins/mdv-app/src/tree.rs` (lazy folder tree) | 388 | `mdview_core::links::is_markdown_path` | Yes, with the file filter passed in |
| `bins/mdv-app/src/keys.rs` (shortcut parsing) | 371 | `mdview_core::commands` | Partly: the parser and key names move, the viewer's command table stays |
| Window frame logic in `app.rs` (frameless resize bands, regions and signatures, damage present, hidden first frame, idle timers) | part of 1199 | winit, softbuffer | Yes, as the last step; it is the least separable |
| Title row, tabs, scrollbar drawing in `chrome.rs`, `panes.rs` | part of 1200 | `gfx`, `ui` | The generic parts (tab strip, window buttons, overlay scrollbar, tooltip, toast, dialog card) |
| `crates/core` session file format, recents, single instance, file watching | | | **Open**, see below |
| The Markdown parser, document model, page layout, engine, fetch | | | No. They are the viewer |
| `workspace.rs`, `page.rs`, the Outline pane, remote images, task boxes | | | No. They are the viewer |

## Shape

One Cargo workspace, three crates, so an application links only what it uses:

| Crate | Holds | Notes |
|---|---|---|
| `mdshared-text` | `Bitmap`, `TextEngine`, `shared_fonts`, `Subpixel` | No window system. The viewer's renderer depends on this instead of owning it |
| `mdshared-highlight` | syntect set-up, themes, language lookup, the TOML scanner | Separate because syntect is the largest dependency and the editor will need an incremental interface the viewer does not |
| `mdshared-ui` | `Painter`, palettes, menu, text field, tree, tooltip, toast, dialog card, scrollbar, tab strip, window frame, `platform` | Depends on `mdshared-text`, winit and softbuffer |

Names are placeholders. **Open:** the user may prefer other names.

## How the two applications depend on it

A git dependency pinned to a commit, in each application's `Cargo.toml`:

```toml
mdshared-ui = { git = "https://github.com/Kaan88/markdown-shared-rs", rev = "<commit>" }
```

- A pinned `rev` means a change here never alters a released application by accident; each
  application moves to a new commit on purpose and runs its own tests then.
- For work that touches both sides at once, a local `[patch]` entry (kept out of git, in
  `.cargo/config.local.toml`, which the viewer already ignores) points at a checkout on disk.
- All three repositories are private; cargo reaches them through the git credentials already on
  the PC. Not verified: that `cargo` picks up the `gh` credential helper here without extra
  set-up. Check this before step 1.
- No crates.io publishing.

## Order of work

Each step is one change to this repository and one to the viewer, and ends with the viewer's own
checks. The editor does not start until step 4 is done.

| Step | Work | Done when |
|---|---|---|
| 0 | Confirm the git dependency works from the PC with a private repository (an empty crate, one function) | `cargo build` of the viewer fetches it without a prompt |
| 1 | `mdshared-text`: move `Bitmap`, `TextEngine`, `shared_fonts`. The viewer's `mdview-render` re-exports them, so no other viewer code changes | The four `showcase-*` goldens match byte for byte; workspace tests pass; exe size and first-frame time within noise of before |
| 2 | `mdshared-highlight`: move `highlight.rs` | Same checks; highlighted fixtures render identically |
| 3 | `mdshared-ui`, the leaf modules: `gfx`, `theme`, `menu`, `textfield`, `platform`, then `tree` and the key parser | `--shot` renders of both themes, the four menus and the dialogs match the renders taken before the move, byte for byte |
| 4 | `mdshared-ui`, the window frame: resize bands, regions and signatures, damage present, timers, as a small framework the application plugs its regions into | The same shot comparison; the scroll-frame timing (`--scroll-steps`) within noise; the user uses the app and approves, since this step can change feel |
| 5 | Tag `v0.1.0` here; the viewer and the editor pin it | Both build from the tag |

Step 4 is the risky one. `app.rs` mixes generic window handling with viewer behaviour, and the
split has to be designed, not just moved. If it turns out badly, the fallback is to stop after
step 3: the editor then writes its own `app.rs` on top of the shared widgets, which duplicates
roughly a thousand lines but keeps both applications independent.

## Rules carried over from the viewer

- Report results plainly, including what was not run. Nothing counts as working on Windows until
  it has run on Windows.
- The user approves the interface by looking at it. Screenshots and tests do not stand in for that.
- No bundled fonts; no DLLs beyond the system's; `unsafe` only in `platform`.
- No CI. Run the tests before a push.
- Linux is low priority until the Windows applications are settled.

## Open decisions

1. **Session, recents, single instance, file watching.** These live in the viewer's `crates/core`
   and the editor needs all four. Either they move here as a fourth crate (`mdshared-app`), or the
   editor depends on the viewer's `mdview-core` directly. Moving them is cleaner; depending on
   `mdview-core` is less work and drags the Markdown parser into the editor's build (not into its
   exe if unused). Recommendation: move them, after step 3.
2. **One repository or three.** The alternative to all of this is one workspace holding the
   viewer, the editor and the shared crates. It removes the pinning and the cross-repository
   steps. The user asked for separate repositories on 2026-11-12, so this plan follows that; it is
   listed here because it is the cheaper arrangement if the split proves tiresome.
3. **Crate names**, above.
4. **Licence.** The viewer has no licence file and all three repositories are private. Nothing
   needs deciding unless one becomes public.
