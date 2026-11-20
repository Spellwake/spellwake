//! Compile a conservative native runtime-input identity; no runtime environment trust.
#[path = "build_support/native_identity.rs"]
mod native_identity;
use std::{collections::BTreeMap, env, fs, io, path::PathBuf, process::Command};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| io::Error::other("missing Cargo manifest root"))?,
    );
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| io::Error::other("missing repository root"))?;
    let rustc =
        Command::new(env::var_os("RUSTC").ok_or_else(|| io::Error::other("missing actual rustc"))?)
            .arg("-vV")
            .output()?;
    if !rustc.status.success() {
        return Err(io::Error::other("cannot identify actual compiler").into());
    }
    let mut metadata = BTreeMap::from([("RUSTC_VERSION".into(), String::from_utf8(rustc.stdout)?)]);
    for name in ["TARGET", "HOST", "PROFILE", "OPT_LEVEL", "DEBUG"] {
        metadata.insert(name.into(), env::var(name)?);
    }
    metadata.insert(
        "CARGO_ENCODED_RUSTFLAGS".into(),
        env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default(),
    );
    for (name, value) in env::vars_os() {
        if let Some(name) = name.to_str()
            && (name.starts_with("CARGO_CFG_") || name.starts_with("CARGO_FEATURE_"))
        {
            let value = value
                .into_string()
                .map_err(|_| io::Error::other("nonUTF8 Cargo build metadata"))?;
            metadata.insert(name.to_owned(), value);
        }
    }
    for name in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_BOOTSTRAP"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let fingerprint = native_identity::fingerprint(root, &metadata)?;
    for path in &fingerprint.watches {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!(
        "cargo:rustc-env=SPELLWAKE_NATIVE_RUNTIME_ID={}",
        fingerprint.identity
    );
    let path = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("missing Cargo output directory"))?,
    )
    .join("native-runtime-inputs.json");
    let bytes = serde_json::to_vec_pretty(&fingerprint)?;
    if fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
        fs::write(path, bytes)?;
    }
    Ok(())
}
