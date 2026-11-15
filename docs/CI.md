# CI (GitHub Actions)

`.github/workflows/ci.yml` runs on a push to `main`, `feat/linux` or `f/macos`, on every pull request and by
hand (`workflow_dispatch`); a newer run of the same branch cancels the older one. Permissions:
`contents: read`; no secrets.

Three jobs, `linux` (ubuntu-latest), `windows` (windows-latest, MSVC, natively) and `macos` (a
matrix of two real Macs: `macos-15` for Apple Silicon, `macos-15-intel` for Intel; `macos-13` is
retired), each:

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

No test is skipped on either system and no step is allowed to fail.

Finding from the first runs: on real Windows `paths::tests::canonical_paths_have_one_identity_on_windows`
failed because `%TEMP%` on the runner is the 8.3 short path (`C:\Users\RUNNER~1\...`): a file
that exists was canonicalized to the long form, a missing one kept the short spelling, so one
file had two identities. `canonical_or_absolute` now canonicalizes the nearest existing folder of
a missing file, so the identity is the same with or without the file (and the CI job needs no
`TEMP` override).

Known limits: no ARM Windows or ARM Linux; the tests run headless, so nothing here opens a window.
