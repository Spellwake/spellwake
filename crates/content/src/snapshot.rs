//! Shared saved-snapshot boundary for the inventory CLI and native program consumer.
use crate::{BulkMetadata, CatalogError, Inventory};
use flate2::read::MultiGzDecoder;
use sha2::{Digest, Sha256};
use std::{
    io::{BufReader, Cursor},
    path::Path,
};

#[derive(Debug)]
pub enum SnapshotError {
    Invalid {
        field: &'static str,
        message: String,
    },
    Operational(std::io::Error),
}
/// Read and validate saved files without fetching or publishing any catalogue.
pub fn load(metadata_path: &Path, snapshot_path: &Path) -> Result<Inventory, SnapshotError> {
    let metadata = std::fs::read(metadata_path).map_err(SnapshotError::Operational)?;
    let bytes = std::fs::read(snapshot_path).map_err(SnapshotError::Operational)?;
    decode(&metadata, bytes)
}
/// Hash and decode the same owned bytes. Useful for a tiny embedded, explicitly
/// labelled test subset as well as full saved source snapshots.
pub fn decode(metadata: &[u8], bytes: Vec<u8>) -> Result<Inventory, SnapshotError> {
    let metadata: BulkMetadata =
        serde_json::from_slice(metadata).map_err(|error| SnapshotError::Invalid {
            field: "metadata",
            message: error.to_string(),
        })?;
    if bytes.len() as u64 != metadata.compressed_size {
        return Err(SnapshotError::Invalid {
            field: "snapshot",
            message: "compressed length differs from metadata".into(),
        });
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let reader = BufReader::new(MultiGzDecoder::new(Cursor::new(bytes)));
    crate::inventory(reader, metadata, digest).map_err(|error| SnapshotError::Invalid {
        field: "snapshot",
        message: match error {
            CatalogError::Invalid(message) => message,
            CatalogError::Io(error) => error.to_string(),
        },
    })
}
