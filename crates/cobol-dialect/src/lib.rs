use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DialectName {
    IbmZos,
    GnuCobol,
    MicroFocus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollatingSequence {
    Ascii,
    Ebcdic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImplicitSubjectScope {
    CrossParentheses,
    ParenthesizedGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Numproc {
    Pfd,
    Nopfd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TruncationMode {
    Std,
    Bin,
    Opt,
    FailClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscriptPolicy {
    Strict,
    NoBounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvalidNumericPolicy {
    Error,
    TreatAsZero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OdoGroupLengthRule {
    Maximum,
    Current,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceCharset {
    Ascii,
    Ebcdic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FloatFormat {
    IbmHex,
    IeeeBinary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialectProfile {
    pub name: DialectName,
    pub collating_sequence: CollatingSequence,
    pub implicit_subject_scope: ImplicitSubjectScope,
    pub numproc: Numproc,
    pub truncation: TruncationMode,
    pub arith_digits: u8,
    pub subscript_policy: SubscriptPolicy,
    pub invalid_numeric_policy: InvalidNumericPolicy,
    pub odo_group_length_rule: OdoGroupLengthRule,
    pub source_charset: SourceCharset,
    pub float_format: FloatFormat,
}

impl DialectProfile {
    pub fn ibm_zos() -> Self {
        Self {
            name: DialectName::IbmZos,
            collating_sequence: CollatingSequence::Ebcdic,
            implicit_subject_scope: ImplicitSubjectScope::CrossParentheses,
            numproc: Numproc::Pfd,
            truncation: TruncationMode::FailClosed,
            arith_digits: 18,
            subscript_policy: SubscriptPolicy::Strict,
            invalid_numeric_policy: InvalidNumericPolicy::Error,
            odo_group_length_rule: OdoGroupLengthRule::Maximum,
            source_charset: SourceCharset::Ebcdic,
            float_format: FloatFormat::IbmHex,
        }
    }

    pub fn gnucobol() -> Self {
        Self {
            name: DialectName::GnuCobol,
            collating_sequence: CollatingSequence::Ascii,
            implicit_subject_scope: ImplicitSubjectScope::ParenthesizedGroup,
            numproc: Numproc::Pfd,
            truncation: TruncationMode::FailClosed,
            arith_digits: 18,
            subscript_policy: SubscriptPolicy::Strict,
            invalid_numeric_policy: InvalidNumericPolicy::Error,
            odo_group_length_rule: OdoGroupLengthRule::Maximum,
            source_charset: SourceCharset::Ascii,
            float_format: FloatFormat::IeeeBinary,
        }
    }

    pub fn micro_focus() -> Self {
        Self {
            name: DialectName::MicroFocus,
            collating_sequence: CollatingSequence::Ascii,
            implicit_subject_scope: ImplicitSubjectScope::ParenthesizedGroup,
            numproc: Numproc::Nopfd,
            truncation: TruncationMode::FailClosed,
            arith_digits: 18,
            subscript_policy: SubscriptPolicy::Strict,
            invalid_numeric_policy: InvalidNumericPolicy::Error,
            odo_group_length_rule: OdoGroupLengthRule::Maximum,
            source_charset: SourceCharset::Ascii,
            float_format: FloatFormat::IeeeBinary,
        }
    }
}
