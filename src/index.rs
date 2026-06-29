use crate::error::BlkReaderError;
use crate::varint::read_varint;
use std::path::Path;

/// Represents a CDiskBlockIndex entry from Bitcoin Core.
#[derive(Debug, Clone)]
pub struct BlockIndexEntry {
    /// Block hash (32 bytes, Bitcoin Core internal format — little-endian)
    pub hash: [u8; 32],
    /// Block height in the chain
    pub height: u32,
    /// Bitcoin Core status field:
    /// - bits 0–2: validity level (0=unknown, 1=header, 2=tree, 3=transactions,
    ///   4=chain/BLOCK_VALID_CHAIN, 5=scripts/BLOCK_VALID_SCRIPTS)
    /// - bit 3 (0x08): BLOCK_HAVE_DATA — block data is in blk*.dat
    /// - bit 4 (0x10): BLOCK_HAVE_UNDO — undo data is in rev*.dat
    pub status: u32,
    /// blk*.dat file index (0 → blk00000.dat)
    pub n_file: u32,
    /// Byte offset within the blk{n_file:05}.dat file where the block starts
    pub n_data_pos: u32,
}

impl BlockIndexEntry {
    /// Returns true if the block is validated in the chain and the data is available on disk.
    ///
    /// Requires:
    /// - Validity level ≥ 4 (BLOCK_VALID_CHAIN): bits 0–2 ≥ 4
    /// - BLOCK_HAVE_DATA (bit 3): data present in blk*.dat
    ///
    /// Note: using `status & 0x04` would be incorrect — it would only check bit 2, which is
    /// true for any level ≥ 4 (binary ...1xx), but also for level 6+ or
    /// invalid combinations. The correct way isolates the 3 validity bits with `& 0x07`.
    pub fn is_valid_and_available(&self) -> bool {
        (self.status & 0x07) >= 4 && (self.status & 0x08 != 0)
    }

    /// Returns the block's validity level (bits 0–2 of status).
    pub fn validity_level(&self) -> u32 {
        self.status & 0x07
    }
}

pub struct IndexReader;

