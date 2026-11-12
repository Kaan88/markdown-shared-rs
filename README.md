# markdown-shared-rs

The code that [markdown-viewer-rs](https://github.com/Kaan88/markdown-viewer-rs) (`mdview`) and
[markdown-editor-rs](https://github.com/Kaan88/markdown-editor-rs) (`mdedit`) both use: the
hand-drawn window chrome, the painter, the text engine and syntax highlighting.

**Status: plan only.** Nothing has been moved here yet. The code still lives in the viewer's
repository. [docs/PLAN.md](docs/PLAN.md) says what moves, in what order, and how each step is
checked.

This repository exists so the editor never copies code out of the viewer, and so the viewer's
size, memory and launch time are not touched by anything the editor needs.
