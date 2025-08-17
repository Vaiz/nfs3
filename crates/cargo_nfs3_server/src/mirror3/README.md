# Mirror3 NFS Filesystem

Mirror3 is a minimal NFS3 filesystem implementation that provides read-only access to the underlying filesystem through an NFS interface. It caches only file names and paths for handle resolution, with all metadata read directly from the filesystem.

## Features

- **Minimal Caching**: Only caches file names and paths for handle resolution
- **Fresh Metadata**: All metadata (size, timestamps, permissions) is read directly from the filesystem
- **No Children Cache**: Directory contents are always read fresh from the filesystem
- **Independent Iterator**: Directory traversal logic is decoupled from the main filesystem implementation
- **Read-only Access**: Implements the `NfsReadFileSystem` trait for safe read-only operations

## Design

### Cache Behavior
- **Path Resolution**: Maps file handles to filesystem paths for efficient lookups
- **No Metadata Caching**: File attributes, sizes, and timestamps are always read from disk
- **No Directory Caching**: Directory listings are generated fresh on each request

### Iterator Independence
- **Modular Design**: Directory iteration logic is separated into its own module (`iterator.rs`)
- **Split Iterators**: Separate `Mirror3ReadDirIterator` and `Mirror3ReadDirPlusIterator` for optimal performance
- **Shared Core Logic**: Common directory entry creation logic is factored into a shared helper function
- **Consistent Ordering**: Directory entries are sorted by file ID for predictable iteration

### Architecture
- `mod.rs`: Main filesystem implementation with `NfsReadFileSystem` trait
- `iterator.rs`: Independent directory iteration logic with separate iterators:
  - `Mirror3ReadDirIterator`: Lightweight iterator implementing `ReadDirIterator`
  - `Mirror3ReadDirPlusIterator`: Full-featured iterator implementing `ReadDirPlusIterator`
- **Cache**: Minimal symbol table for path-to-handle mapping
- **Root Path**: Base directory that the NFS filesystem mirrors

## Usage

The Mirror3 filesystem can be used as a command-line argument:
```bash
cargo-nfs3-server --mirrorfs3 /path/to/directory
```

This provides a read-only NFS3 interface to the specified directory with minimal caching and fresh metadata on every access.
