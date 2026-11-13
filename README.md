# markdown-shared-rs

The code that [markdown-viewer-rs](https://github.com/Kaan88/markdown-viewer-rs) (`mdview`) and
[markdown-editor-rs](https://github.com/Kaan88/markdown-editor-rs) (`mdedit`) both use: the
hand-drawn window chrome, the painter, the text engine and syntax highlighting.

**Status: in use by both applications since 2026-11-12.** Five crates:

| Crate | Holds |
|---|---|
| `mdshared-text` | `Bitmap`, `TextEngine`, the shared font context |
| `mdshared-highlight` | syntect set-up and themes, whole-block highlighting, an incremental line highlighter, and hand-written scanners for Kotlin, Swift, PowerShell and Dockerfile (no vendored grammars: all code is original and MIT, nothing third-party was copied) |
| `mdshared-ui` | `Painter`, palettes, menu, text field, folder tree, key parser, `platform`, and the generic widgets: tabs, caption buttons, scrollbar, pane pieces, tooltips and toasts, dialog frame, resize bands, the bar that offers an update |
| `mdshared-app` | Path helpers, file reading, recent files, the settings file layer, single-instance hand-over, file watching, and updating in place: the release manifest, SHA-256, the file swap and the state behind the update bar (`updater::App` names the application) |
| `mdshared-fetch` | Downloads over WinHTTP with strict limits: the image fetcher with its disk cache, and `fetch_file` for an update. No dependency on the other crates |

Each application pins a commit of this repository in its `Cargo.toml`. The move was checked by
rendering 43 states of the viewer before and after and comparing the files byte for byte.
The window loop itself is not shared yet: each application has its own (step 4 of
[docs/PLAN.md](docs/PLAN.md)). Licence: [MIT](LICENSE).

This repository exists so the editor never copies code out of the viewer, and so the viewer's
size, memory and launch time are not touched by anything the editor needs.
