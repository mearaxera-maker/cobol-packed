use super::TextEncoding;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct EncodingInfo {
    pub(super) encoding: String,
    pub(super) field_type: &'static str,
    pub(super) byte_model: &'static str,
    pub(super) aliases: Vec<String>,
    pub(super) notes: &'static str,
}

pub(super) fn all() -> Vec<EncodingInfo> {
    ENCODINGS
        .iter()
        .copied()
        .map(|encoding| {
            let mixed_dbcs = encoding.is_mixed_dbcs();
            EncodingInfo {
                encoding: encoding.to_string(),
                field_type: if mixed_dbcs {
                    "mixed-dbcs-text"
                } else {
                    "display-text"
                },
                byte_model: if mixed_dbcs {
                    "stateful-mixed-dbcs"
                } else {
                    "single-byte"
                },
                aliases: aliases_for(encoding),
                notes: if mixed_dbcs {
                    "Stateful EBCDIC mixed DBCS with SO/SI shifts; use field_type mixed-dbcs-text."
                } else {
                    "Single-byte EBCDIC display text; use field_type display-text."
                },
            }
        })
        .collect()
}

const ENCODINGS: &[TextEncoding] = &[
    TextEncoding::Cp037,
    TextEncoding::Cp273,
    TextEncoding::Cp277,
    TextEncoding::Cp278,
    TextEncoding::Cp280,
    TextEncoding::Cp284,
    TextEncoding::Cp285,
    TextEncoding::Cp290,
    TextEncoding::Cp297,
    TextEncoding::Cp420,
    TextEncoding::Cp423,
    TextEncoding::Cp424,
    TextEncoding::Cp500,
    TextEncoding::Cp833,
    TextEncoding::Cp838,
    TextEncoding::Cp870,
    TextEncoding::Cp871,
    TextEncoding::Cp875,
    TextEncoding::Cp880,
    TextEncoding::Cp905,
    TextEncoding::Cp924,
    TextEncoding::Cp930,
    TextEncoding::Cp933,
    TextEncoding::Cp935,
    TextEncoding::Cp937,
    TextEncoding::Cp939,
    TextEncoding::Cp1025,
    TextEncoding::Cp1026,
    TextEncoding::Cp1047,
    TextEncoding::Cp1140,
    TextEncoding::Cp1141,
    TextEncoding::Cp1142,
    TextEncoding::Cp1143,
    TextEncoding::Cp1144,
    TextEncoding::Cp1145,
    TextEncoding::Cp1146,
    TextEncoding::Cp1147,
    TextEncoding::Cp1148,
    TextEncoding::Cp1149,
];

fn aliases_for(encoding: TextEncoding) -> Vec<String> {
    let canonical = encoding.to_string();
    let number = canonical.trim_start_matches("cp").to_string();
    let unpadded = number.trim_start_matches('0');
    let unpadded = if unpadded.is_empty() {
        number.clone()
    } else {
        unpadded.to_string()
    };
    let mut aliases = vec![
        canonical,
        format!("ibm{}", number),
        format!("ccsid{}", number),
        number.clone(),
    ];
    if unpadded != number {
        aliases.push(format!("cp{}", unpadded));
        aliases.push(format!("ibm{}", unpadded));
        aliases.push(format!("ccsid{}", unpadded));
        aliases.push(unpadded);
    }
    if let Some(alias) = windows_codepage_alias(encoding) {
        aliases.push(alias.to_string());
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn windows_codepage_alias(encoding: TextEncoding) -> Option<&'static str> {
    match encoding {
        TextEncoding::Cp273 => Some("20273"),
        TextEncoding::Cp277 => Some("20277"),
        TextEncoding::Cp278 => Some("20278"),
        TextEncoding::Cp280 => Some("20280"),
        TextEncoding::Cp284 => Some("20284"),
        TextEncoding::Cp285 => Some("20285"),
        TextEncoding::Cp290 => Some("20290"),
        TextEncoding::Cp297 => Some("20297"),
        TextEncoding::Cp420 => Some("20420"),
        TextEncoding::Cp423 => Some("20423"),
        TextEncoding::Cp424 => Some("20424"),
        TextEncoding::Cp833 => Some("20833"),
        TextEncoding::Cp838 => Some("20838"),
        TextEncoding::Cp871 => Some("20871"),
        TextEncoding::Cp880 => Some("20880"),
        TextEncoding::Cp905 => Some("20905"),
        TextEncoding::Cp924 => Some("20924"),
        TextEncoding::Cp930 => Some("50930"),
        TextEncoding::Cp933 => Some("50933"),
        TextEncoding::Cp935 => Some("50935"),
        TextEncoding::Cp937 => Some("50937"),
        TextEncoding::Cp939 => Some("50939"),
        TextEncoding::Cp1025 => Some("21025"),
        TextEncoding::Cp1047 => Some("01047"),
        TextEncoding::Cp1140 => Some("01140"),
        TextEncoding::Cp1141 => Some("01141"),
        TextEncoding::Cp1142 => Some("01142"),
        TextEncoding::Cp1143 => Some("01143"),
        TextEncoding::Cp1144 => Some("01144"),
        TextEncoding::Cp1145 => Some("01145"),
        TextEncoding::Cp1146 => Some("01146"),
        TextEncoding::Cp1147 => Some("01147"),
        TextEncoding::Cp1148 => Some("01148"),
        TextEncoding::Cp1149 => Some("01149"),
        TextEncoding::Cp037
        | TextEncoding::Cp500
        | TextEncoding::Cp870
        | TextEncoding::Cp875
        | TextEncoding::Cp1026 => None,
    }
}
