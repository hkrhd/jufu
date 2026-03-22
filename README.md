# jufu

Jujutsu log viewer TUI

## Install

### crates.io

```bash
cargo install jufu
```

### Homebrew / Linuxbrew

```bash
brew install hkrhd/homebrew-jufu/jufu
```

## Release

Releases are published from GitHub Actions.

- `release-plz` creates the release PR, publishes to crates.io, and pushes the `v*` tag.
- `dist` builds release artifacts for macOS arm64, Linux x86_64, and Linux arm64, creates the GitHub Release, and updates the Homebrew tap.

## Development

```bash
cargo run --manifest-path path/to/project/Cargo.toml
```
