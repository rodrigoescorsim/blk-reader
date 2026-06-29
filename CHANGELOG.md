# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-06-29

### Added
- `BlkReader`: low-level reader for `blk*.dat` files with LRU file handle pool (max 8 open files)
- `BlockIterator`: height-ordered iterator backed by the LevelDB block index
- `BlkReaderConfig`: configuration struct with sensible defaults
- `RawBlock`: return type carrying height, hash, and raw block bytes
- `IndexReader`: reads Bitcoin Core's LevelDB block index; handles locked databases on Windows via shadow copy and synthetic MANIFEST generation
- Automatic XOR decoding for Bitcoin Core 28.0+ (`blocks/xor.dat`)
- Integration tests (require a Bitcoin Core data directory, skipped by default)
- Examples: `read_genesis`, `iterate_blocks`
