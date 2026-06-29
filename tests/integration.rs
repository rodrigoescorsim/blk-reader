use blk_reader::{BlkReader, BlkReaderConfig, BlockIterator};

fn blocks_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("BITCOIN_BLOCKS_DIR")
            .expect("Set BITCOIN_BLOCKS_DIR to your Bitcoin Core blocks/ directory"),
    )
}

/// Reads the genesis block directly and verifies size (285 bytes) and XOR decoding.
#[test]
#[ignore]
fn test_xor_decode_genesis_block() {
    let mut reader = BlkReader::new(blocks_dir(), 8 * 1024 * 1024);
    let data = reader
        .read_block_at(0, 8)
        .expect("Failed to read genesis block");
    assert_eq!(data.len(), 285, "Genesis block must be 285 bytes");
}

/// Reads block 4094 — the first block that triggered XOR magic-byte failures before the fix.
#[test]
#[ignore]
fn test_xor_decode_block_4094() {
    let mut reader = BlkReader::new(blocks_dir(), 8 * 1024 * 1024);
    let data = reader
        .read_block_at(0, 950778)
        .expect("Failed to read block 4094");
    assert!(!data.is_empty());
    assert!(data.len() > 80, "Block must have at least 80 header bytes");
}

/// Iterates the first 10 blocks and checks order, hash, and non-empty data.
#[test]
#[ignore]
fn test_read_first_10_blocks() {
    let blocks_dir = blocks_dir();
    let index_dir = blocks_dir.join("index");

    let iter = BlockIterator::new(BlkReaderConfig {
        blocks_dir: blocks_dir.clone(),
        index_dir,
        start_height: 0,
        end_height: 10,
        read_buffer_bytes: 8 * 1024 * 1024,
    })
    .expect("Failed to create BlockIterator");

    let blocks: Vec<_> = iter
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to read blocks");

    assert_eq!(blocks.len(), 10);
    assert_eq!(blocks[0].height, 0);

    let genesis_hash_be = hex::encode(blocks[0].hash.iter().rev().cloned().collect::<Vec<_>>());
    assert_eq!(
        genesis_hash_be,
        "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"
    );

    for i in 1..blocks.len() {
        assert!(blocks[i].height > blocks[i - 1].height);
    }

    for block in &blocks {
        assert!(!block.data.is_empty());
    }
}
