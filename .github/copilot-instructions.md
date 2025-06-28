# Copilot Instructions

## Overview

This repository contains a Rust implementation of an NFS3 protocol with server and client components. The development environment is equipped with a comprehensive Rust MCP (Model Context Protocol) server that provides specialized tools for Rust development.

## Rust MCP Server Tools

The following Rust-specific tools are available through the MCP server and should be used for all Rust development tasks:

### Core Build & Test Tools

- **Generic arguments**:
  - Use `package: "crate-name"` to build a specific crate
  - Use `all_features: true` to build with all features enabled
  - Use `all_targets: true` to build all targets (e.g., tests, examples)
  - Use `warnings_as_errors: true` for strict checking

- **`Rust-cargo-build`**: Build the project using Cargo

- **`Rust-cargo-check`**: Fast compilation check without code generation
  - Preferred for quick validation during development
  - Use before making code changes to understand existing issues

- **`Rust-cargo-test`**: Run tests

- **`Rust-cargo-clippy`**: Advanced linting with Clippy
  - Use `fix: true` to automatically apply fixes for Clippy warnings
  - Use `allow_dirty: true` to allow dirty workspaces (useful for CI/CD)

- **`Rust-cargo-fmt`**: Code formatting with rustfmt
  - Use `all: true` to format all packages
  - Use `check: true` to verify formatting without applying changes

### Dependency Management

- **`Rust-cargo-add`**: Add dependencies to Cargo.toml
- **`Rust-cargo-remove`**: Remove dependencies from Cargo.toml
- **`Rust-cargo-update`**: Update dependencies in Cargo.lock
- **`Rust-cargo-machete`**: Find unused dependencies
- **`Rust-cargo-deny-check`**: Security and compliance checking

### Development Workflow

- **`Rust-cargo-metadata`**: Get project metadata in JSON format
- **`Rust-cargo-search`**: Search for crates on crates.io
- **`Rust-cargo-info`**: Get information about specific crates

## Project Structure

```
nfs3/
├── crates/
│   ├── cargo_nfs3_server/     # Lightweight NFSv3 server tool
│   ├── nfs3_client/          # Async NFS3 client implementation
│   ├── nfs3_server/          # Async NFS3 server implementation
│   ├── nfs3_types/           # Types and utilities for NFS operations
│   ├── nfs3_macros/          # Procedural macros
│   └── nfs3_tests/           # Integration tests
├── .github/workflows/        # CI/CD workflows
└── scripts/                  # Build and utility scripts
```

## AI Agent Guidelines

### 1. Always Use Rust MCP Tools

- **DO**: Use `Rust-cargo-build` instead of `bash` commands like `cargo build`
- **DO**: Use `Rust-cargo-check` for quick validation
- **DO**: Use `Rust-cargo-clippy` for linting instead of manual clippy commands
- **WHY**: MCP tools provide better defaults, better structured output and better error handling

### 2. Development Workflow

When working on code changes:

1. **Check current state**: Use `Rust-cargo-check` with `all_targets: true, all_features: true`
2. **Make changes**: Edit code using appropriate tools
3. **Validate**: Use `Rust-cargo-clippy` with `workspace: true, all_targets: true`
4. **Format**: Use `Rust-cargo-fmt` with `all: true`
5. **Test**: Use `Rust-cargo-test` with `all_features: true`
6. **Build**: Use `Rust-cargo-build` with `all_targets: true, all_features: true` for final verification
7. **Check unused dependencies**: Use `Rust-cargo-machete` to find unused dependencies
8. **Check security compliance**: Use `Rust-cargo-deny-check` to verify security and compliance

### 3. Dependency Management

- When adding dependencies, prefer workspace-level dependencies in the root `Cargo.toml`

### 4. Code Quality Standards

This project follows strict code quality standards:

- **Clippy**: All clippy warnings must be addressed
- **Formatting**: Code must be formatted with rustfmt using nightly toolchain
- **Tests**: All changes must maintain test coverage
- **Documentation**: Public APIs must be documented

### 5. Common Patterns

#### Checking Project Health
```
1. Rust-cargo-check (all_features: true, all_targets: true)
2. Rust-cargo-clippy (warnings_as_errors: true)
3. Rust-cargo-test (all_features: true)
4. Rust-cargo-fmt (check: true)
```

#### Fixing Clippy Issues
```
1. Rust-cargo-clippy (all_targets: true, fix: true, allow_dirty: true)
2. Rust-cargo-fmt (all: true)
3. Rust-cargo-test (all_features: true)
```

#### Adding New Dependencies
```
1. Rust-cargo-add (package: "dependency-name", workspace: true if workspace dep)
2. Rust-cargo-deny-check (verify security compliance)
```

## Repository-Specific Context

### NFS3 Protocol Implementation

This project implements the NFS3 protocol in Rust with the following key components:

- **Types**: Core NFS3 types and RPC definitions
- **Server**: Async server implementation with pluggable VFS backends
- **Client**: Async client for NFS3 operations
- **Testing**: Comprehensive integration tests including WASM targets

### Key Files to Understand

- `crates/nfs3_types/src/lib.rs`: Core NFS3 type definitions
- `crates/nfs3_server/src/vfs/mod.rs`: Virtual file system traits
- `crates/nfs3_client/src/lib.rs`: Client API
- `crates/nfs3_tests/src/`: Integration test suites
