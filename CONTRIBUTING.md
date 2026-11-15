# Contributing to appofholding

1. Fork the repository and create a branch from `main`.
2. Make your change and add or update tests.
3. Run the checks CI runs:
   - `cargo test --locked -p appofholding-core -p appofholding` tests the library and CLI
     without Qt. `cargo test --locked --workspace` (or `make test`) tests the GUI too and needs
     Qt 6 (base, declarative, svg), Kirigami, CMake and a C++ compiler.
   - `cargo fmt --all --check`
   - `cargo clippy --locked -p appofholding-core -p appofholding --all-targets -- -D warnings`
     (use `--workspace` instead of the `-p` flags to lint the GUI as well).
4. Open a pull request describing what changed and why.
