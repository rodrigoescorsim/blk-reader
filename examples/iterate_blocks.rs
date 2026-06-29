//! Iterates the first 1000 blocks and prints height + size.
//!
//! Run with:
//!   BITCOIN_BLOCKS_DIR=/home/user/.bitcoin/blocks cargo run --example iterate_blocks

use blk_reader::{BlockIterator, BlkReaderConfig};
use std::path::PathBuf;

fn main() {
    let blocks_dir = PathBuf::from(
        std::env::var("BITCOIN_BLOCKS_DIR")
            .expect("Set BITCOIN_BLOCKS_DIR to your Bitcoin Core blocks/ directory"),
    );

    let config = BlkReaderConfig {
        index_dir: blocks_dir.join("index"),
        blocks_dir,
        start_height: 0,
        end_height: 1000,
        ..Default::default()
    };

    let iter = BlockIterator::new(config).expect("Failed to open block index");

    let mut count = 0usize;
    let mut total_bytes = 0usize;

    for result in iter {
        match result {
            Ok(block) => {
                total_bytes += block.data.len();
                count += 1;
                if count % 100 == 0 {
                    println!("height={} size={} bytes", block.height, block.data.len());
                }
            }
            Err(e) => eprintln!("Error at block {count}: {e}"),
        }
    }

    println!("\nRead {count} blocks, {total_bytes} bytes total");
}
