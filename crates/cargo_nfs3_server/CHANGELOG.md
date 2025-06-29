# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.1](https://github.com/Vaiz/nfs3/compare/cargo-nfs3-server-v0.1.0-alpha.2...cargo-nfs3-server-v0.0.1) - 2025-06-29

### Changes

- re-export nfs3_types from nfs3_server and nfs3_client crates ([#94](https://github.com/Vaiz/nfs3/pull/94))
- fix new clippy issues from recent Rust update ([#97](https://github.com/Vaiz/nfs3/pull/97))
- drop xdr-codec dependency ([#98](https://github.com/Vaiz/nfs3/pull/98))

## [0.1.0-alpha.2](https://github.com/Vaiz/nfs3/compare/cargo-nfs3-server-v0.1.0-alpha.1...cargo-nfs3-server-v0.1.0-alpha.2) - 2025-06-15

### Changes

- MemFs now supports `rename` and `create_exclusive`
