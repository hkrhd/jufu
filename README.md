# jufu

Jujutsu log viewer TUI

## Install

### crates.io

```bash
cargo install jufu
```

### Homebrew / Linuxbrew

```bash
brew tap hkrhd/homebrew-tap
brew install hkrhd/homebrew-tap/jufu
```

## Release

Releases are published from GitHub Actions.

- Push to `main` to create or update the release PR.
- The `release-plz: prepare release` PR is auto-merged after CI passes.
- The merge triggers the `Release` workflow, which publishes to crates.io, pushes the `v*` tag, creates the GitHub Release, updates the Homebrew tap, and runs the smoke workflow to verify `cargo install` and `brew install`.

## Development

```bash
cargo run --manifest-path path/to/project/Cargo.toml
```
