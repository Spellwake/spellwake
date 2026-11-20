//! Process-owned controller credential verification, separate from SQL authority.
//! Loss of this store invalidates its credentials; durable custody and provider
//! delivery are not installed here. A locator alone never authenticates a caller.
use crate::{amount::Amount, capability::decode_key, source_protocol::SecretReference};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

/// Actual private credential bytes. No implicit serialization or diagnostics.
pub struct ControllerCredential([u8; 32]);
impl ControllerCredential {
    /// Parse one canonical unpadded 32-byte capability without authenticating it.
    /// # Errors
    /// Rejects malformed, padded or noncanonical input without retaining its text.
    pub fn parse(raw: &str) -> Result<Self, ControllerPossessionError> {
        decode_key(raw)
            .map(Self)
            .map_err(|_| ControllerPossessionError(Failure::Invalid))
    }
    /// Expose only for intentional private delivery to the controller process.
    /// Never include this value in SQL, activity payloads, logs or public DTOs.
    pub fn expose_for_delivery(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }
}

/// Safe private boundary failure; supplied credentials and locators are omitted.
pub struct ControllerPossessionError(Failure);
enum Failure {
    Invalid,
    Conflict,
    Unavailable,
    Key(hmac::digest::InvalidLength),
}
impl fmt::Display for ControllerPossessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            Failure::Invalid => "invalid controller credential context",
            Failure::Conflict => "controller generation cannot be reused",
            _ => "controller credential store unavailable",
        })
    }
}
impl fmt::Debug for ControllerPossessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for ControllerPossessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.0 {
            Failure::Key(e) => Some(e),
            _ => None,
        }
    }
}
struct Generation {
    grant: Uuid,
    binding: Uuid,
    epoch: Amount,
    reference: SecretReference,
    verifier: [u8; 32],
}
struct Stored {
    generation: Arc<Generation>,
    revoked: bool,
}
#[derive(Default)]
struct State {
    grants: BTreeMap<Uuid, Stored>,
    references: BTreeSet<String>,
}

/// A concrete local secret registry. Trusted setup installs canonical metadata;
/// installation does not itself prove SQL grant issuance or credential delivery.
/// Revoked generation identities remain tombstoned for this store's lifetime.
#[derive(Default)]
pub struct ControllerSecretStore {
    identity: Arc<()>,
    state: Mutex<State>,
}

