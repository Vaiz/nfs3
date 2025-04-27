# NFS3

A Rust implementation of an NFS3 protocol, including NFS3 server and client.

## Overview

This project provides a Rust-based implementation of an NFS3 protocol. It includes various components and dependencies to facilitate the development, deployment, and interaction with an NFS server. The project is designed to be modular, with separate crates for different functionalities.

## Project Structure

- `crates/cargo_nfs3_server`: A lightweight and easy-to-deploy NFSv3 server tool for quick file transfers.
- `crates/nfs3_macros`: Procedural macros used in the project.
- `crates/nfs3_types`: Types and utilities for NFS operations.
- `crates/nfs3_server`: Async NFS3 server implementation.
- `crates/nfs3_client`: Async NFS3 client implementation.

## NFS3 Server

The `cargo-nfs3-server` tool allows you to quickly set up an NFSv3 server. It supports features like in-memory filesystems and read-only mode. For more details, refer to the [cargo-nfs3-server README](crates/cargo_nfs3_server/README.md).

## NFS3 Server Lib

The `nfs3_server` crate provides an asynchronous implementation of an NFSv3 server. It is designed to be modular and extensible, allowing developers to implement custom virtual file systems by adhering to the provided `NfsReadFileSystem` and `NfsFileSystem` traits. This enables the creation of both read-only and writable NFS servers tailored to specific use cases. For more details, refer to the [nfs3_server README](crates/nfs3_server/README.md).

## NFS3 Client Lib

The `nfs3_client` crate provides an asynchronous client for interacting with NFS3 servers. It supports various NFS operations and handles the underlying RPC communication. For examples and usage, refer to the [nfs3_client README](crates/nfs3_client/README.md).