impl IndexReader {
    /// Reads all LevelDB index entries from Bitcoin Core in `index_dir`.
    ///
    /// If opening fails due to a lock (common if Bitcoin Core is running),
    /// attempts to create a temporary copy of index files for reading.
    pub fn read_all(index_dir: &Path) -> Result<Vec<BlockIndexEntry>, BlkReaderError> {
        use rusty_leveldb::{Options, DB};

        let path_str = index_dir
            .to_str()
            .ok_or_else(|| BlkReaderError::LevelDbOpen {
                path: index_dir.to_path_buf(),
                reason: "Path is not valid UTF-8".to_string(),
            })?;

        let options = Options {
            create_if_missing: false,
            ..Options::default()
        };

        // Try to open the original database
        let db_result = DB::open(path_str, options.clone());

        let db = match db_result {
            Ok(db) => db,
            Err(e) => {
                let err_msg = e.to_string();
                // On Windows, the lock error is typically "process cannot access the file"
                if err_msg.contains("process cannot access") || err_msg.contains("lock") {
                    tracing::info!(
                        path = %index_dir.display(),
                        "LevelDB index locked by Bitcoin Core. Attempting reading via temporary Shadow Copy..."
                    );

                    // Use LOCALAPPDATA as base for the temporary directory.
                    // std::env::temp_dir() might resolve to virtual paths (e.g. MobaXterm /tmp)
                    // which are not real Windows paths. LOCALAPPDATA is always a native path.
                    let base_tmp = std::env::var("LOCALAPPDATA")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| std::env::temp_dir());
                    let temp_dir = base_tmp
                        .join("Temp")
                        .join(format!("semantiq_index_copy_{}", uuid::Uuid::new_v4()));
                    std::fs::create_dir_all(&temp_dir).map_err(|io_err| {
                        BlkReaderError::LevelDbOpen {
                            path: temp_dir.clone(),
                            reason: format!("Failed to create temporary directory: {io_err}"),
                        }
                    })?;

                    // Copy relevant files (ldb, log, CURRENT, MANIFEST)
                    // Note: We do not copy the LOCK file
                    //
                    // MANIFEST strategy: Bitcoin Core keeps the MANIFEST with an exclusive lock
                    // permanently (not only during compaction). If duplication fails, we generate
                    // a synthetic MANIFEST from the already copied .ldb — LevelDB level-0 includes
                    // all files in the iterator regardless of key ranges, thus full iteration
                    // works correctly with a synthetic MANIFEST.
                    let mut manifest_needs_synthesis = false;

                    if let Ok(entries) = std::fs::read_dir(index_dir) {
                        for entry in entries.flatten() {
                            let file_type = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
                            let file_name = entry.file_name();
                            let file_name_str = file_name.to_string_lossy().to_string();

                            if file_type && !file_name_str.contains("LOCK") {
                                let dest_path = temp_dir.join(&file_name);
                                let copy_res = Self::copy_file_shared(&entry.path(), &dest_path);

                                match copy_res {
                                    Ok(_) => {
                                        tracing::debug!(file = %file_name_str, "Index file copied successfully");
                                    }
                                    Err(copy_err) if file_name_str.starts_with("MANIFEST") => {
                                        // MANIFEST permanently locked — will be synthesized after the loop
                                        tracing::info!(
                                            file = %file_name_str,
                                            error = %copy_err,
                                            "MANIFEST locked by Bitcoin Core — synthetic MANIFEST will be generated"
                                        );
                                        manifest_needs_synthesis = true;
                                    }
                                    Err(copy_err) if file_name_str == "CURRENT" => {
                                        // CURRENT can also be synthesized along with the MANIFEST
                                        tracing::debug!(file = %file_name_str, error = %copy_err, "CURRENT locked — will be generated along with synthetic MANIFEST");
                                        manifest_needs_synthesis = true;
                                    }
                                    Err(copy_err) => {
                                        tracing::debug!(file = %file_name_str, error = %copy_err, "Ignoring non-critical locked index file");
                                    }
                                }
                            }
                        }
                    }

                    // Synthesize MANIFEST + CURRENT if necessary
                    if manifest_needs_synthesis {
                        if let Err(e) = Self::write_synthetic_manifest(&temp_dir) {
                            let _ = std::fs::remove_dir_all(&temp_dir);
                            return Err(BlkReaderError::LevelDbOpen {
                                path: temp_dir.clone(),
                                reason: format!("Failed to generate synthetic MANIFEST: {e}"),
                            });
                        }
                        tracing::info!("Shadow copy: synthetic MANIFEST generated successfully");
                    }

                    let temp_path_str = temp_dir.to_str().unwrap();
                    let temp_db = DB::open(temp_path_str, options).map_err(|e2| {
                        BlkReaderError::LevelDbOpen {
                            path: temp_dir.clone(),
                            reason: format!("Failed to open temporary LevelDB copy: {e2}"),
                        }
                    })?;

                    // Schedule cleanup of the temporary directory after reading
                    // Since we don't have a guaranteed destructor here, the ideal is to read and then delete.
                    // We will read everything and clean up at the end of this function.
                    let result = Self::read_from_db(temp_db);

                    // Cleanup
                    let _ = std::fs::remove_dir_all(&temp_dir);

                    return result;
                } else {
                    return Err(BlkReaderError::LevelDbOpen {
                        path: index_dir.to_path_buf(),
                        reason: err_msg,
                    });
                }
            }
        };

