# markdown-shared-rs

The code that [markdown-viewer-rs](https://github.com/Kaan88/markdown-viewer-rs) (`mdview`) and
[markdown-editor-rs](https://github.com/Kaan88/markdown-editor-rs) (`mdedit`) both use: the
hand-drawn window chrome, the painter, the text engine and syntax highlighting.

**Status: in use by both applications since 2026-11-12.** Five crates:

| Crate | Holds |
|---|---|
| `mdshared-text` | `Bitmap`, `TextEngine`, the shared font context |
| `mdshared-highlight` | syntect set-up and themes, whole-block highlighting, an incremental line highlighter, and hand-written scanners for Kotlin, Swift, PowerShell and Dockerfile (no vendored grammars: all code is original and MIT, nothing third-party was copied) |
| `mdshared-ui` | `Painter`, palettes, menu, text field, folder tree, key parser, `platform`, and the generic widgets: tabs, caption buttons, scrollbar, pane pieces, tooltips and toasts, dialog frame, resize bands, the bar that offers an update, the command palette and the settings form, window placement, the window shadow, and off Windows the file dialogs, window identity and the AppImage's desktop entry |
| `mdshared-app` | Path helpers, file reading, recent files, the settings file layer, single-instance hand-over, file watching, fuzzy matching, the hand-off between the two apps (`sister`), and updating in place: the release manifest, SHA-256, the file swap and the state behind the update bar (`updater::App` names the application; on Linux only an AppImage updates itself) |
| `mdshared-fetch` | Downloads over WinHTTP (the system libcurl off Windows) with strict limits: the image fetcher with its disk cache, and `fetch_file` for an update. No dependency on the other crates |

**Linux.** Branch `feat/linux` (at `7cd6461` for mdedit 0.5.0 and mdview 0.3.0; to become `main`)
makes the crates work off Windows: downloads through the system libcurl loaded at run time, file
dialogs, self-update of an AppImage, a desktop entry with file associations (it takes no
defaults), fonts and colour emoji, the clipboard, built-in icons, the system theme from the
settings portal, window placement, the window shadow, and the hand-off between the apps. What was
run and what was not (no Windows run, KDE Plasma and sway only) is in
[docs/LINUX.md](docs/LINUX.md); the hand-off protocol is in [docs/SISTER.md](docs/SISTER.md).
`mdshared-ui`'s `client-margin` feature needs patched forks of winit and softbuffer, set through
`[patch.crates-io]` in the app (LINUX.md, "Window shadow").

Each application pins a commit of this repository in its `Cargo.toml`. The move was checked by
rendering 43 states of the viewer before and after and comparing the files byte for byte.
The window loop itself is not shared yet: each application has its own (step 4 of
[docs/PLAN.md](docs/PLAN.md)). Licence: [MIT](LICENSE).

This repository exists so the editor never copies code out of the viewer, and so the viewer's
size, memory and launch time are not touched by anything the editor needs.
