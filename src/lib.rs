//! Fast reader for Bitcoin Core `blk*.dat` files.
//!
//! Reads raw block data directly from Bitcoin Core's block storage, using the
//! LevelDB index to locate blocks by height. Automatically detects and decodes
//! the XOR encoding introduced in Bitcoin Core 28.0.
//!
//! # Quick start
//!
//! ```no_run
//! use blk_reader::{BlockIterator, BlkReaderConfig};
//! use std::path::PathBuf;
//!
//! let config = BlkReaderConfig {
//!     blocks_dir: PathBuf::from("/home/user/.bitcoin/blocks"),
//!     index_dir:  PathBuf::from("/home/user/.bitcoin/blocks/index"),
//!     start_height: 0,
//!     end_height: 100,
//!     ..Default::default()
//! };
//!
//! for result in BlockIterator::new(config).unwrap() {
//!     let block = result.unwrap();
//!     println!("height={} size={} bytes", block.height, block.data.len());
//! }
//! ```
//!
//! # XOR decoding
//!
//! Bitcoin Core 28.0 introduced XOR encoding of `blk*.dat` files to prevent
//! false-positive virus scanner alerts. The key is stored in `blocks/xor.dat`.
//! This crate detects the file automatically — no configuration needed.
//!
//! # Reading a single block
//!
//! If you already know the file index and byte offset (e.g. from your own
//! LevelDB query), use [`BlkReader`] directly:
//!
//! ```no_run
//! use blk_reader::BlkReader;
//! use std::path::PathBuf;
//!
//! let mut reader = BlkReader::new(
//!     PathBuf::from("/home/user/.bitcoin/blocks"),
//!     8 * 1024 * 1024, // read buffer
//! );
//! // Genesis block: file 0, data offset 8
//! let raw = reader.read_block_at(0, 8).unwrap();
//! assert_eq!(raw.len(), 285);
//! ```

mod error;
mod index;
mod iterator;
mod reader;
mod varint;

pub use error::BlkReaderError;
pub use index::{BlockIndexEntry, IndexReader};
pub use iterator::{BlkReaderConfig, BlockIterator, RawBlock};
pub use reader::BlkReader;
