#![allow(clippy::unwrap_used, clippy::significant_drop_tightening)] // for the sake of the example

mod filesystem;
mod fs_map;
mod iterator;
pub mod string_ext;

pub use filesystem::MirrorFs;
