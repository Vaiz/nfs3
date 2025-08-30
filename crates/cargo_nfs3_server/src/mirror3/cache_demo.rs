// Example demonstrating the iterator cache behavior in mirror3
use nfs3_server::vfs::{ReadDirIterator, NextResult, NfsReadFileSystem};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

/// This example demonstrates how the iterator cache works.
/// 
/// The cache automatically stores iterator positions when iterators are dropped
/// before completion, allowing efficient resumption of directory listings.
pub async fn demonstrate_iterator_cache<P: AsRef<Path>>(dir_path: P) {
    let fs = super::Fs::new(dir_path);
    let root_handle = fs.root_dir();
    
    println!("=== Iterator Cache Demonstration ===");
    
    // Start listing the directory
    let mut iter = fs.readdir(&root_handle, 0).await.unwrap();
    
    // Read a few entries
    let mut entries = Vec::new();
    for _ in 0..3 {
        match iter.next().await {
            NextResult::Ok(entry) => {
                println!("Read entry: {}", std::format!("{:?}", entry.name));
                entries.push(entry);
            }
            NextResult::Eof => {
                println!("Reached end of directory early");
                break;
            }
            NextResult::Err(e) => {
                println!("Error reading directory: {:?}", e);
                return;
            }
        }
    }
    
    if let Some(last_entry) = entries.last() {
        let resume_cookie = last_entry.cookie;
        println!("Dropping iterator at cookie: {}", resume_cookie);
        
        // Drop the iterator - this should cache the current position
        drop(iter);
        
        // Check cache stats
        let stats = fs.cache_stats();
        println!("Cache stats after drop: {:?}", stats);
        
        // Small delay to simulate real-world usage
        sleep(Duration::from_millis(10)).await;
        
        // Try to resume from the cached position
        println!("Attempting to resume from cookie: {}", resume_cookie);
        match fs.readdir(&root_handle, resume_cookie).await {
            Ok(mut resumed_iter) => {
                println!("Successfully resumed iteration!");
                
                // Read a few more entries to show it works
                for _ in 0..2 {
                    match resumed_iter.next().await {
                        NextResult::Ok(entry) => {
                            println!("Resumed entry: {}", std::format!("{:?}", entry.name));
                        }
                        NextResult::Eof => {
                            println!("Reached end of directory");
                            break;
                        }
                        NextResult::Err(e) => {
                            println!("Error reading directory: {:?}", e);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                println!("Failed to resume: {:?}", e);
            }
        }
        
        // Final cache stats
        let final_stats = fs.cache_stats();
        println!("Final cache stats: {:?}", final_stats);
    }
}
