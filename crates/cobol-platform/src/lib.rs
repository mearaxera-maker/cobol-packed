use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformConfig {
    #[serde(default)]
    pub files: Vec<FileBinding>,
}

impl PlatformConfig {
    pub fn from_json_str(text: &str) -> Result<Self, PlatformError> {
        serde_json::from_str(text).map_err(PlatformError::Json)
    }

    pub fn from_json_file(path: &Path) -> Result<Self, PlatformError> {
        let text = fs::read_to_string(path).map_err(|source| PlatformError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_json_str(&text)
    }

    pub fn validate(&self) -> Result<(), PlatformError> {
        let mut names = BTreeMap::<String, usize>::new();
        for (index, file) in self.files.iter().enumerate() {
            file.validate(index)?;
            let normalized = normalize_file_name(&file.name);
            if let Some(first_index) = names.insert(normalized, index) {
                return Err(PlatformError::DuplicateFileName {
                    name: file.name.clone(),
                    first_index,
                    second_index: index,
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBinding {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub organization: DatasetOrganization,
    #[serde(default)]
    pub record_format: RecordFormat,
    #[serde(default)]
    pub disposition: FileDisposition,
    #[serde(default)]
    pub encoding: DataEncoding,
}

impl FileBinding {
    pub fn record_len(&self) -> Option<usize> {
        match self.record_format {
            RecordFormat::Fixed { record_len } => Some(record_len),
            RecordFormat::Variable | RecordFormat::LineSequential => None,
        }
    }

    fn validate(&self, index: usize) -> Result<(), PlatformError> {
        if self.name.trim().is_empty() {
            return Err(PlatformError::EmptyFileName { index });
        }
        if self.name.trim() != self.name {
            return Err(PlatformError::InvalidFileName {
                name: self.name.clone(),
                message: "file name must not contain leading or trailing whitespace".to_string(),
            });
        }
        if self.path.as_os_str().is_empty() {
            return Err(PlatformError::EmptyHostPath {
                name: self.name.clone(),
            });
        }
        if self.encoding != DataEncoding::Ascii {
            return Err(PlatformError::UnsupportedEncoding {
                name: self.name.clone(),
                encoding: self.encoding,
            });
        }
        match self.organization {
            DatasetOrganization::Sequential => {}
            DatasetOrganization::Indexed
            | DatasetOrganization::Relative
            | DatasetOrganization::Vsam => {
                return Err(PlatformError::UnsupportedOrganization {
                    name: self.name.clone(),
                    organization: self.organization,
                });
            }
        }
        match self.record_format {
            RecordFormat::Fixed { record_len } if record_len > 0 => Ok(()),
            RecordFormat::Fixed { .. } => Err(PlatformError::InvalidFixedRecordLen {
                name: self.name.clone(),
            }),
            RecordFormat::Variable | RecordFormat::LineSequential => {
                Err(PlatformError::UnsupportedRecordFormat {
                    name: self.name.clone(),
                    record_format: self.record_format.clone(),
                })
            }
        }
    }
}

fn normalize_file_name(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .replace('-', "_")
        .to_ascii_uppercase()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatasetOrganization {
    #[default]
    Sequential,
    Indexed,
    Relative,
    Vsam,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RecordFormat {
    Fixed { record_len: usize },
    Variable,
    LineSequential,
}

impl Default for RecordFormat {
    fn default() -> Self {
        Self::Fixed { record_len: 0 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileDisposition {
    #[default]
    Old,
    Shr,
    New,
    Mod,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataEncoding {
    #[default]
    Ascii,
    Ebcdic037,
    Ebcdic500,
    Ebcdic1140,
    Ebcdic1148,
    Binary,
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("failed to read platform config {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse platform config JSON: {0}")]
    Json(serde_json::Error),
    #[error("platform file binding at index {index} has an empty name")]
    EmptyFileName { index: usize },
    #[error("platform file binding {name} is invalid: {message}")]
    InvalidFileName { name: String, message: String },
    #[error(
        "platform file binding {name} at index {second_index} duplicates a prior file binding at index {first_index}"
    )]
    DuplicateFileName {
        name: String,
        first_index: usize,
        second_index: usize,
    },
    #[error("platform file binding {name} has an empty host path")]
    EmptyHostPath { name: String },
    #[error("platform file binding {name} uses fixed records with record_len 0")]
    InvalidFixedRecordLen { name: String },
    #[error("platform file binding {name} uses unsupported encoding {encoding:?}")]
    UnsupportedEncoding {
        name: String,
        encoding: DataEncoding,
    },
    #[error("platform file binding {name} uses unsupported organization {organization:?}")]
    UnsupportedOrganization {
        name: String,
        organization: DatasetOrganization,
    },
    #[error("platform file binding {name} uses unsupported record format {record_format:?}")]
    UnsupportedRecordFormat {
        name: String,
        record_format: RecordFormat,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates_fixed_sequential_file() {
        let config = PlatformConfig::from_json_str(
            r#"{
                "files": [{
                    "name": "INFILE",
                    "path": "input.dat",
                    "organization": "sequential",
                    "record_format": { "kind": "fixed", "record_len": 3 },
                    "disposition": "old",
                    "encoding": "ascii"
                }]
            }"#,
        )
        .expect("valid config");

        assert_eq!(config.files.len(), 1);
        assert_eq!(config.files[0].name, "INFILE");
        assert_eq!(config.files[0].record_len(), Some(3));
        config
            .validate()
            .expect("fixed sequential file is supported");
    }

    #[test]
    fn rejects_empty_host_path() {
        let config = PlatformConfig::from_json_str(
            r#"{
                "files": [{
                    "name": "INFILE",
                    "path": "",
                    "organization": "sequential",
                    "record_format": { "kind": "fixed", "record_len": 3 }
                }]
            }"#,
        )
        .expect("config parses before validation");

        assert!(matches!(
            config.validate(),
            Err(PlatformError::EmptyHostPath { name }) if name == "INFILE"
        ));
    }

    #[test]
    fn rejects_padded_and_duplicate_file_names() {
        let padded = PlatformConfig::from_json_str(
            r#"{
                "files": [{
                    "name": " INFILE ",
                    "path": "input.dat",
                    "record_format": { "kind": "fixed", "record_len": 3 }
                }]
            }"#,
        )
        .expect("padded name config parses");

        assert!(matches!(
            padded.validate(),
            Err(PlatformError::InvalidFileName { name, .. }) if name == " INFILE "
        ));

        let duplicate = PlatformConfig::from_json_str(
            r#"{
                "files": [
                    {
                        "name": "IN-FILE",
                        "path": "input-a.dat",
                        "record_format": { "kind": "fixed", "record_len": 3 }
                    },
                    {
                        "name": "IN_FILE",
                        "path": "input-b.dat",
                        "record_format": { "kind": "fixed", "record_len": 3 }
                    }
                ]
            }"#,
        )
        .expect("duplicate name config parses");

        assert!(matches!(
            duplicate.validate(),
            Err(PlatformError::DuplicateFileName {
                name,
                first_index: 0,
                second_index: 1
            }) if name == "IN_FILE"
        ));
    }

    #[test]
    fn rejects_unsupported_encoding_until_translation_exists() {
        let config = PlatformConfig::from_json_str(
            r#"{
                "files": [{
                    "name": "INFILE",
                    "path": "input.dat",
                    "record_format": { "kind": "fixed", "record_len": 3 },
                    "encoding": "ebcdic037"
                }]
            }"#,
        )
        .expect("encoding intent config parses");

        assert!(matches!(
            config.validate(),
            Err(PlatformError::UnsupportedEncoding { name, encoding })
                if name == "INFILE" && encoding == DataEncoding::Ebcdic037
        ));
    }

    #[test]
    fn unsupported_indexed_config_parses_but_fails_validation() {
        let config = PlatformConfig::from_json_str(
            r#"{
                "files": [{
                    "name": "VSAMFILE",
                    "path": "catalog.ksds",
                    "organization": "indexed",
                    "record_format": { "kind": "fixed", "record_len": 80 }
                }]
            }"#,
        )
        .expect("indexed config parses");

        assert!(matches!(
            config.validate(),
            Err(PlatformError::UnsupportedOrganization { name, organization })
                if name == "VSAMFILE" && organization == DatasetOrganization::Indexed
        ));
    }

    #[test]
    fn unsupported_relative_and_vsam_configs_parse_but_fail_validation() {
        for (organization_name, expected) in [
            ("relative", DatasetOrganization::Relative),
            ("vsam", DatasetOrganization::Vsam),
        ] {
            let config = PlatformConfig::from_json_str(&format!(
                r#"{{
                    "files": [{{
                        "name": "BADFILE",
                        "path": "catalog.dat",
                        "organization": "{organization_name}",
                        "record_format": {{ "kind": "fixed", "record_len": 80 }}
                    }}]
                }}"#
            ))
            .expect("unsupported config parses");

            assert!(
                matches!(
                    config.validate(),
                    Err(PlatformError::UnsupportedOrganization { name, organization })
                        if name == "BADFILE" && organization == expected
                ),
                "{organization_name} should fail validation"
            );
        }
    }

    #[test]
    fn default_config_is_legacy_safe() {
        let config = PlatformConfig::default();
        assert!(config.files.is_empty());
        config.validate().expect("empty platform config is valid");
    }
}
