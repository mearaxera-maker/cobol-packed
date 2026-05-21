use super::{ebcdic_tables, TextEncoding};

pub(super) fn decode(bytes: &[u8], encoding: TextEncoding) -> Result<String, String> {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        out.push(ebcdic_tables::decode_byte(encoding, byte).ok_or_else(|| {
            format!("byte 0x{byte:02X} is undefined for EBCDIC encoding {encoding}")
        })?);
    }
    Ok(out)
}

pub(super) fn encode(text: &str, encoding: TextEncoding) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        out.push(ebcdic_tables::encode_char(encoding, ch).ok_or_else(|| {
            format!(
                "character U+{:04X} cannot be encoded as {encoding}",
                ch as u32
            )
        })?);
    }
    Ok(out)
}
