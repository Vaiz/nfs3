# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0](https://github.com/Vaiz/nfs3/compare/nfs3_server-v0.6.0...nfs3_server-v0.7.0) - 2025-06-29

### Added

- [**breaking**] pass arguments by reference ([#99](https://github.com/Vaiz/nfs3/pull/99))
- [**breaking**] drop xdr-codec dependency ([#98](https://github.com/Vaiz/nfs3/pull/98))
- re-export nfs3_types from nfs3_server and nfs3_client crates ([#94](https://github.com/Vaiz/nfs3/pull/94))

### Fixed

- new clippy issues from recent Rust update ([#97](https://github.com/Vaiz/nfs3/pull/97))

## [0.6.0](https://github.com/Vaiz/nfs3/compare/nfs3_server-v0.5.0...nfs3_server-v0.6.0) - 2025-06-15

### Added

- [**breaking**] add createverf parameter to `NfsFileSystem::create_exclusive` method ([#90](https://github.com/Vaiz/nfs3/pull/90))
- implement rename for MemFs ([#91](https://github.com/Vaiz/nfs3/pull/91))
- set MSRV to 1.85 ([#85](https://github.com/Vaiz/nfs3/pull/85))

## [0.5.0](https://github.com/Vaiz/nfs3/compare/nfs3_server-v0.4.1...nfs3_server-v0.5.0) - 2025-04-27

### Added

- [**breaking**] add FileHandle trait ([#76](https://github.com/Vaiz/nfs3/pull/76))
- [**breaking**] split NFSFileSystem trait into two ([#75](https://github.com/Vaiz/nfs3/pull/75))
- [**breaking**] remove async_trait dependency ([#70](https://github.com/Vaiz/nfs3/pull/70))

## [0.4.1](https://github.com/Vaiz/nfs3/compare/nfs3_server-v0.4.0...nfs3_server-v0.4.1) - 2025-04-13

### Changes

- improve transaction tracker performance ([#58](https://github.com/Vaiz/nfs3/pull/58))
- major rework of nfs3_server request handlers ([#69](https://github.com/Vaiz/nfs3/pull/69))
- return `GARBAGE_ARGS` if RPC request cannot be parsed ([#63](https://github.com/Vaiz/nfs3/pull/63))
- apply new clippy rules ([#64](https://github.com/Vaiz/nfs3/pull/64))

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
