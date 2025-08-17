# MirrorFs2 Implementation

## Overview

`MirrorFs2` is a new filesystem implementation for the NFS3 server that optimizes memory usage by caching only file names and paths for handle resolution, while loading all other metadata directly from the underlying filesystem on demand.

## Key Differences from MirrorFs

### Memory Usage
- **MirrorFs**: Caches complete file metadata (fattr3) and directory children for all discovered files
- **MirrorFs2**: Only caches file paths for handle resolution, no metadata or children caching

### Metadata Loading
- **MirrorFs**: Returns cached metadata, refreshes when changes detected
- **MirrorFs2**: Always loads metadata from filesystem when requested (getattr, readdirplus)

### Directory Listing
- **MirrorFs**: Caches directory children and compares metadata for changes
- **MirrorFs2**: Reads directory contents from filesystem every time

## Architecture

### Core Components

1. **MirrorFs2** (`filesystem.rs`)
   - Main filesystem implementation
   - Implements `NfsReadFileSystem` and `NfsFileSystem` traits
   - Handles all NFS operations
   - Delegates metadata loading to filesystem

2. **FSMap** (`fs_map.rs`)
   - Path-to-ID mapping cache
   - Directory change detection via mtime
   - Minimal memory footprint
   - No metadata caching

3. **MirrorFs2Iterator** (`iterator.rs`)
   - Directory listing iterator
   - Reads directory contents on-demand
   - Loads metadata on-demand for readdirplus
   - Skips metadata loading for readdir

4. **string_ext** (`string_ext.rs`)
   - OS string conversion utilities
   - Cross-platform filename handling

### Data Structures

```rust
struct FSEntry {
    path: Vec<Symbol>,                    // Interned path components
}

struct FSMap {
    root: PathBuf,                        // Root directory path
    next_fileid: AtomicU64,               // File ID generator
    intern: SymbolTable,                  // String interning
    id_to_path: HashMap<fileid3, FSEntry>, // ID -> Path mapping
    path_to_id: HashMap<Vec<Symbol>, fileid3>, // Path -> ID mapping
}
```

## Usage

The implementation can be used through the command-line interface:

```bash
# Use the original MirrorFs (default)
cargo run --bin cargo-nfs3-server -- --path /tmp

# Use MirrorFs2 (memory-optimized)
cargo run --bin cargo-nfs3-server -- --path /tmp --mirrorfs2
```

## Benefits

1. **Minimized Memory Usage**: No metadata or children caching, extremely low memory footprint
2. **Always Current Data**: All data is fresh from the filesystem
3. **Simplified Code**: No cache management complexity
4. **Excellent for Large Directories**: Memory usage doesn't scale with file count or directory depth

## Trade-offs

1. **Higher I/O**: More filesystem calls for both metadata and directory operations
2. **Potential Latency**: All requests require filesystem access
3. **No Caching Benefits**: No performance optimization for frequently accessed files or directories

## Use Cases

MirrorFs2 is ideal for:
- Large filesystems with many files
- Memory-constrained environments
- Scenarios where data freshness is critical
- Applications with infrequent metadata access patterns

## Implementation Notes

- File handles are cached only for path resolution (minimal memory usage)
- Directory contents are read from filesystem on every directory operation
- No caching of any kind except for path-to-ID mapping
- Uses efficient string interning for path components
- Cross-platform string handling for filenames
- Async I/O throughout for good performance
