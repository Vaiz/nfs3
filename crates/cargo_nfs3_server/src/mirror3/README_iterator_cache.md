# Iterator Cache Implementation

This implementation provides a custom cache for NFS directory iterators in the mirror3 module, following a pattern similar to the transaction tracker in `nfs3_server/src/transaction_tracker.rs`.

## Key Components

### 1. `IteratorCache` (`simple_iterator_cache.rs`)
The main cache structure that tracks valid iterator positions:

```rust
pub struct IteratorCache {
    retention_period: Duration,
    cache: RwLock<HashMap<IteratorKey, CachedIteratorInfo>>,
    max_cached_per_dir: u16,
}
```

**Key Methods:**
- `pop(dir_id, cookie) -> Option<Iter>` - Retrieves and removes a cached iterator position
- `cleanup()` - Removes stale entries (called periodically by a background task)
- `cache_state()` - Stores an iterator position for future reuse
- `has_cached()` - Checks if a position is cached and valid

### 2. `IteratorCacheCleaner`
A background task that periodically cleans up stale cache entries:

```rust
pub struct IteratorCacheCleaner {
    cache: Arc<IteratorCache>,
    interval: Duration,
    stop: Arc<tokio::sync::Notify>,
}
```

The cleaner runs every 30 seconds by default and removes entries older than the retention period (60 seconds).

### 3. Enhanced Iterator Structs
Both `Mirror3ReadDirIterator` and `Mirror3ReadDirPlusIterator` now include:

- Direct reference to the iterator cache via `Arc<IteratorCache>`
- `should_cache` flag to control caching behavior
- `Drop` implementation that automatically caches the current position when dropped

### 4. Integration with File System
The `Fs` struct now includes:

- An iterator cache instance shared across all operations
- Cache-aware iterator creation methods
- Background cleaner task that runs for the lifetime of the filesystem

## How It Works

### Normal Operation Flow

1. **Iterator Creation**: When `readdir` or `readdirplus` is called:
   - If `cookie == 0`: Create a new iterator from the beginning
   - If `cookie != 0`: Check if the position is cached
     - If cached: Remove from cache and create iterator at that position
     - If not cached: Create new iterator anyway (for compatibility)

2. **Iterator Usage**: As the iterator is used, it updates its internal cookie position

3. **Iterator Caching**: When an iterator is dropped (via `Drop` trait):
   - If not exhausted and `should_cache == true`: Cache the current position
   - Store the directory ID, cookie position, iterator type, and timestamp

4. **Cache Cleanup**: Background task periodically removes stale entries

### API Design

The API follows the pattern you requested:

```rust
// Equivalent to pop() - handled internally by get_or_create_*_iterator
fn has_cached(dir_id, cookie) -> bool;
fn remove_state(dir_id, cookie) -> Option<CachedIteratorInfo>;

// Cleanup - called every 30 seconds by background task
fn cleanup();
```

## Benefits

1. **Efficient Resumption**: Clients can resume directory listings without re-reading from the beginning
2. **Automatic Management**: No manual cache management required - handled by Drop trait
3. **Memory Efficiency**: Stale entries are automatically cleaned up
4. **Configurable**: Retention period and per-directory limits are configurable
5. **Compatible**: Works with existing NFS clients that expect standard cookie behavior

## Configuration

The cache is configured in `Fs::new()`:

```rust
let iterator_cache = Arc::new(IteratorCache::new(
    Duration::from_secs(60),  // Retention period
    20,                       // Max cached positions per directory
));
```

## Monitoring

Cache statistics are available via `Fs::cache_stats()`:

```rust
pub struct CacheStats {
    pub total_cached_positions: usize,
    pub cached_directories: usize,
    pub max_per_directory: usize,
}
```

## Testing

The implementation includes comprehensive tests and maintains compatibility with existing NFS behavior. All original tests pass, ensuring no regression in functionality.

## Future Enhancements

Potential improvements could include:

1. **Smarter Seeking**: Instead of always creating new iterators, implement seeking within existing iterators
2. **Persistence**: Cache could survive process restarts by persisting to disk
3. **Metrics**: More detailed cache hit/miss statistics
4. **Adaptive Limits**: Dynamic cache size based on usage patterns

## Example Usage

See `cache_demo.rs` for a demonstration of how the cache works in practice.
