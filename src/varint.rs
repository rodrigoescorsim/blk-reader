use crate::error::BlkReaderError;

/// Decodes the custom Bitcoin Core varint (used in the LevelDB index).
///
/// Format: big-endian, 7 bits per byte. MSB=1 → more bytes follow.
/// Each continuation byte adds 1 to the accumulated value.
pub fn read_varint(data: &[u8], pos: &mut usize) -> Result<u64, BlkReaderError> {
    let mut n: u64 = 0;
    loop {
        if *pos >= data.len() {
            return Err(BlkReaderError::IndexParseError {
                reason: "Buffer truncated while reading varint".to_string(),
            });
        }
        let byte = data[*pos];
        *pos += 1;
        n = (n << 7) | (byte & 0x7F) as u64;
        if byte & 0x80 != 0 {
            n += 1;
        } else {
            return Ok(n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> u64 {
        read_varint(bytes, &mut 0).expect("decode failed")
    }

    #[test]
    fn test_varint_zero() {
        assert_eq!(decode(&[0x00]), 0);
    }

    #[test]
    fn test_varint_127() {
        assert_eq!(decode(&[0x7F]), 127);
    }

    #[test]
    fn test_varint_128() {
        assert_eq!(decode(&[0x80, 0x00]), 128);
    }

    #[test]
    fn test_varint_255() {
        assert_eq!(decode(&[0x80, 0x7F]), 255);
    }

    #[test]
    fn test_varint_256() {
        assert_eq!(decode(&[0x81, 0x00]), 256);
    }

    #[test]
    fn test_varint_truncated_returns_error() {
        let result = read_varint(&[0x80], &mut 0);
        assert!(result.is_err());
    }
}
