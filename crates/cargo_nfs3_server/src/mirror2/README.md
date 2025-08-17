# MirrorFs2 Implementation

## Overview

`MirrorFs2` is a new filesystem implementation for the NFS3 server that optimizes memory usage by caching only file names and paths for handle resolution, while loading all other metadata directly from the underlying filesystem on demand.

## Key Differences from MirrorFs

### Memory Usage
- **MirrorFs**: Caches complete file metadata (fattr3) for all discovered files
- **MirrorFs2**: Only caches file paths and modification times for directories

### Metadata Loading
- **MirrorFs**: Returns cached metadata, refreshes when changes detected
- **MirrorFs2**: Always loads metadata from filesystem when requested (getattr, readdirplus)

### Directory Tracking
- **MirrorFs**: Caches directory metadata and compares for changes
- **MirrorFs2**: Uses modification time comparison to detect directory changes

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
   - Loads metadata on-demand for readdirplus
   - Skips metadata loading for readdir

4. **string_ext** (`string_ext.rs`)
   - OS string conversion utilities
   - Cross-platform filename handling

### Data Structures

```rust
struct FSEntry {
    path: Vec<Symbol>,                    // Interned path components
    last_dir_mtime: Option<SystemTime>,   // Directory modification tracking
    children: Option<Vec<fileid3>>,       // Cached child file IDs
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

1. **Reduced Memory Usage**: No metadata caching reduces memory footprint significantly
2. **Always Current Data**: Metadata is always fresh from the filesystem
3. **Simplified Cache Management**: No need to track metadata staleness
4. **Better for Large Directories**: Memory usage doesn't scale with file count

## Trade-offs

1. **Higher I/O**: More filesystem calls for metadata operations
2. **Potential Latency**: Metadata requests require filesystem access
3. **Less Optimization**: No metadata pre-caching for frequently accessed files

## Use Cases

MirrorFs2 is ideal for:
- Large filesystems with many files
- Memory-constrained environments
- Scenarios where data freshness is critical
- Applications with infrequent metadata access patterns

## Implementation Notes

- File handles are still cached for path resolution
- Directory listings are cached until modification detected
- Uses efficient string interning for path components
- Cross-platform string handling for filenames
- Async I/O throughout for good performance
