//! Conservative declared-input identity, not binary attestation or semantic proof.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

const TREES: &[&str] = &[
    "services/application/src",
    "services/application/build_support",
    "crates/engine/src",
    "crates/contracts/src",
    "crates/card_programs/src",
    "crates/content/src",
];
const FILES: &[&str] = &[
    "Cargo.toml",
    "rust-toolchain.toml",
    "services/application/Cargo.toml",
    "services/application/Cargo.lock",
    "services/application/build.rs",
    "crates/engine/Cargo.toml",
    "crates/contracts/Cargo.toml",
    "crates/card_programs/Cargo.toml",
    "crates/content/Cargo.toml",
];
/// One exact source or build-metadata digest; metadata values are not disclosed.
#[derive(Serialize)]
pub struct InputDigest {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}
/// Deterministic selected-input record. Source inputs need not be a binary closure.
#[derive(Serialize)]
pub struct Fingerprint {
    pub version: &'static str,
    pub identity: String,
    pub inputs: Vec<InputDigest>,
    #[serde(skip)]
    pub watches: Vec<PathBuf>,
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn failure(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn collect(root: &Path, path: &Path, files: &mut BTreeMap<String, PathBuf>) -> io::Result<()> {
    let info = fs::symlink_metadata(path)?;
    if info.file_type().is_symlink() {
        return Err(failure("native identity refuses source symlinks"));
    }
    if info.is_dir() {
        for entry in fs::read_dir(path)? {
            collect(root, &entry?.path(), files)?;
        }
        return Ok(());
    }
    if !info.is_file() {
        return Err(failure("native identity requires regular source files"));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| failure("native identity source escapes root"))?;
    let name = relative
        .components()
        .map(|part| {
            part.as_os_str()
                .to_str()
                .ok_or_else(|| failure("native identity requires UTF8 paths"))
        })
        .collect::<io::Result<Vec<_>>>()?
        .join("/");
    if files.insert(name, path.to_path_buf()).is_some() {
        return Err(failure("native identity source repeated"));
    }
    Ok(())
}
fn input(path: String, bytes: &[u8]) -> io::Result<InputDigest> {
    Ok(InputDigest {
        path,
        size: u64::try_from(bytes.len())
            .map_err(|_| failure("native identity byte count overflow"))?,
        sha256: hex(&Sha256::digest(bytes)),
    })
}
fn frame(hash: &mut Sha256, bytes: &[u8]) -> io::Result<()> {
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| failure("native identity frame overflow"))?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(())
}
/// Fingerprint all declared source membership and exact bytes plus build metadata.
/// # Errors
/// Rejects unreadable/missing inputs, symlinks, escaping/nonUTF8 paths and overflow.
pub fn fingerprint(root: &Path, metadata: &BTreeMap<String, String>) -> io::Result<Fingerprint> {
    let mut files = BTreeMap::new();
    let mut watches = Vec::new();
    for tree in TREES {
        let path = root.join(tree);
        collect(root, &path, &mut files)?;
        watches.push(path);
    }
    for file in FILES {
        let path = root.join(file);
        collect(root, &path, &mut files)?;
        watches.push(path);
    }
    // Cargo itself tracks effective configuration. Existing repo configuration is
    // additionally a declared input; missing files are represented explicitly.
    let configs = [
        ".cargo/config",
        ".cargo/config.toml",
        "services/application/.cargo/config",
        "services/application/.cargo/config.toml",
    ];
    let mut inputs = Vec::new();
    for config in configs {
        let path = root.join(config);
        if path.exists() {
            collect(root, &path, &mut files)?;
            if let Some(parent) = path.parent() {
                watches.push(parent.to_path_buf());
            }
        } else {
            inputs.push(input(format!("@absent/{config}"), b"")?);
        }
    }
    for (name, path) in files {
        inputs.push(input(name, &fs::read(path)?)?);
    }
    for (name, value) in metadata {
        inputs.push(input(format!("@metadata/{name}"), value.as_bytes())?);
    }
    inputs.sort_by(|a, b| a.path.cmp(&b.path));
    let mut hash = Sha256::new();
    hash.update(b"spellwake.native-runtime-inputs.v1\0");
    for entry in &inputs {
        frame(&mut hash, entry.path.as_bytes())?;
        hash.update(entry.size.to_be_bytes());
        frame(&mut hash, entry.sha256.as_bytes())?;
    }
    watches.sort();
    watches.dedup();
    Ok(Fingerprint {
        version: "native-runtime-inputs-v1",
        identity: hex(&hash.finalize()),
        inputs,
        watches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Tree(PathBuf);
    impl Drop for Tree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn tree() -> Tree {
        let root =
            std::env::temp_dir().join(format!("spellwake-build-inputs-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        for name in TREES {
            let path = root.join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("module.rs"), b"original production source").unwrap();
        }
        for name in FILES {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"declared input").unwrap();
        }
        Tree(root)
    }
    #[test]
    fn relocated_input_tree_is_stable_and_bytes_membership_helpers_and_cfg_are_pinned() {
        let a = tree();
        let b = tree();
        let metadata = BTreeMap::from([
            ("rustc".into(), "pinned compiler".into()),
            ("TARGET".into(), "host".into()),
        ]);
        let initial = fingerprint(&a.0, &metadata).unwrap();
        assert!(
            initial
                .watches
                .contains(&a.0.join("services/application/src"))
        );
        assert!(initial.watches.contains(&a.0.join("crates/engine/src")));
        let original = initial.identity;
        assert_eq!(original, fingerprint(&b.0, &metadata).unwrap().identity);
        assert_eq!(original, fingerprint(&a.0, &metadata).unwrap().identity);
        let helper = a.0.join("services/application/src/provider_json.rs");
        fs::write(&helper, b"object decoder").unwrap();
        let with_helper = fingerprint(&a.0, &metadata).unwrap().identity;
        assert_ne!(original, with_helper);
        fs::write(&helper, b"changed object decoder").unwrap();
        assert_ne!(with_helper, fingerprint(&a.0, &metadata).unwrap().identity);
        fs::remove_file(&helper).unwrap();
        assert_eq!(original, fingerprint(&a.0, &metadata).unwrap().identity);
        let added = a.0.join("crates/engine/src/new-operation.rs");
        fs::write(&added, b"new operation").unwrap();
        assert_ne!(original, fingerprint(&a.0, &metadata).unwrap().identity);
        fs::remove_file(added).unwrap();
        assert_eq!(original, fingerprint(&a.0, &metadata).unwrap().identity);
        let mut changed = metadata.clone();
        changed.insert("TARGET".into(), "different target".into());
        assert_ne!(original, fingerprint(&a.0, &changed).unwrap().identity);
        for key in [
            "RUSTC_VERSION",
            "CARGO_CFG_TARGET_FEATURE",
            "CARGO_ENCODED_RUSTFLAGS",
        ] {
            let mut changed = metadata.clone();
            changed.insert(key.into(), "different declared compiler input".into());
            assert_ne!(original, fingerprint(&a.0, &changed).unwrap().identity);
        }
        let implementation = a.0.join("services/application/build_support/module.rs");
        fs::write(implementation, b"changed fingerprint implementation").unwrap();
        assert_ne!(original, fingerprint(&a.0, &metadata).unwrap().identity);
    }
    #[test]
    fn missing_declared_source_never_uses_an_empty_identity() {
        let a = tree();
        fs::remove_file(a.0.join("rust-toolchain.toml")).unwrap();
        assert!(fingerprint(&a.0, &BTreeMap::new()).is_err());
    }
}