        Self::read_from_db(db)
    }

    /// Copies a file using shared access mode on Windows.
    ///
    /// On Windows, Bitcoin Core keeps files like MANIFEST and .log open with
    /// an exclusive lock. Standard Rust `std::fs::File::open()` uses `FILE_SHARE_READ`
    /// but not `FILE_SHARE_WRITE`, causing the open to fail with `os error 32`.
    ///
    /// This function uses `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` (0x07)
    /// via `OpenOptionsExt::share_mode()`, allowing reading the file even with another
    /// process keeping it open for writing.
    fn copy_file_shared(
        src_path: &std::path::Path,
        dst_path: &std::path::Path,
    ) -> std::io::Result<u64> {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_SHARE_READ (0x01) | FILE_SHARE_WRITE (0x02) | FILE_SHARE_DELETE (0x04)
            let mut src = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0x01 | 0x02 | 0x04)
                .open(src_path)?;
            let mut dst = std::fs::File::create(dst_path)?;
            std::io::copy(&mut src, &mut dst)
        }

        #[cfg(not(target_os = "windows"))]
        {
            let mut src = std::fs::File::open(src_path)?;
            let mut dst = std::fs::File::create(dst_path)?;
            std::io::copy(&mut src, &mut dst)
        }
    }

    fn read_from_db(mut db: rusty_leveldb::DB) -> Result<Vec<BlockIndexEntry>, BlkReaderError> {
        use rusty_leveldb::LdbIterator;
        let mut entries = Vec::new();
        let mut iter = db.new_iter().map_err(|e| BlkReaderError::IndexParseError {
            reason: format!("Failed to create iterator: {e}"),
        })?;

        // Ensure we are at the absolute beginning of the database
        iter.seek_to_first();

        let mut total_keys: u64 = 0;
        while iter.valid() {
            let mut key = Vec::new();
            let mut value = Vec::new();
            if !iter.current(&mut key, &mut value) {
                break;
            }
            total_keys += 1;

            // Diagnostic log of the first key read (any type) to confirm reading
            if total_keys == 1 {
                tracing::debug!(
                    key_len = key.len(),
                    first_byte = key.first().copied().unwrap_or(0),
                    "First key read from LevelDB index"
                );
            }

            // CDiskBlockIndex entries have key: [0x62, <32 bytes of hash>]
            if key.len() >= 33 && key[0] == b'b' {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&key[1..33]);

                match Self::parse_value(&value, hash) {
                    Ok(entry) => {
                        if entries.is_empty() {
                            tracing::info!(
                                height = entry.height,
                                file = entry.n_file,
                                pos = entry.n_data_pos,
                                "First index entry decoded"
                            );
                        }
                        entries.push(entry);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Ignoring invalid index entry");
                    }
                }
            }
            iter.advance();
        }

        tracing::info!(
            total_keys,
            block_entries = entries.len(),
            "LevelDB index entries read"
        );
        Ok(entries)
    }

    fn parse_value(data: &[u8], hash: [u8; 32]) -> Result<BlockIndexEntry, BlkReaderError> {
        let mut pos = 0usize;

        // nVersion
        let _version = read_varint(data, &mut pos)?;

        // nHeight
        let height = read_varint(data, &mut pos)? as u32;

        // nStatus
        let status = read_varint(data, &mut pos)? as u32;

        // nTx
        let _n_tx = read_varint(data, &mut pos)?;

        let mut n_file = 0u32;
        let mut n_data_pos = 0u32;

        // Bitcoin Core rules for CDiskBlockIndex deserialization:
        // 1. nFile exists if status has BLOCK_HAVE_DATA (0x08) or BLOCK_HAVE_UNDO (0x10)
        if status & (0x08 | 0x10) != 0 {
            n_file = read_varint(data, &mut pos)? as u32;
        }

        // 2. nDataPos exists ONLY if status has BLOCK_HAVE_DATA (0x08)
        if status & 0x08 != 0 {
            n_data_pos = read_varint(data, &mut pos)? as u32;
        }

        // 3. nUndoPos exists ONLY if status has BLOCK_HAVE_UNDO (0x10)
        if status & 0x10 != 0 {
            let _n_undo_pos = read_varint(data, &mut pos)?;
        }

        Ok(BlockIndexEntry {
            hash,
            height,
            status,
            n_file,
            n_data_pos,
        })
    }

    /// Generates a synthetic MANIFEST in `temp_dir` referencing all present `.ldb` files.
    ///
    /// Used when the original MANIFEST is exclusively locked by Bitcoin Core and cannot
    /// be copied. All `.ldb` files are declared at level 0 of the LevelDB — at level 0,
    /// the LevelDB iterator includes ALL files unconditionally (level-0 files
    /// can have overlapping key ranges, so all are queried), allowing
    /// full iteration of the index without relying on real MANIFEST key ranges.
    ///
    /// Generated format:
    /// - `MANIFEST-000001`: a VersionEdit in LevelDB log record with masked CRC32C
    /// - `CURRENT`: points to `MANIFEST-000001`
    fn write_synthetic_manifest(temp_dir: &std::path::Path) -> std::io::Result<()> {
        use crc::{Crc, CRC_32_ISCSI};
        use std::io::Write;

        // Collect .ldb files present in the temporary directory
        let mut ldb_files: Vec<(u64, u64)> = Vec::new(); // (file_number, file_size)
        for entry in std::fs::read_dir(temp_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(num_str) = name.strip_suffix(".ldb") {
                if let Ok(num) = num_str.parse::<u64>() {
                    let size = entry.metadata()?.len();
                    ldb_files.push((num, size));
                }
            }
        }
        ldb_files.sort_by_key(|(n, _)| *n);

        let max_file_num = ldb_files.iter().map(|(n, _)| *n).max().unwrap_or(1);

        // Encode VersionEdit (LevelDB format — NOT protobuf; tags are simple varints)
        // Reference: leveldb/db/version_edit.cc::EncodeTo()
        let mut edit: Vec<u8> = Vec::new();

        // kComparator = 1: varint(1) + varint(len) + bytes
        Self::ldb_put_varint32(&mut edit, 1);
        let comparator = b"leveldb.BytewiseComparator";
        Self::ldb_put_varint32(&mut edit, comparator.len() as u32);
        edit.extend_from_slice(comparator);

        // kLogNumber = 2: varint(2) + varint64(0)
        Self::ldb_put_varint32(&mut edit, 2);
        Self::ldb_put_varint64(&mut edit, 0);

        // kNextFileNumber = 3: varint(3) + varint64(max+10)
        Self::ldb_put_varint32(&mut edit, 3);
        Self::ldb_put_varint64(&mut edit, max_file_num + 10);

        // kLastSequence = 4: must be larger than any real seq in Bitcoin Core .ldb
        // (typically ~millions for block index) so all entries are visible
        // to the iterator (which shows entries with seq <= snapshot = last_seq), but MUST be smaller than
        // MAX_SEQUENCE_NUMBER = (1<<56)-1 to avoid the compaction bug.
        //
        // Bug: with last_seq = MAX_SEQUENCE_NUMBER, LevelDB computes smallest_snap = MAX_SEQUENCE_NUMBER
        // and compaction logic does: `if MAX_SEQUENCE_NUMBER <= MAX_SEQUENCE_NUMBER → skip ALL entries`,
        // resulting in zero outputs and exclusion of all input files — empty database.
        //
        // Using (1<<55): is ~36 quadrillion — greater than any Bitcoin Core seq,
        // and smaller than MAX_SEQUENCE_NUMBER = (1<<56)-1 — compaction preserves entries.
        Self::ldb_put_varint32(&mut edit, 4);
        Self::ldb_put_varint64(&mut edit, 1u64 << 55);

        // kNewFile = 7 for each .ldb file — all at level 0
        for (file_num, file_size) in &ldb_files {
            Self::ldb_put_varint32(&mut edit, 7);
            Self::ldb_put_varint32(&mut edit, 0); // level 0
            Self::ldb_put_varint64(&mut edit, *file_num);
            Self::ldb_put_varint64(&mut edit, *file_size);

            // InternalKey::Encode() = user_key + PackSequenceAndType(seq, type)
            // PackSequenceAndType = (seq << 8) | type, stored as little-endian u64
            //
            // smallest: user_key = [0x00], seq=0, type=kTypeValue(1) → packed = 1_u64
            let mut smallest = vec![0x00u8];
            smallest.extend_from_slice(&1u64.to_le_bytes());
            Self::ldb_put_varint32(&mut edit, smallest.len() as u32);
            edit.extend_from_slice(&smallest);

            // largest: user_key = [0xff; 200], seq=0, type=kTypeDeletion(0) → packed = 0_u64
            // Synthetic key ranges: at level 0 the iterator includes all files
            // regardless of these values, therefore any range covers all blocks.
            let mut largest = vec![0xffu8; 200];
            largest.extend_from_slice(&0u64.to_le_bytes());
            Self::ldb_put_varint32(&mut edit, largest.len() as u32);
            edit.extend_from_slice(&largest);
        }

        // Write as LevelDB log record (kFullType = 1)
        // Header: masked_CRC32C(4, LE) + Length(2, LE) + Type(1)
        // CRC covers: [type_byte] + data
        let record_type: u8 = 1; // kFullType
        let crc32c_algo = Crc::<u32>::new(&CRC_32_ISCSI);
        let mut digest = crc32c_algo.digest();
        digest.update(&[record_type]);
        digest.update(&edit);
        let raw_crc = digest.finalize();
        // LevelDB masking: rotate right 15 bits + add constant
        let masked_crc = raw_crc.rotate_right(15).wrapping_add(0xa282ead8u32);

        let manifest_path = temp_dir.join("MANIFEST-000001");
        let mut f = std::fs::File::create(&manifest_path)?;
        f.write_all(&masked_crc.to_le_bytes())?;
        f.write_all(&(edit.len() as u16).to_le_bytes())?;
        f.write_all(&[record_type])?;
        f.write_all(&edit)?;
        drop(f);

        // CURRENT points to synthetic MANIFEST
        std::fs::write(temp_dir.join("CURRENT"), "MANIFEST-000001\n")?;

        tracing::info!(
            ldb_files = ldb_files.len(),
            manifest = "MANIFEST-000001",
            "Synthetic MANIFEST created with {} .ldb files at level 0",
            ldb_files.len()
        );

        Ok(())
    }

    #[inline]
    fn ldb_put_varint32(buf: &mut Vec<u8>, mut v: u32) {
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                buf.push(byte);
                break;
            } else {
                buf.push(byte | 0x80);
            }
        }
    }

    #[inline]
    fn ldb_put_varint64(buf: &mut Vec<u8>, mut v: u64) {
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                buf.push(byte);
                break;
            } else {
                buf.push(byte | 0x80);
            }
        }
    }
}
