//! Integration tests for cargo-nfs3-server MirrorFS implementation
//!
//! This binary starts a cargo-nfs3-server instance and executes comprehensive
//! tests for both readonly and readwrite modes.

use std::{io::Write, path::PathBuf};

use colored::Colorize;

mod context;
mod fs_util;
mod readonly;
mod readwrite;
mod server;

use context::ServerMode;
use nfs3_types::nfs3::nfs_fh3;
use server::init_context;

macro_rules! test {
    ($ctx:expr, $test_fn:path) => {{
        let test_name = stringify!($test_fn).split("::").last().unwrap();
        run_test(&mut $ctx, test_name, $test_fn).await;
    }};
}

async fn run_test<'a, F, R>(ctx: &'a mut context::TestContext, test_name: &str, test_fn: F)
where
    F: Fn(&'a mut context::TestContext, PathBuf, nfs_fh3) -> R,
    R: std::future::Future<Output = ()> + 'a,
{
    print!("🧪 {} ... ", test_name.bright_white());
    std::io::stdout().flush().unwrap();

    let subdir = ctx.create_test_subdir(test_name);
    let subdir_fh = ctx.get_subdir_fh(&subdir).await;
    test_fn(ctx, subdir, subdir_fh).await;
    println!("{}", "PASSED".green().bold());
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    println!("{}", "=".repeat(80).bright_blue());
    println!(
        "{}",
        "  MirrorFS Integration Test Suite".bright_cyan().bold()
    );
    println!("{}\n", "=".repeat(80).bright_blue());
    println!("{}\n", "📖 READONLY MODE TESTS".bright_yellow().bold());
    run_readonly_tests().await;

    // Give OS time to release resources
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    println!("\n{}\n", "✍️  READWRITE MODE TESTS".bright_yellow().bold());
    run_readwrite_tests().await;

    println!("\n{}", "=".repeat(80).bright_blue());
    println!("{}", "  All tests passed! ✅".bright_green().bold());
    println!("{}\n", "=".repeat(80).bright_blue());
}

async fn run_readonly_tests() {
    let mut ctx = init_context(ServerMode::ReadOnly)
        .await
        .expect("failed to initialize readonly context");

    println!("{}", "  Basic Operations".bright_cyan());
    test!(ctx, readonly::null);
    test!(ctx, readonly::getattr_root);
    test!(ctx, readonly::getattr_file);

    println!();
    println!("{}", "  Lookup Operations".bright_cyan());
    test!(ctx, readonly::lookup_existing_file);
    test!(ctx, readonly::lookup_non_existing_file);
    test!(ctx, readonly::lookup_in_subdirectory);

    println!();
    println!("{}", "  Access Operations".bright_cyan());
    test!(ctx, readonly::access_file);

    println!();
    println!("{}", "  Read Operations".bright_cyan());
    test!(ctx, readonly::read_file_contents);
    test!(ctx, readonly::read_large_file);
    test!(ctx, readonly::read_with_offset);
    test!(ctx, readonly::read_binary_file);

    println!();
    println!("{}", "  Directory Operations".bright_cyan());
    test!(ctx, readonly::readdir_multiple_files);
    test!(ctx, readonly::readdir_empty_directory);
    test!(ctx, readonly::readdir_many_files);
    test!(ctx, readonly::readdirplus_basic);

    println!();
    println!("{}", "  Filesystem Info".bright_cyan());
    test!(ctx, readonly::fsstat_root);
    test!(ctx, readonly::fsinfo_root);
    test!(ctx, readonly::pathconf_root);

    println!();
    println!("{}", "  Deep Navigation".bright_cyan());
    test!(ctx, readonly::deep_directory_navigation);

    println!();
    println!("{}", "  Symlink Operations".bright_cyan());
    test!(ctx, readonly::readlink_symlink);

    println!();
    println!("{}", "  Special Cases".bright_cyan());
    test!(ctx, readonly::special_characters_filename);
    test!(ctx, readonly::concurrent_reads);

    println!();
    println!("{}", "  Write Operations (ROFS Error Tests)".bright_cyan());
    test!(ctx, readonly::setattr_readonly_error);
    test!(ctx, readonly::write_readonly_error);
    test!(ctx, readonly::create_readonly_error);
    test!(ctx, readonly::mkdir_readonly_error);
    test!(ctx, readonly::symlink_readonly_error);
    test!(ctx, readonly::mknod_readonly_error);
    test!(ctx, readonly::remove_readonly_error);
    test!(ctx, readonly::rmdir_readonly_error);
    test!(ctx, readonly::rename_readonly_error);
    test!(ctx, readonly::link_readonly_error);
    test!(ctx, readonly::commit_readonly_error);
}

async fn run_readwrite_tests() {
    let mut ctx = init_context(ServerMode::ReadWrite)
        .await
        .expect("failed to initialize readwrite context");

    println!("{}", "  Write Operations".bright_cyan());
    test!(ctx, readwrite::write_to_file);
    test!(ctx, readwrite::write_with_offset);

    println!();
    println!("{}", "  Create Operations".bright_cyan());
    test!(ctx, readwrite::create_new_file);
    test!(ctx, readwrite::create_exclusive);

    println!();
    println!("{}", "  Directory Creation".bright_cyan());
    test!(ctx, readwrite::mkdir_new_directory);
    test!(ctx, readwrite::mkdir_nested);

    println!();
    println!("{}", "  Remove Operations".bright_cyan());
    test!(ctx, readwrite::remove_file);
    test!(ctx, readwrite::rmdir_directory);

    println!();
    println!("{}", "  Rename Operations".bright_cyan());
    test!(ctx, readwrite::rename_file);
    test!(ctx, readwrite::rename_directory);

    println!();
    println!("{}", "  Link Operations".bright_cyan());
    test!(ctx, readwrite::create_hard_link);
    test!(ctx, readwrite::create_symlink);

    println!();
    println!("{}", "  Setattr Operations".bright_cyan());
    test!(ctx, readwrite::setattr_file);

    println!();
    println!("{}", "  Commit Operations".bright_cyan());
    test!(ctx, readwrite::commit_writes);
}
