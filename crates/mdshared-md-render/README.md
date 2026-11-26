# Show a Markdown page

These crates are the headless page renderer from `markdown-viewer-rs` at `01f67b2`.
Use `mdshared_md_core::parse` (or `parse_bytes`) to get a `Document`, then pass it to
`mdshared_md_render::render` for a bitmap, or keep a `DocumentRenderer` for a scrollable
view. The application owns the window, input, filesystem choices, remote image fetching,
and opening links. `export_html` makes a standalone HTML page.

| Package | Use it for |
|---|---|
| `mdshared-md-core` | Parsing, model, links, find, task boxes and rich selection |
| `mdshared-md-render` | Page layout, CPU drawing, image decoding and HTML export |
| `mdshared-md-engine-proto` | The versionless wire format used by the renderer and the engine |
| `mdshared-md-engine` | The separate rasterizer for SVG, TeX and Mermaid; its binary is still `mdview-engine` |
| `mdshared-md-preview` | The `MDVPREVIEW7` protocol; enable `server` for `Server`, `serve` and `serve_stdio` |

Existing callers can keep import names by aliasing the packages in their own manifests:

```toml
[dependencies]
mdview-core = { package = "mdshared-md-core", git = "https://github.com/Kaan88/markdown-shared-rs", rev = "<toolkit commit>" }
mdview-render = { package = "mdshared-md-render", git = "https://github.com/Kaan88/markdown-shared-rs", rev = "<toolkit commit>", optional = true }
mdview-preview = { package = "mdshared-md-preview", git = "https://github.com/Kaan88/markdown-shared-rs", rev = "<toolkit commit>" }

[features]
embedded-preview = ["mdview-preview/server", "dep:mdview-render"]
```

Preview defaults are empty; a protocol-only client needs no parser or renderer.
Core uses `mdshared-app` with defaults off, for `notes`, `wikilink` and their `files` and
`paths` helpers. That adds no third-party dependencies through `mdshared-app`. If the same
build also uses `mdshared-ui` or directly requests `mdshared-app`'s defaults, Cargo unifies
features and the full application plumbing still builds.

## Ship the engine

Build it with `cargo build --release -p mdshared-md-engine --bin mdview-engine`. Ship
`mdview-engine` (`mdview-engine.exe` on Windows) next to your executable, or supply its
location using `set_engine_path`. The renderer looks in this order:

1. The path passed to `mdshared_md_render::set_engine_path`.
2. The `MDV_ENGINE` environment variable.
3. A sibling of the running executable.
4. A development fallback: walking up from the executable directory, it checks for a binary
   directly in each directory and in its `target/release`, at most six directories in all.

An explicit path that does not exist does not fall back. A third app must set the engine
path or ship the binary for SVG, TeX and Mermaid to render; the rest of the page does not
require the engine. Neither the renderer nor the preview server builds it as a dependency.
`cargo test --workspace` builds it for the workspace tests. If testing only the renderer,
build the engine first in the same Cargo target directory and profile.

## Release settings

Profiles belong in the application's workspace root. The source app used the following
settings; the package name in the override, unlike the executable name, changes with the move.
The original key was `[profile.release.package.mdview-engine]`.

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "unwind"

[profile.release.package.mdshared-md-engine]
opt-level = "s"
```
