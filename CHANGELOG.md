# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/hkrhd/jufu/compare/v0.1.1...v0.1.2) - 2026-03-23

### Other

- Add auto-merge for release-plz release PRs
- Release 成功後に cargo install / brew install の smoke workflow を追加

## [0.1.1](https://github.com/hkrhd/jufu/compare/v0.1.0...v0.1.1) - 2026-03-23

### Other

- 短い日時フォーマットを入力タイムゾーン基準に固定
- cargo-dist の生成 CI を .github/workflows/release.yml に切り替え、FGPAT_RELEASE_TAP を使うため allow-dirty を追加した。旧 dist.yml は削除した。
