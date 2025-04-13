# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1](https://github.com/Vaiz/nfs3/compare/nfs3_server-v0.4.0...nfs3_server-v0.4.1) - 2025-04-13

### Added

- major rework of nfs3_server request handlers ([#69](https://github.com/Vaiz/nfs3/pull/69))
- apply new clippy rules to nfs3_types, nfs3_macros ([#66](https://github.com/Vaiz/nfs3/pull/66))
- embrace clippy ([#64](https://github.com/Vaiz/nfs3/pull/64))
- add fragment_header type ([#61](https://github.com/Vaiz/nfs3/pull/61))

### Other

- add rpc tests ([#62](https://github.com/Vaiz/nfs3/pull/62))
- limit number of tracked requests ([#60](https://github.com/Vaiz/nfs3/pull/60))
- improve transaction tracker performance ([#58](https://github.com/Vaiz/nfs3/pull/58))

## [0.4.0](https://github.com/Vaiz/nfs3/compare/nfs3_server-v0.3.0...nfs3_server-v0.4.0) - 2025-03-23

### Changes

- add iterator traits for readdir, readdirplus methods ([#54](https://github.com/Vaiz/nfs3/pull/54))
- fix readdir implementation
- add memfs implementation
- add tests for readdir and readdirplus ([#52](https://github.com/Vaiz/nfs3/pull/52))

## [0.3.0](https://github.com/Vaiz/nfs3/compare/nfs3_server-v0.2.0...nfs3_server-v0.3.0) - 2025-03-02

### Changes

- [**breaking**] update to Rust 2024 ([#32](https://github.com/Vaiz/nfs3/pull/32))
- reimplement readdirplus function ([#34](https://github.com/Vaiz/nfs3/pull/34))
- remove xdr_codec dependency ([#33](https://github.com/Vaiz/nfs3/pull/33))
- test basic functionality ([#37](https://github.com/Vaiz/nfs3/pull/37))
- add nfs3_tests crate ([#35](https://github.com/Vaiz/nfs3/pull/35))
- update formatting rules ([#30](https://github.com/Vaiz/nfs3/pull/30))

## [0.1.0](https://github.com/Vaiz/nfs3/releases/tag/nfs3_server-v0.1.0) - 2025-02-09

### Changes

- remove xdrgen module ([#6](https://github.com/Vaiz/nfs3/pull/6))
- update docs ([#5](https://github.com/Vaiz/nfs3/pull/5))
- move dependencies to workspace level ([#4](https://github.com/Vaiz/nfs3/pull/4))
- split into smaller crates, rewrite XDR encoding ([#5](https://github.com/Vaiz/nfs3/pull/5))
