use std::path::PathBuf;

use crate::error::BlkReaderError;
use crate::index::{BlockIndexEntry, IndexReader};
use crate::reader::BlkReader;

#[derive(Debug, Clone)]
pub struct BlkReaderConfig {
    pub blocks_dir: PathBuf,
    pub index_dir: PathBuf,
    pub start_height: u32,
    pub end_height: u32,
    pub read_buffer_bytes: usize,
}

impl Default for BlkReaderConfig {
    fn default() -> Self {
        Self {
            blocks_dir: PathBuf::new(),
            index_dir: PathBuf::new(),
            start_height: 0,
            end_height: u32::MAX,
            read_buffer_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub struct RawBlock {
    pub height: u32,
    pub hash: [u8; 32],
    pub data: Vec<u8>,
}

pub struct BlockIterator {
    sorted_entries: Vec<BlockIndexEntry>,
    cursor: usize,
    reader: BlkReader,
}

impl BlockIterator {
    pub fn new(config: BlkReaderConfig) -> Result<Self, BlkReaderError> {
        let all_entries = IndexReader::read_all(&config.index_dir)?;

        let mut filtered: Vec<BlockIndexEntry> = all_entries
            .into_iter()
            .filter(|e| {
                e.is_valid_and_available()
                    && e.height >= config.start_height
                    && e.height < config.end_height
            })
            .collect();

        filtered.sort_unstable_by_key(|e| e.height);

        tracing::info!(
            count = filtered.len(),
            start_height = config.start_height,
            end_height = config.end_height,
            "BlockIterator: entries filtered and sorted"
        );

        let reader = BlkReader::new(config.blocks_dir, config.read_buffer_bytes);

        Ok(Self {
            sorted_entries: filtered,
            cursor: 0,
            reader,
        })
    }
}

impl Iterator for BlockIterator {
    type Item = Result<RawBlock, BlkReaderError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.sorted_entries.len() {
            return None;
        }

        let entry = &self.sorted_entries[self.cursor];
        self.cursor += 1;

        let result = self.reader
            .read_block_at(entry.n_file, entry.n_data_pos)
            .map(|data| RawBlock {
                height: entry.height,
                hash: entry.hash,
                data,
            });

        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::index::BlockIndexEntry;

    fn make_entry(height: u32, status: u32, n_file: u32, n_data_pos: u32) -> BlockIndexEntry {
        BlockIndexEntry {
            hash: [0u8; 32],
            height,
            status,
            n_file,
            n_data_pos,
        }
    }

    #[test]
    fn test_filters_orphans() {
        let orphan = make_entry(100, 0x00, 0, 0);
        assert!(!orphan.is_valid_and_available());

        let valid_no_data = make_entry(101, 0x04, 0, 0);
        assert!(!valid_no_data.is_valid_and_available());

        let valid = make_entry(102, 0x04 | 0x08, 0, 0);
        assert!(valid.is_valid_and_available());
    }

    #[test]
    fn test_filters_by_height_range() {
        let entries = vec![
            make_entry(0, 0x0C, 0, 0),
            make_entry(5, 0x0C, 0, 0),
            make_entry(10, 0x0C, 0, 0),
            make_entry(15, 0x0C, 0, 0),
        ];

        let filtered: Vec<_> = entries
            .iter()
            .filter(|e| e.is_valid_and_available() && e.height >= 5 && e.height < 10)
            .collect();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].height, 5);
    }

    #[test]
    fn test_sorts_by_height() {
        let mut entries = vec![
            make_entry(300, 0x0C, 0, 0),
            make_entry(100, 0x0C, 0, 0),
            make_entry(200, 0x0C, 0, 0),
        ];
        entries.sort_unstable_by_key(|e| e.height);

        assert_eq!(entries[0].height, 100);
        assert_eq!(entries[1].height, 200);
        assert_eq!(entries[2].height, 300);
        for i in 1..entries.len() {
            assert!(entries[i].height > entries[i - 1].height);
        }
    }
}
