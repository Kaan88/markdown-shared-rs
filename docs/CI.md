# CI (GitHub Actions)

`.github/workflows/ci.yml` runs on a push to `main` or `feat/linux`, on every pull request and by
hand (`workflow_dispatch`); a newer run of the same branch cancels the older one. Permissions:
`contents: read`; no secrets.

Two jobs, `linux` (ubuntu-latest) and `windows` (windows-latest, MSVC, natively), each:

1. `cargo fmt --all --check`
2. `cargo clippy --release --workspace --all-targets -- -D warnings`
3. `cargo test --release --workspace`

The Rust toolchain is pinned in the workflow (`RUST_TOOLCHAIN`, 1.99.1, `dtolnay/rust-toolchain`);
there is no `rust-toolchain.toml`. `Swatinem/rust-cache` caches the registry and `target`. The
Linux job installs the system packages the windowing crates link against (fontconfig, freetype,
X11/xcb, xkbcommon, cmake).

The two applications (mdedit, mdview) pin these crates by commit, so a change here reaches them
only when their pin moves; their own CI builds against the pinned commit.

There is no release workflow here: the crates are not published, the applications' releases are
made in their own repositories (`docs/CI.md` there).

Known limits: x86_64 only, no macOS; the tests run headless, so nothing here opens a window.
