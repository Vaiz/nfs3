# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.2](https://github.com/Vaiz/nfs3/compare/nfs3_types-v0.3.1...nfs3_types-v0.3.2) - 2025-04-13

### Changes

- apply new clippy rules ([#66](https://github.com/Vaiz/nfs3/pull/66))
- add `fragment_header` type ([#61](https://github.com/Vaiz/nfs3/pull/61))
- add `Display` trait for some enums
- add `PackedSize` implementation for mountres3 
- fix `PackedSize` implementation for `Nfs3Result`
- add `nfspath3::into_owned` method

## [0.3.1](https://github.com/Vaiz/nfs3/compare/nfs3_types-v0.3.0...nfs3_types-v0.3.1) - 2025-03-23

### Changes

- add `opaque_auth::clone`, `opaque_auth::auth_unix`, `opaque_auth::borrow` methods
- add `List::is_empty` method


## [0.3.0](https://github.com/Vaiz/nfs3/compare/nfs3_types-v0.2.0...nfs3_types-v0.3.0) - 2025-03-02

### Changes

- fix ACCESS3res type ([#36](https://github.com/Vaiz/nfs3/pull/36))
- [**breaking**] update to Rust 2024 ([#32](https://github.com/Vaiz/nfs3/pull/32))

## [0.1.0](https://github.com/Vaiz/nfs3/releases/tag/nfs3_types-v0.1.0) - 2025-02-09

### Changes

- split into smaller crates, rewrite XDR encoding ([#5](https://github.com/Vaiz/nfs3/pull/5))
