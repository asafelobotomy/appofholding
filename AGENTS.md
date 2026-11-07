# appOFholding

Rust workspace that keeps AppImages tidy, updated and (optionally) sandboxed with bubblewrap. A Qt 6/Kirigami app (`appofholding-gui`) and a CLI (`appofholding`) share one library.

## Commands
- Build: `make` (release, `cargo build --release --locked --workspace`); install: `make install PREFIX=/usr` (needs root; use `run0`).
- Test (all, needs Qt): `make test` or `cargo test --locked --workspace`.
- Test without Qt (core + CLI, as CI's `core` job): `cargo test --locked -p appofholding-core -p appofholding`.
- Format: `cargo fmt --all --check`.
- Lint: `cargo clippy --locked -p appofholding-core -p appofholding --all-targets -- -D warnings` (no Qt); add `--workspace` instead of `-p` for the GUI too.
- QML lint (CI, non-blocking): `/usr/lib/qt6/bin/qmllint crates/gui/qml/*.qml`.
- Run: `cargo run -p appofholding -- --help`; GUI: `cargo run -p appofholding-gui`.
- `scripts/smoke.sh` is a live-desktop smoke test (real home, real systemd, windows appear). It refuses to run if `~/Applications` or appOFholding data already exist. Do not run it casually.

## Layout
- `crates/core`: library (no Qt). `crates/cli`: `appofholding` binary, tests in `crates/cli/tests`.
- `crates/gui`: Qt/Kirigami GUI; QML in `crates/gui/qml`, `build.rs` does the Qt build.
- `docs/`: configuration, importing, sandbox, embedding-update-info. `packaging/`: arch (PKGBUILD), rpm spec, deb, appimage, desktop file, metainfo.

## Conventions
- Rust edition 2024, MSRV 1.89, GPL-3.0-or-later. Work on `main`; CI runs on push to main and PRs.
- CI uses `--locked`, so commit `Cargo.lock` changes.
- Releases: push a tag `vX.Y.Z`; the Release workflow builds the Arch package in an archlinux container and publishes it. It fails unless the tag equals the version in all of: `Cargo.toml` (workspace version), `packaging/arch/PKGBUILD` (`pkgver=`), `packaging/rpm/appofholding.spec` (`Version:`), `packaging/io.github.appofholding.metainfo.xml` (a `release version=`), and a `## X.Y.Z` section in `CHANGELOG.md` (used as release notes). Bump all of them together.

## Gotchas
- The GUI must not link Qt private API: CI fails if `nm -D --undefined-only` on `appofholding-gui` shows `Qt_6_PRIVATE_API` (see `crates/gui/build.rs`).
- Building the GUI needs Qt 6 (base, declarative, svg), Kirigami, CMake and a C++ compiler. Runtime: `bubblewrap`, `xdg-dbus-proxy`; optional `zsync`, `libnotify`.
- Use `run0` for `pacman -U` (the self-update path does).
