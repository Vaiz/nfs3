# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.2](https://github.com/Vaiz/nfs3/compare/nfs3_client-v0.4.1...nfs3_client-v0.4.2) - 2025-06-15

### Changes

- set MSRV to 1.85 ([#85](https://github.com/Vaiz/nfs3/pull/85))

## [0.4.1](https://github.com/Vaiz/nfs3/compare/nfs3_client-v0.4.0...nfs3_client-v0.4.1) - 2025-04-27

### Changes

- update dependencies

## [0.4.0](https://github.com/Vaiz/nfs3/compare/nfs3_client-v0.3.0...nfs3_client-v0.4.0) - 2025-04-13

### Changes

- make nfs3_client operations work with default tokio scheduler ([#68](https://github.com/Vaiz/nfs3/pull/68))
- apply new clippy rules ([#65](https://github.com/Vaiz/nfs3/pull/65))

## [0.3.0](https://github.com/Vaiz/nfs3/compare/nfs3_client-v0.2.0...nfs3_client-v0.3.0) - 2025-03-23

### Changes

- connect to nfs share from privileged ports by default ([#55](https://github.com/Vaiz/nfs3/pull/55))
- allow to set credentials and verifier for RPC connection
- fix tokio features
- add new examples: download_folder and ls ([#57](https://github.com/Vaiz/nfs3/pull/57))

## [0.2.0](https://github.com/Vaiz/nfs3/compare/nfs3_client-v0.1.1...nfs3_client-v0.2.0) - 2025-03-02

### Changes

- fix ACCESS3res type ([#36](https://github.com/Vaiz/nfs3/pull/36))
- [**breaking**] update to Rust 2024 ([#32](https://github.com/Vaiz/nfs3/pull/32))

## [0.1.1](https://github.com/Vaiz/nfs3/compare/nfs3_client-v0.1.0...nfs3_client-v0.1.1) - 2025-02-23

### Changes

- enable net feature in tokio ([#28](https://github.com/Vaiz/nfs3/pull/28))

## [0.1.0](https://github.com/Vaiz/nfs3/releases/tag/nfs3_client-v0.1.0) - 2025-02-22

### Changes

- Add nfs3_client crate ([#23](https://github.com/Vaiz/nfs3/pull/23))