/// Opaque local possession proof, tied to one store and immutable generation.
/// This is not a SQL authority lease. Every operation must refresh both local
/// registry validity and canonical SQL authority; no public constructor exists.
pub struct PossessedController {
    owner: Arc<()>,
    generation: Arc<Generation>,
}
impl PossessedController {
    pub(crate) fn grant(&self) -> Uuid {
        self.generation.grant
    }
    pub(crate) fn binding(&self) -> Uuid {
        self.generation.binding
    }
    pub(crate) fn epoch(&self) -> &Amount {
        &self.generation.epoch
    }
    pub(crate) fn reference(&self) -> &str {
        self.generation.reference.as_str()
    }
}
fn mac(
    credential: &ControllerCredential,
    grant: Uuid,
    binding: Uuid,
    epoch: &Amount,
    reference: &str,
) -> Result<Hmac<Sha256>, ControllerPossessionError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(&credential.0)
        .map_err(|e| ControllerPossessionError(Failure::Key(e)))?;
    mac.update(b"spellwake-controller-possession-v1\0");
    let epoch = epoch.to_string();
    for field in [
        grant.as_bytes().as_slice(),
        binding.as_bytes().as_slice(),
        epoch.as_bytes(),
        reference.as_bytes(),
    ] {
        let length =
            u16::try_from(field.len()).map_err(|_| ControllerPossessionError(Failure::Invalid))?;
        mac.update(&length.to_be_bytes());
        mac.update(field);
    }
    Ok(mac)
}
impl ControllerSecretStore {
    /// Mint a generation only from independently resolved, trusted metadata.
    /// Two UUID-v4 draws supply 32 bytes (244 random bits); only its verifier is
    /// retained. The caller must retain original delivery bytes across retries.
    /// # Errors
    /// Rejects nil IDs, any reused grant/locator (including revoked generations),
    /// poisoned storage or cryptographic initialization failure.
    pub fn install(
        &self,
        grant: Uuid,
        binding: Uuid,
        epoch: Amount,
        reference: SecretReference,
    ) -> Result<ControllerCredential, ControllerPossessionError> {
        if grant.is_nil() || binding.is_nil() {
            return Err(ControllerPossessionError(Failure::Invalid));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ControllerPossessionError(Failure::Unavailable))?;
        if state.grants.contains_key(&grant) || state.references.contains(reference.as_str()) {
            return Err(ControllerPossessionError(Failure::Conflict));
        }
        let mut bytes = [0; 32];
        bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
        bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
        let credential = ControllerCredential(bytes);
        let verifier = mac(&credential, grant, binding, &epoch, reference.as_str())?
            .finalize()
            .into_bytes()
            .into();
        state.references.insert(reference.as_str().to_owned());
        state.grants.insert(
            grant,
            Stored {
                generation: Arc::new(Generation {
                    grant,
                    binding,
                    epoch,
                    reference,
                    verifier,
                }),
                revoked: false,
            },
        );
        Ok(credential)
    }
    /// Verify actual credential bytes; untrusted input supplies only grant lookup.
    /// Unknown, revoked or mismatched credentials return None. The original
    /// epoch, binding and locator are derived solely from this store's metadata.
    /// # Errors
    /// Returns a safe store/cryptographic failure without secret diagnostics.
    pub fn authenticate(
        &self,
        grant: Uuid,
        credential: &ControllerCredential,
    ) -> Result<Option<PossessedController>, ControllerPossessionError> {
        let state = self
            .state
            .lock()
            .map_err(|_| ControllerPossessionError(Failure::Unavailable))?;
        let Some(stored) = state.grants.get(&grant).filter(|s| !s.revoked) else {
            return Ok(None);
        };
        let g = &stored.generation;
        if mac(
            credential,
            g.grant,
            g.binding,
            &g.epoch,
            g.reference.as_str(),
        )?
        .verify_slice(&g.verifier)
        .is_err()
        {
            return Ok(None);
        }
        Ok(Some(PossessedController {
            owner: self.identity.clone(),
            generation: g.clone(),
        }))
    }
    /// Invalidate a generation while preserving its non-reuse tombstone.
    /// # Errors
    /// Rejects nil IDs and poisoned storage. False means absent or already revoked.
    pub fn revoke(&self, grant: Uuid) -> Result<bool, ControllerPossessionError> {
        if grant.is_nil() {
            return Err(ControllerPossessionError(Failure::Invalid));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| ControllerPossessionError(Failure::Unavailable))?;
        let Some(stored) = state.grants.get_mut(&grant) else {
            return Ok(false);
        };
        let changed = !stored.revoked;
        stored.revoked = true;
        Ok(changed)
    }
    pub(crate) fn current(
        &self,
        proof: &PossessedController,
    ) -> Result<bool, ControllerPossessionError> {
        if !Arc::ptr_eq(&self.identity, &proof.owner) {
            return Ok(false);
        }
        let state = self
            .state
            .lock()
            .map_err(|_| ControllerPossessionError(Failure::Unavailable))?;
        Ok(state.grants.get(&proof.grant()).is_some_and(|stored| {
            !stored.revoked && Arc::ptr_eq(&stored.generation, &proof.generation)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn install(
        store: &ControllerSecretStore,
        grant: u128,
        reference: &str,
    ) -> ControllerCredential {
        store
            .install(
                Uuid::from_u128(grant),
                Uuid::from_u128(2),
                3.into(),
                reference.parse().unwrap(),
            )
            .unwrap()
    }
    #[test]
    fn actual_credentials_are_required_and_locators_are_not_credentials() {
        let store = ControllerSecretStore::default();
        let token = install(&store, 1, "private:generation:1");
        let parsed = ControllerCredential::parse(&token.expose_for_delivery()).unwrap();
        let proof = store
            .authenticate(Uuid::from_u128(1), &parsed)
            .unwrap()
            .unwrap();
        assert!(store.current(&proof).unwrap());
        assert_eq!(proof.binding(), Uuid::from_u128(2));
        assert_eq!(proof.epoch(), &3.into());
        let wrong = ControllerCredential([0; 32]);
        assert!(
            store
                .authenticate(Uuid::from_u128(1), &wrong)
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .authenticate(Uuid::from_u128(9), &parsed)
                .unwrap()
                .is_none()
        );
        assert!(ControllerCredential::parse("private:generation:1").is_err());
        for raw in [
            format!("{}=", token.expose_for_delivery()),
            "A".repeat(42),
            "!".repeat(43),
            format!(" {}", token.expose_for_delivery()),
        ] {
            assert!(ControllerCredential::parse(&raw).is_err());
        }
    }
    #[test]
    fn revocation_cross_store_and_restart_cannot_reuse_authority() {
        let store = ControllerSecretStore::default();
        let token = install(&store, 1, "private:old");
        let proof = store
            .authenticate(Uuid::from_u128(1), &token)
            .unwrap()
            .unwrap();
        let other = ControllerSecretStore::default();
        assert!(!other.current(&proof).unwrap());
        assert!(
            other
                .authenticate(Uuid::from_u128(1), &token)
                .unwrap()
                .is_none()
        );
        let replacement = install(&other, 1, "private:old");
        assert!(
            other
                .authenticate(Uuid::from_u128(1), &token)
                .unwrap()
                .is_none()
        );
        assert!(!other.current(&proof).unwrap());
        assert!(
            other
                .authenticate(Uuid::from_u128(1), &replacement)
                .unwrap()
                .is_some()
        );
        assert!(store.revoke(Uuid::from_u128(1)).unwrap());
        assert!(!store.current(&proof).unwrap());
        assert!(
            store
                .authenticate(Uuid::from_u128(1), &token)
                .unwrap()
                .is_none()
        );
        assert!(!store.revoke(Uuid::from_u128(1)).unwrap());
        assert!(
            store
                .install(
                    Uuid::from_u128(1),
                    Uuid::from_u128(2),
                    3.into(),
                    "private:new".parse().unwrap()
                )
                .is_err()
        );
        assert!(
            store
                .install(
                    Uuid::from_u128(9),
                    Uuid::from_u128(2),
                    3.into(),
                    "private:old".parse().unwrap()
                )
                .is_err()
        );
        assert!(
            store
                .install(
                    Uuid::nil(),
                    Uuid::from_u128(2),
                    3.into(),
                    "private:nil".parse().unwrap()
                )
                .is_err()
        );
    }
    #[test]
    fn verifier_binds_each_original_metadata_field() {
        let token = ControllerCredential([7; 32]);
        let original = mac(
            &token,
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            &3.into(),
            "ref:one",
        )
        .unwrap()
        .finalize()
        .into_bytes();
        // Independent Python HMAC-SHA256 vector over the documented domain and u16-length framing.
        let expected = "a854cd7357ebea8642c095b18c5e53f420230f477edd0d17ac30296344ffe7f3";
        assert_eq!(
            original
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            expected
        );
        for (grant, binding, epoch, reference) in [
            (9, 2, 3, "ref:one"),
            (1, 9, 3, "ref:one"),
            (1, 2, 4, "ref:one"),
            (1, 2, 3, "ref:two"),
        ] {
            assert!(
                mac(
                    &token,
                    Uuid::from_u128(grant),
                    Uuid::from_u128(binding),
                    &epoch.into(),
                    reference
                )
                .unwrap()
                .verify_slice(&original)
                .is_err()
            );
        }
    }
}
