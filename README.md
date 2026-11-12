# markdown-shared-rs

The code that [markdown-viewer-rs](https://github.com/Kaan88/markdown-viewer-rs) (`mdview`) and
[markdown-editor-rs](https://github.com/Kaan88/markdown-editor-rs) (`mdedit`) both use: the
hand-drawn window chrome, the painter, the text engine and syntax highlighting.

**Status: in use by both applications since 2026-11-12.** Four crates:

| Crate | Holds |
|---|---|
| `mdshared-text` | `Bitmap`, `TextEngine`, the shared font context |
| `mdshared-highlight` | syntect set-up and themes, whole-block highlighting, and an incremental line highlighter |
| `mdshared-ui` | `Painter`, palettes, menu, text field, folder tree, key parser, `platform`, and the generic widgets: tabs, caption buttons, scrollbar, pane pieces, tooltips and toasts, dialog frame, resize bands |
| `mdshared-app` | Path helpers, file reading, recent files, the settings file layer, single-instance hand-over, file watching |

Each application pins a commit of this repository in its `Cargo.toml`. The move was checked by
rendering 43 states of the viewer before and after and comparing the files byte for byte.
The window loop itself is not shared yet: each application has its own (step 4 of
[docs/PLAN.md](docs/PLAN.md)). Licence: [MIT](LICENSE).

This repository exists so the editor never copies code out of the viewer, and so the viewer's
size, memory and launch time are not touched by anything the editor needs.
