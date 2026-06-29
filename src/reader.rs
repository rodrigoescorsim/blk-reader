use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;

use crate::error::BlkReaderError;

/// Low-level reader for Bitcoin Core `blk*.dat` files.
///
/// Manages a pool of up to 8 open file handles (LRU eviction) and applies
/// XOR decoding automatically when `blocks/xor.dat` is present.
///
/// For height-ordered iteration prefer [`BlockIterator`](crate::BlockIterator).
pub struct BlkReader {
    blocks_dir: PathBuf,
    buffer_size: usize,
    open_files: HashMap<u32, BufReader<File>>,
    access_order: Vec<u32>,
    /// XOR key for blk*.dat decoding (Bitcoin Core 28.0+).
    /// Read from `blocks/xor.dat`. Empty = no XOR (Bitcoin Core < 28.0).
    xor_key: [u8; 8],
    xor_key_len: usize,
}

const MAX_OPEN_FILES: usize = 8;
const MAINNET_MAGIC: u32 = 0xD9B4BEF9;

impl BlkReader {
    /// Creates a new reader for the given `blocks/` directory.
    ///
    /// `buffer_size` is the I/O read buffer per open file in bytes.
    /// A value of `8 * 1024 * 1024` (8 MiB) works well for sequential reads.
    /// XOR key detection from `xor.dat` happens here at construction time.
    pub fn new(blocks_dir: PathBuf, buffer_size: usize) -> Self {
        // Auto-detect XOR key from blocks/xor.dat (Bitcoin Core 28.0+).
        // Older nodes do not have this file — in this case xor_key_len=0 (no XOR).
        let (xor_key, xor_key_len) = Self::load_xor_key(&blocks_dir);
        if xor_key_len > 0 {
            tracing::info!(
                key_hex = %hex::encode(&xor_key[..xor_key_len]),
                "BlkReader: xor.dat detected — Bitcoin Core 28.0+ XOR encoding active"
            );
        }
        Self {
            blocks_dir,
            buffer_size,
            open_files: HashMap::new(),
            access_order: Vec::new(),
            xor_key,
            xor_key_len,
        }
    }

    /// Reads the XOR key from `blocks/xor.dat`. Returns key and length.
    /// Bitcoin Core 28.0+ creates this file with 8 random bytes.
    fn load_xor_key(blocks_dir: &std::path::Path) -> ([u8; 8], usize) {
        let xor_path = blocks_dir.join("xor.dat");
        match std::fs::read(&xor_path) {
            Ok(data) if !data.is_empty() => {
                let len = data.len().min(8);
                let mut key = [0u8; 8];
                key[..len].copy_from_slice(&data[..len]);
                (key, len)
            }
            _ => ([0u8; 8], 0),
        }
    }

    /// Reads the raw serialized block at a given file index and byte offset.
    ///
    /// `n_file` corresponds to `blk{n_file:05}.dat` (e.g. `0` → `blk00000.dat`).
    /// `n_data_pos` is the offset of the block data as stored in the LevelDB index
    /// (`CDiskBlockIndex::nDataPos`). The 8 bytes immediately before it contain the
    /// network magic and block size.
    ///
    /// Returns the raw block bytes without any header prefix. Pass the result to
    /// `bitcoin::consensus::deserialize` or similar to decode transactions.
    ///
    /// # Errors
    ///
    /// Returns [`BlkReaderError`] if the file is missing, the magic bytes don't
    /// match mainnet, or an I/O error occurs.
    pub fn read_block_at(&mut self, n_file: u32, n_data_pos: u32) -> Result<Vec<u8>, BlkReaderError> {
        // CDiskBlockIndex nDataPos = offset of the start of serialized block data.
        // The 8 bytes BEFORE contain: [magic: 4 bytes LE][block_size: 4 bytes LE].
        // Bitcoin Core 28.0+ XOR-encodes the entire file — we decode here.
        if n_data_pos < 8 {
            return Err(BlkReaderError::InvalidMagicBytes {
                file: n_file,
                offset: n_data_pos as u64,
                got: 0,
            });
        }
        let header_pos = (n_data_pos - 8) as u64;

        // Copy the XOR key to local variables BEFORE getting the reader
        // (prevents borrow conflict: reader &mut borrow vs xor_key & borrow)
        let xor_key = self.xor_key;
        let xor_key_len = self.xor_key_len;

        let reader = self.get_or_open(n_file)?;
        reader.seek(SeekFrom::Start(header_pos))?;

        let mut magic_buf = [0u8; 4];
        reader.read_exact(&mut magic_buf)?;
        // Decode XOR using local copy of the key
        if xor_key_len > 0 {
            for (i, byte) in magic_buf.iter_mut().enumerate() {
                *byte ^= xor_key[(header_pos as usize + i) % xor_key_len];
            }
        }
        let magic = u32::from_le_bytes(magic_buf);

        if magic != MAINNET_MAGIC {
            // Diagnostic: read 32 bytes at data offset to facilitate debugging
            let mut diag_buf = [0u8; 32];
            reader.seek(SeekFrom::Start(n_data_pos as u64))?;
            let _ = reader.read_exact(&mut diag_buf);
            if xor_key_len > 0 {
                for (i, byte) in diag_buf.iter_mut().enumerate() {
                    *byte ^= xor_key[(n_data_pos as usize + i) % xor_key_len];
                }
            }
            tracing::error!(
                file = n_file,
                offset = n_data_pos,
                hex = %hex::encode(diag_buf),
                "ERROR: Magic bytes not found. Content at original offset above."
            );
            return Err(BlkReaderError::InvalidMagicBytes {
                file: n_file,
                offset: n_data_pos as u64,
                got: magic,
            });
        }

        // Read and decode the block size (4 bytes after the magic)
        let mut size_buf = [0u8; 4];
        reader.read_exact(&mut size_buf)?;
        if xor_key_len > 0 {
            let size_pos = header_pos as usize + 4;
            for (i, byte) in size_buf.iter_mut().enumerate() {
                *byte ^= xor_key[(size_pos + i) % xor_key_len];
            }
        }
        let block_size = u32::from_le_bytes(size_buf) as usize;

        // Read and decode the block data
        let mut block_data = vec![0u8; block_size];
        reader.read_exact(&mut block_data)?;
        if xor_key_len > 0 {
            let data_pos = n_data_pos as usize;
            for (i, byte) in block_data.iter_mut().enumerate() {
                *byte ^= xor_key[(data_pos + i) % xor_key_len];
            }
        }

        if let Some(pos) = self.access_order.iter().position(|&f| f == n_file) {
            self.access_order.remove(pos);
        }
        self.access_order.push(n_file);

        Ok(block_data)
    }

    fn get_or_open(&mut self, n_file: u32) -> Result<&mut BufReader<File>, BlkReaderError> {
        if !self.open_files.contains_key(&n_file) {
            if self.open_files.len() >= MAX_OPEN_FILES {
                if let Some(oldest) = self.access_order.first().copied() {
                    self.open_files.remove(&oldest);
                    self.access_order.retain(|&f| f != oldest);
                }
            }

            let path = self.blocks_dir.join(format!("blk{n_file:05}.dat"));
            if !path.exists() {
                return Err(BlkReaderError::BlkFileNotFound { index: n_file });
            }

            let file = File::open(&path)?;
            let reader = BufReader::with_capacity(self.buffer_size, file);
            self.open_files.insert(n_file, reader);
            self.access_order.push(n_file);
        }

        Ok(self.open_files.get_mut(&n_file).unwrap())
    }
}
