use blk_reader::{BlkReaderConfig, BlockIterator, BlkReader};

/// Tests direct reading of blk00000.dat with XOR decoding (Bitcoin Core 28.0+).
/// Validates: correct magic bytes after decode, reasonable genesis block size (285 bytes).
#[test]
#[ignore]
fn test_xor_decode_genesis_block() {
    let blocks_dir = std::path::PathBuf::from("D:/Bitcoin/blocks");
    let mut reader = BlkReader::new(blocks_dir, 8 * 1024 * 1024);
    // Genesis block: n_file=0, n_data_pos=8 (magic at offset 0, size at offset 4)
    let data = reader.read_block_at(0, 8).expect("Failed to read genesis block");
    assert_eq!(data.len(), 285, "Genesis block must be 285 bytes");
}

/// Tests block reading at height 4094 (the first one that failed with invalid magic bytes).
#[test]
#[ignore]
fn test_xor_decode_block_4094() {
    let blocks_dir = std::path::PathBuf::from("D:/Bitcoin/blocks");
    let mut reader = BlkReader::new(blocks_dir, 8 * 1024 * 1024);
    // n_data_pos=950778 for height=4094 according to LevelDB index
    let data = reader.read_block_at(0, 950778).expect("Failed to read block 4094");
    assert!(!data.is_empty(), "Block 4094 must have data");
    // 2009 block has at least 80 header bytes + 1 coinbase tx
    assert!(data.len() > 80, "Block 4094 must have more than 80 bytes");
}

#[test]
#[ignore]
fn test_read_first_10_blocks() {
    let blocks_dir = std::path::PathBuf::from(
        std::env::var("BITCOIN_BLOCKS_DIR")
            .expect("BITCOIN_BLOCKS_DIR must point to ~/.bitcoin/blocks/"),
    );
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
