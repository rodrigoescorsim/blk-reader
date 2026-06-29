# blk-reader

[![Crates.io](https://img.shields.io/crates/v/blk-reader)](https://crates.io/crates/blk-reader)
[![Docs.rs](https://docs.rs/blk-reader/badge.svg)](https://docs.rs/blk-reader)
[![CI](https://github.com/rodrigoescorsim/blk-reader/actions/workflows/ci.yml/badge.svg)](https://github.com/rodrigoescorsim/blk-reader/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Fast reader for Bitcoin Core `blk*.dat` files, written in Rust.

Uses the LevelDB block index to locate blocks by height and reads raw block bytes directly from disk. Automatically detects and decodes the XOR encoding introduced in **Bitcoin Core 28.0**.

## Features

- Iterate blocks by height range via `BlockIterator`
- Read individual blocks by file index and offset via `BlkReader`
- Automatic XOR decoding — detects `blocks/xor.dat` with no configuration
- Handles locked LevelDB index (Bitcoin Core running) via shadow copy on Windows
- LRU file handle cache (max 8 open files) for sequential and random access
- No async runtime required

## Installation

```toml
[dependencies]
blk-reader = "0.1"
```

## Usage

### Iterate blocks

```rust
use blk_reader::{BlockIterator, BlkReaderConfig};
use std::path::PathBuf;

let config = BlkReaderConfig {
    blocks_dir: PathBuf::from("/home/user/.bitcoin/blocks"),
    index_dir:  PathBuf::from("/home/user/.bitcoin/blocks/index"),
    start_height: 0,
    end_height: 840_000,
    ..Default::default()
};

for result in BlockIterator::new(config)? {
    let block = result?;
    println!("height={} size={} bytes", block.height, block.data.len());
}
```

### Read a single block

If you already know the file index and byte offset:

```rust
use blk_reader::BlkReader;
use std::path::PathBuf;

let mut reader = BlkReader::new(
    PathBuf::from("/home/user/.bitcoin/blocks"),
    8 * 1024 * 1024,
);

// Genesis block: file 0, data offset 8
let raw = reader.read_block_at(0, 8)?;
assert_eq!(raw.len(), 285);
```

### Parse the block data

`blk-reader` returns raw bytes — parsing is left to the caller. Pair it with
[`bitcoin`](https://crates.io/crates/bitcoin) or
[`bitcoin-consensus`](https://crates.io/crates/bitcoin-consensus):

```rust
use bitcoin::consensus::deserialize;
use bitcoin::Block;

let block: Block = deserialize(&raw_bytes)?;
println!("txs={}", block.txdata.len());
```

## Requirements

- A running or stopped Bitcoin Core node with the `blocks/` directory intact
- The `blocks/index/` LevelDB directory (created by Bitcoin Core automatically)
- Rust 1.75+

## XOR Decoding (Bitcoin Core 28.0+)

Bitcoin Core 28.0 introduced XOR encoding of `blk*.dat` files using an 8-byte
key stored in `blocks/xor.dat`. This was added to prevent false-positive
antivirus alerts on block data that resembles malware signatures.

This crate detects `xor.dat` automatically at startup. Older nodes without
the file are handled transparently (no XOR applied).

## Examples

```sh
# Read and print the genesis block
BITCOIN_BLOCKS_DIR=~/.bitcoin/blocks cargo run --example read_genesis

# Iterate the first 1000 blocks
BITCOIN_BLOCKS_DIR=~/.bitcoin/blocks cargo run --example iterate_blocks
```

## Integration tests

Integration tests require a Bitcoin Core data directory and are skipped by default:

```sh
BITCOIN_BLOCKS_DIR=~/.bitcoin/blocks cargo test -- --include-ignored
```

## License

MIT — see [LICENSE](LICENSE).

## Author

[Rodrigo Escorsim](https://github.com/rodrigoescorsim) · [cachesnap.com](https://cachesnap.com)
