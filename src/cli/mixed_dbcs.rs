use super::{ebcdic_tables, mixed_dbcs_tables, TextEncoding};

const SHIFT_OUT: u8 = 0x0E;
const SHIFT_IN: u8 = 0x0F;

pub(super) fn decode(bytes: &[u8], encoding: TextEncoding) -> Result<String, String> {
    let sbcs = profile(encoding)?;
    let mut out = String::with_capacity(bytes.len());
    let mut idx = 0usize;
    let mut dbcs = false;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if !dbcs {
            match byte {
                SHIFT_OUT => {
                    dbcs = true;
                    idx += 1;
                }
                SHIFT_IN => {
                    return Err(format!(
                        "stray shift-in byte 0x{SHIFT_IN:02X} at offset {idx}"
                    ));
                }
                _ => {
                    out.push(ebcdic_tables::decode_byte(sbcs, byte).ok_or_else(|| {
                        format!("byte 0x{byte:02X} is undefined for SBCS side of {encoding}")
                    })?);
                    idx += 1;
                }
            }
            continue;
        }
        if byte == SHIFT_IN {
            dbcs = false;
            idx += 1;
            continue;
        }
        if byte == SHIFT_OUT {
            return Err(format!(
                "nested shift-out byte 0x{SHIFT_OUT:02X} at offset {idx}"
            ));
        }
        let Some(&trail) = bytes.get(idx + 1) else {
            return Err(format!("unterminated DBCS pair at offset {idx}"));
        };
        if trail == SHIFT_IN || trail == SHIFT_OUT {
            return Err(format!(
                "incomplete DBCS pair before shift byte at offset {idx}"
            ));
        }
        out.push(
            mixed_dbcs_tables::decode_pair(encoding, byte, trail).ok_or_else(|| {
                format!("DBCS pair 0x{byte:02X}{trail:02X} is not mapped for {encoding}")
            })?,
        );
        idx += 2;
    }
    if dbcs {
        return Err("unterminated DBCS shift-out sequence".to_string());
    }
    Ok(out)
}

pub(super) fn encode(text: &str, encoding: TextEncoding) -> Result<Vec<u8>, String> {
    let sbcs = profile(encoding)?;
    let mut out = Vec::with_capacity(text.len());
    let mut dbcs = false;
    for ch in text.chars() {
        if let Some(byte) = ebcdic_tables::encode_char(sbcs, ch) {
            if byte == SHIFT_OUT || byte == SHIFT_IN {
                return Err(format!(
                    "character U+{:04X} maps to a reserved SO/SI byte in {encoding}",
                    ch as u32
                ));
            }
            if dbcs {
                out.push(SHIFT_IN);
                dbcs = false;
            }
            out.push(byte);
            continue;
        }
        if let Some((lead, trail)) = mixed_dbcs_tables::encode_char(encoding, ch) {
            if !dbcs {
                out.push(SHIFT_OUT);
                dbcs = true;
            }
            out.extend([lead, trail]);
            continue;
        }
        return Err(format!(
            "character U+{:04X} cannot be encoded as {encoding}",
            ch as u32
        ));
    }
    if dbcs {
        out.push(SHIFT_IN);
    }
    Ok(out)
}

fn profile(encoding: TextEncoding) -> Result<TextEncoding, String> {
    match encoding {
        TextEncoding::Cp930 => Ok(TextEncoding::Cp290),
        TextEncoding::Cp933 => Ok(TextEncoding::Cp833),
        TextEncoding::Cp935 | TextEncoding::Cp937 | TextEncoding::Cp939 => Ok(TextEncoding::Cp037),
        other => Err(format!("{other} is not a mixed DBCS encoding")),
    }
}
