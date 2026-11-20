//! Private recovery delivery. A detector signs the exact persisted request bytes.
//! Retrying uses the same bytes, MAC and request identity.
use super::*;
use crate::recovery_ports::{
    FinalizeUnrecoverableMatch, PauseGame, PauseMatch, RestoreGame, ResumeMatch,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostExit {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    instance_id: Uuid,
    #[serde(deserialize_with = "crate::recovery_ports::incident_timestamp")]
    incident_at: chrono::DateTime<chrono::Utc>,
    process: i32,
    status: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostExitAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    instance_id: Uuid,
    paused: u32,
    stale: u32,
}
fn incident_mac(body: &[u8]) -> Result<Hmac<Sha256>, Box<dyn Error>> {
    let key = std::env::var("SPELLWAKE_INCIDENT_HMAC_KEY").map_err(|_| invalid())?;
    let key = crate::capability::decode_key(&key).map_err(|_| invalid())?;
    let mut verifier = Hmac::<Sha256>::new_from_slice(&key)?;
    verifier.update(b"spellwake-native-incident-v1\0");
    verifier.update(body);
    Ok(verifier)
}

#[derive(Deserialize)]
#[serde(
    tag = "operation",
    content = "source",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum Incident {
    HostExit(HostExit),
    PauseGame(PauseGame),
    RestoreGame(RestoreGame),
    PauseMatch(PauseMatch),
    ResumeMatch(ResumeMatch),
    FinalizeUnrecoverable(FinalizeUnrecoverableMatch),
}

impl NativeMatchWorker {
    /// Deliver a private incident source authenticated with the independent detector key.
    /// Returns the SQL acknowledgment after verifying its shape and original frontier.
    /// # Errors
    /// Rejects malformed or unsigned requests, unreconstructable checkpoint history,
    /// unavailable sources and invalid acknowledgments.
    pub fn deliver_incident(body: &[u8], signature: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        if body.len() > 16_384 {
            return Err(invalid().into());
        }
        let mac = crate::capability::decode_key(signature).map_err(|_| invalid())?;
        incident_mac(body)?
            .verify_slice(&mac)
            .map_err(|_| invalid())?;
        let source: Incident = crate::decode_wire_object(body).map_err(|_| invalid())?;
        let repo = NativeSourceRepository::from_env()?;
        let rpc = |name, args| repo.backend.rpc_bytes_bounded(name, &args, 16_384);
        let ack = match source {
            Incident::HostExit(p) => {
                let ack = rpc(
                    "pause_native_host_exit",
                    json!({"p_instance":p.instance_id,"p_incident_at":p.incident_at,
                    "p_process":p.process,"p_status":p.status}),
                )?;
                match crate::decode_wire_object::<ApiResponse<HostExitAck>>(&ack)
                    .map_err(|_| invalid())?
                {
                    ApiResponse::Success { data }
                        if data.instance_id == p.instance_id
                            && data.paused.checked_add(data.stale).is_some() =>
                    {
                        ack
                    }
                    _ => return Err(invalid().into()),
                }
            }
            Incident::PauseGame(p) => {
                let ack = rpc(
                    "pause_game_incident",
                    json!({"p_game":p.game_id,"p_owner":p.owner_ref.as_str(),"p_epoch":p.epoch,"p_operational_revision":p.operational_revision,"p_incident_at":p.incident_at,"p_kind":p.kind.as_str(),"p_proof":p.proof_ref.as_str(),"p_request":p.request_id}),
                )?;
                p.decode_response(&ack)?;
                ack
            }
            Incident::RestoreGame(p) => {
                let original = rpc(
                    "read_native_restore_original",
                    json!({"p_game":p.game_id,"p_request":p.request_id,"p_terms":{
                    "version":"game-restoration-v1","gameId":p.game_id,"epoch":p.epoch,"basis":p.operational_revision,"checkpointId":p.checkpoint_id,"owner":p.new_owner_ref.as_str(),"restorationRef":p.restoration_ref.as_str()}}),
                )?;
                if serde_json::from_slice::<Option<serde::de::IgnoredAny>>(&original)?.is_some() {
                    p.decode_response(&original)?;
                    return Ok(original);
                }
                let (owner, runtime) = response_worker::owned_runtime()?;
                let recovered = repo
                    .recover_with_history(
                        p.game_id,
                        &runtime.store,
                        &runtime.programs,
                        runtime.recovery.clone(),
                    )?
                    .ok_or_else(invalid)?;
                let source = recovered.source();
                let (_, epoch, revision, availability, checkpoint) = source.operational_basis();
                if availability != Availability::Recovering
                    || checkpoint != Some(p.checkpoint_id)
                    || source.checkpoint_ids().0 != p.checkpoint_id
                    || epoch != &p.epoch
                    || revision != &p.operational_revision
                    || owner.as_str() != p.new_owner_ref.as_str()
                {
                    return Err(invalid().into());
                }
                let ack = rpc(
                    "restore_game_checkpoint",
                    json!({"p_game":p.game_id,"p_epoch":p.epoch,"p_operational_revision":p.operational_revision,"p_checkpoint":p.checkpoint_id,"p_new_owner":p.new_owner_ref.as_str(),"p_restoration_ref":p.restoration_ref.as_str(),"p_request":p.request_id}),
                )?;
                p.decode_response(&ack)?;
                ack
            }
            Incident::PauseMatch(p) => {
                let ack = rpc(
                    "pause_match_between_games",
                    json!({"p_match":p.match_id,"p_expected":p.basis,"p_availability":p.availability_basis,"p_incident_at":p.incident_at,"p_kind":p.kind.as_str(),"p_proof":p.proof_ref.as_str(),"p_request":p.request_id}),
                )?;
                p.decode_response(&ack)?;
                ack
            }
            Incident::ResumeMatch(p) => {
                let ack = rpc(
                    "resume_match_between_games",
                    json!({"p_match":p.match_id,"p_expected":p.basis,"p_availability":p.availability_basis,"p_incident":p.incident_id,"p_proof":p.proof_ref.as_str(),"p_request":p.request_id}),
                )?;
                p.decode_response(&ack)?;
                ack
            }
            Incident::FinalizeUnrecoverable(p) => {
                let ack = rpc(
                    "finalize_unrecoverable_match",
                    json!({"p_match":p.match_id,"p_expected":p.basis,"p_availability":p.availability_basis,"p_incident":p.incident_id,"p_failure_ref":p.failure_ref.as_str()}),
                )?;
                p.decode_response(&ack)?;
                ack
            }
        };
        Ok(ack)
    }
}

impl NativeMatchWorker {
    /// Register the process incarnation before serving the API's configured host.
    /// # Errors
    /// Refuses an overlapping process or an unavailable registration.
    pub fn register_supervised_host(&self, instance: Uuid) -> Result<(), Box<dyn Error>> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Registration {
            registered: bool,
        }
        let active: Registration = self.repo.native_record(
            "register_native_host_watch",
            &json!({"p_instance":instance,"p_owner":self.owner.as_str()}),
            None,
        )?;
        if !active.registered {
            return Err("configured native host already supervised".into());
        }
        Ok(())
    }

    /// Supervise the local match host under the same configured owner as player commands. Persist
    /// the observed process exit before contacting the database.
    /// # Errors
    /// Reports configuration, process, durable storage and delivery failures.
    pub fn supervise(spool: &Path) -> Result<(), Box<dyn Error>> {
        use std::process::Command;
        incident_mac(b"")?;
        let instance = Uuid::new_v4();
        let name = std::env::var("SPELLWAKE_NATIVE_HOST_OWNER").map_err(|_| invalid())?;
        NativeHostOwner::new(&name)?;
        let dir = File::open(spool)?;
        if !dir.metadata()?.is_dir() {
            return Err(invalid().into());
        }
        let pending = spool.join(format!("{instance}.partial"));
        let original = spool.join(format!("{instance}.incident"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&pending)?;
        let mut child = match Command::new(std::env::current_exe()?)
            .arg("serve-supervised")
            .arg(instance.to_string())
            .env_remove("SPELLWAKE_INCIDENT_HMAC_KEY")
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                std::fs::remove_file(&pending)?;
                return Err(error.into());
            }
        };
        eprintln!("supervising {name} as {instance}");
        let exit = child.wait()?;
        let reason = if let Some(code) = exit.code() {
            format!("exit:{code}")
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                format!("signal:{}", exit.signal().ok_or_else(invalid)?)
            }
            #[cfg(not(unix))]
            {
                return Err(invalid().into());
            }
        };
        let instant = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let body = serde_json::to_string(
            &json!({"operation":"hostExit","source":{"instanceId":instance,
            "incidentAt":instant,"process":child.id(),"status":reason}}),
        )?;
        let tag = URL_SAFE_NO_PAD.encode(incident_mac(body.as_bytes())?.finalize().into_bytes());
        writeln!(file, "{tag}")?;
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&pending, &original)?;
        dir.sync_all()?;
        eprintln!("retained exit evidence {}", original.display());
        let ack = Self::redeliver_incident(&original)?;
        std::io::stdout().write_all(&ack)?;
        if exit.success() {
            Ok(())
        } else {
            Err("supervised native match host exited".into())
        }
    }

    /// Redeliver an immutable signed incident after an unavailable database or lost acknowledgment.
    /// # Errors
    /// Rejects oversized, unsigned or invalid evidence and propagates database refusal.
    pub fn redeliver_incident(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut packet = Vec::new();
        File::open(path)?.take(16_430).read_to_end(&mut packet)?;
        let (signature, body) = packet.split_first_chunk::<44>().ok_or_else(invalid)?;
        if signature[43] != b'\n' || body.len() > 16_384 {
            return Err(invalid().into());
        }
        Self::deliver_incident(
            body,
            std::str::from_utf8(&signature[..43]).map_err(|_| invalid())?,
        )
    }
}
