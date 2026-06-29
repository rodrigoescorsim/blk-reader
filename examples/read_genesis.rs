//! Reads the genesis block directly from blk00000.dat.
//!
//! Run with:
//!   BITCOIN_BLOCKS_DIR=/home/user/.bitcoin/blocks cargo run --example read_genesis

use blk_reader::BlkReader;
use std::path::PathBuf;

fn main() {
    let blocks_dir = PathBuf::from(
        std::env::var("BITCOIN_BLOCKS_DIR")
            .expect("Set BITCOIN_BLOCKS_DIR to your Bitcoin Core blocks/ directory"),
    );

    let mut reader = BlkReader::new(blocks_dir, 8 * 1024 * 1024);

    // Genesis block is always at file 0, data offset 8
    match reader.read_block_at(0, 8) {
        Ok(data) => {
            println!("Genesis block: {} bytes", data.len());
            println!("First 80 bytes (header): {}", hex::encode(&data[..80.min(data.len())]));
        }
        Err(e) => eprintln!("Error: {e}"),
    }
}
