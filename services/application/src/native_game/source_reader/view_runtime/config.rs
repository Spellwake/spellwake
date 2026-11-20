//! Optional trusted startup configuration, never client request input.
use super::*;
use crate::native_game::MAX_NATIVE_ARTIFACT_BYTES;
use std::{fs::File, io::Read, time::Duration};

/// Safe startup failure retaining the underlying private source/IO cause.
pub(crate) enum ConfigError {
    Invalid,
    Io(std::io::Error),
    Initial(NativeInitialError),
    Artifact(NativeArtifactError),
    Backend(crate::backend::BackendError),
    Budget(NativeCheckpointError),
}
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("native view configuration is unavailable")
    }
}
impl fmt::Debug for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invalid => None,
            Self::Io(e) => Some(e),
            Self::Initial(e) => Some(e),
            Self::Artifact(e) => Some(e),
            Self::Backend(e) => Some(e),
            Self::Budget(e) => Some(e),
        }
    }
}
fn required(name: &str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| ConfigError::Invalid)
}
fn read_file(path: &str, maximum: u64) -> Result<Vec<u8>, ConfigError> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(ConfigError::Io)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(ConfigError::Io)?;
    if bytes.is_empty() || u64::try_from(bytes.len()).map_err(|_| ConfigError::Invalid)? > maximum {
        return Err(ConfigError::Invalid);
    }
    Ok(bytes)
}
impl NativeGameRuntime {
    pub(crate) fn from_env(base: &str, key: &str) -> Result<Option<Self>, ConfigError> {
        let pool = match std::env::var("SPELLWAKE_NATIVE_VIEWS_POOL") {
            Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => return Err(ConfigError::Invalid),
            Ok(pool) => pool,
        };
        Self::from_values(base, key, &pool, required).map(Some)
    }
    /// Test-only runtime over the same configured inputs but an explicit pool,
    /// to exercise reads whose original program source is unavailable.
    #[cfg(test)]
    pub(in crate::native_game::source_reader) fn with_pool(
        base: &str,
        key: &str,
        pool: &str,
    ) -> Result<Self, ConfigError> {
        Self::from_values(base, key, pool, required)
    }
    fn from_values(
        base: &str,
        key: &str,
        pool: &str,
        lookup: impl Fn(&str) -> Result<String, ConfigError>,
    ) -> Result<Self, ConfigError> {
        let pool = match pool {
            "resolution-selection" => NativeProgramPool::ResolutionSelection,
            "resolution-selection-natural-mana" => {
                NativeProgramPool::ResolutionSelectionNaturalMana
            }
            _ => return Err(ConfigError::Invalid),
        };
        // Reject invalid limits/settings before loading the retained source package.
        let records = lookup("SPELLWAKE_NATIVE_RECOVERY_MAX_RECORDS")?
            .parse()
            .map_err(|_| ConfigError::Invalid)?;
        let bytes = lookup("SPELLWAKE_NATIVE_RECOVERY_MAX_BYTES")?
            .parse()
            .map_err(|_| ConfigError::Invalid)?;
        let budget = NativeRecoveryBudget::new(records, bytes).map_err(ConfigError::Budget)?;
        let bucket = lookup("SPELLWAKE_NATIVE_ARTIFACT_BUCKET")?;
        if !crate::private_storage::bucket_name(&bucket) {
            return Err(ConfigError::Invalid);
        }
        let metadata = read_file(&lookup("SPELLWAKE_CATALOGUE_METADATA")?, 1_048_576)?;
        let snapshot = read_file(
            &lookup("SPELLWAKE_CATALOGUE_INPUT")?,
            MAX_NATIVE_ARTIFACT_BYTES,
        )?;
        let vocabulary = read_file(&lookup("SPELLWAKE_CREATURE_VOCABULARY_INPUT")?, 1_048_576)?;
        let programs = NativeProgramSource::from_bytes(pool, &metadata, snapshot, &vocabulary)
            .map_err(ConfigError::Initial)?;
        let backend = crate::backend::BackendConfig::new(base, key)
            .and_then(|config| {
                crate::backend::BackendClient::new(
                    config,
                    "spellwake-native-views",
                    Duration::from_secs(60),
                )
            })
            .map_err(ConfigError::Backend)?;
        let store = NativeArtifactStore::new(backend, bucket).map_err(ConfigError::Artifact)?;
        Ok(Self::new(programs, store, budget))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Input(std::path::PathBuf);
    impl Drop for Input {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    #[test]
    fn source_files_are_read_with_exact_bounds_and_private_io_errors() {
        let file =
            Input(std::env::temp_dir().join(format!("spellwake-native-source-{}", Uuid::new_v4())));
        std::fs::write(&file.0, b"abc").unwrap();
        let path = file.0.to_str().unwrap();
        assert_eq!(read_file(path, 3).unwrap(), b"abc");
        assert!(matches!(read_file(path, 2), Err(ConfigError::Invalid)));
        std::fs::write(&file.0, b"").unwrap();
        assert!(matches!(read_file(path, 3), Err(ConfigError::Invalid)));
        std::fs::remove_file(&file.0).unwrap();
        let failure = read_file(path, 3).unwrap_err();
        assert!(failure.source().is_some());
        assert!(!failure.to_string().contains(path));
    }

    #[test]
    fn startup_rejects_unknown_pool_missing_settings_bad_limits_bucket_and_source() {
        let file =
            Input(std::env::temp_dir().join(format!("spellwake-native-config-{}", Uuid::new_v4())));
        std::fs::write(&file.0, b"not a qualified source package").unwrap();
        let path = file.0.to_str().unwrap();
        let values = std::collections::BTreeMap::from([
            ("SPELLWAKE_NATIVE_RECOVERY_MAX_RECORDS", "10".to_owned()),
            ("SPELLWAKE_NATIVE_RECOVERY_MAX_BYTES", "10000000".to_owned()),
            (
                "SPELLWAKE_NATIVE_ARTIFACT_BUCKET",
                "private-native".to_owned(),
            ),
            ("SPELLWAKE_CATALOGUE_METADATA", path.to_owned()),
            ("SPELLWAKE_CATALOGUE_INPUT", path.to_owned()),
            ("SPELLWAKE_CREATURE_VOCABULARY_INPUT", path.to_owned()),
        ]);
        let load = |pool: &str, fields: &std::collections::BTreeMap<&str, String>| {
            NativeGameRuntime::from_values(
                "http://127.0.0.1:9",
                "sb_secret_test_only",
                pool,
                |name| fields.get(name).cloned().ok_or(ConfigError::Invalid),
            )
        };
        assert!(matches!(load("latest", &values), Err(ConfigError::Invalid)));
        for key in values.keys() {
            let mut missing = values.clone();
            missing.remove(key);
            assert!(load("resolution-selection", &missing).is_err());
        }
        for (key, value) in [
            ("SPELLWAKE_NATIVE_RECOVERY_MAX_RECORDS", "0"),
            ("SPELLWAKE_NATIVE_RECOVERY_MAX_BYTES", "0"),
            ("SPELLWAKE_NATIVE_RECOVERY_MAX_RECORDS", "-1"),
            ("SPELLWAKE_NATIVE_RECOVERY_MAX_BYTES", "1.5"),
            ("SPELLWAKE_NATIVE_ARTIFACT_BUCKET", "../foreign"),
        ] {
            let mut invalid = values.clone();
            invalid.insert(key, value.to_owned());
            assert!(matches!(
                load("resolution-selection", &invalid),
                Err(ConfigError::Invalid | ConfigError::Budget(_))
            ));
        }
        for pool in ["resolution-selection", "resolution-selection-natural-mana"] {
            assert!(matches!(load(pool, &values), Err(ConfigError::Initial(_))));
        }
    }
}
