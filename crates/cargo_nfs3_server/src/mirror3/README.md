# Mirror3 NFS Filesystem

Mirror3 is a minimal NFS3 filesystem implementation that provides read-only access to the underlying filesystem through an NFS interface. It features advanced iterator caching with cookie validation using the moka crate.

## Features

- **Minimal Caching**: Only caches file names and paths for handle resolution
- **Fresh Metadata**: All metadata (size, timestamps, permissions) is read directly from the filesystem
- **Iterator Caching**: Directory iterators are cached using moka for performance
- **Cookie Validation**: Robust validation of NFS cookies to ensure consistency
- **Independent Iterator**: Directory traversal logic is decoupled from the main filesystem implementation
- **Read-only Access**: Implements the `NfsReadFileSystem` trait for safe read-only operations

## Design

### Iterator Cache with Moka
- **Cache Storage**: Uses moka cache to store `IteratorState` for each directory
- **TTL Policy**: Cached iterators expire after 30 seconds
- **Capacity Limit**: Maximum 1000 cached iterators to prevent memory bloat
- **Directory Validation**: Checks directory modification time to invalidate stale cache entries
- **Cookie Validation**: Verifies that NFS cookies correspond to valid entries in the cached state

### Cookie Validation Process
1. **Cache Lookup**: Check if directory iterator state is cached
2. **Freshness Check**: Validate cached state against directory modification time
3. **Cookie Validation**: Ensure the provided cookie corresponds to a valid entry
4. **Error Handling**: Return `NFS3ERR_BAD_COOKIE` for invalid cookies
5. **Position Recovery**: Resume iteration from the correct position based on cookie

### Cache Behavior
- **Path Resolution**: Maps file handles to filesystem paths for efficient lookups
- **No Metadata Caching**: File attributes, sizes, and timestamps are always read from disk
- **Iterator Caching**: Directory entry lists are cached with validation

### Iterator Independence
- **Modular Design**: Directory iteration logic is separated into its own module (`iterator.rs`)
- **Split Iterators**: Separate `Mirror3ReadDirIterator` and `Mirror3ReadDirPlusIterator` for optimal performance
- **Shared Core Logic**: Common directory entry creation logic with caching support
- **Consistent Ordering**: Directory entries are sorted by file ID for predictable iteration

### Architecture
- `mod.rs`: Main filesystem implementation with `NfsReadFileSystem` trait and moka cache integration
- `iterator.rs`: Directory iteration logic with moka-backed caching:
  - `Mirror3ReadDirIterator`: Lightweight iterator implementing `ReadDirIterator`
  - `Mirror3ReadDirPlusIterator`: Full-featured iterator implementing `ReadDirPlusIterator`
  - `IteratorState`: Cached state with validation and cookie support
- **Cache**: Minimal symbol table for path-to-handle mapping plus moka iterator cache
- **Root Path**: Base directory that the NFS filesystem mirrors

## Cookie Validation Features

### Robust Error Handling
- **Invalid Cookies**: Properly rejects cookies that don't correspond to cached entries
- **Stale Cache**: Automatically refreshes iterator state when directory changes
- **Cache Misses**: Creates fresh iterator state when no cache entry exists

### Performance Benefits
- **Reduced I/O**: Avoids re-reading directory contents for subsequent calls
- **Fast Cookie Lookup**: O(1) cache access for iterator state validation
- **Automatic Cleanup**: TTL-based expiration prevents memory leaks

## Usage

The Mirror3 filesystem can be used as a command-line argument:
```bash
cargo-nfs3-server --mirrorfs3 /path/to/directory
```

This provides a read-only NFS3 interface to the specified directory with moka-cached iterators and robust cookie validation.

## Testing

The implementation includes comprehensive tests for:
- Cookie validation with valid and invalid cookies
- Iterator caching consistency
- Directory modification detection
- Error handling for bad cookies
