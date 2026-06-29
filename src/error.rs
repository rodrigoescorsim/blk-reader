#[derive(Debug, thiserror::Error)]
pub enum BlkReaderError {
    #[error("Failed to open LevelDB at {path}: {reason}")]
    LevelDbOpen {
        path: std::path::PathBuf,
        reason: String,
    },

    #[error("Error parsing index entry: {reason}")]
    IndexParseError { reason: String },

    #[error("File blk{index:05}.dat not found")]
    BlkFileNotFound { index: u32 },

    #[error(
        "Invalid magic bytes in file {file}, offset {offset}: expected 0xD9B4BEF9, got {got:#010x}"
    )]
    InvalidMagicBytes { file: u32, offset: u64, got: u32 },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
