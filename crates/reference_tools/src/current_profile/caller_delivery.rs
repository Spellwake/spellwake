//! New-scope caller installation bookkeeping over the existing native call rows.
//! This module never executes native operations or copies their result/journal.
use super::*;

/// Exact private capture epochs. The old constructor always selects Current10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureScope {
    Current10,
    ResolutionSelection11,
}
impl CaptureScope {
    pub const fn report_schema_version(self) -> u32 {
        match self {
            Self::Current10 => REPORT_SCHEMA_VERSION,
            Self::ResolutionSelection11 => 11,
        }
    }
    pub const fn witness_schema(self) -> &'static str {
        match self {
            Self::Current10 => "spellwake.current-type-context-witness.v8",
            Self::ResolutionSelection11 => "spellwake.current-type-context-witness.v9",
        }
    }
}

/// Closed caller outcome after a native Result has already been retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "stage")]
pub(crate) enum CallerCompletion {
    Installed {
        accepted_record_pointer: String,
        accepted_append_pointer: String,
    },
    NativeError,
    CallerDeclined,
    RecorderPrepareFailed,
    PolicyAcceptFailed,
    CallerAbortedBeforeInstall,
}
impl CallerCompletion {
    fn installed(&self) -> bool {
        matches!(self, Self::Installed { .. })
    }
}

#[derive(Debug, Clone)]
pub(super) struct PendingCallerDelivery {
    requested: bool,
    completion: Option<CallerCompletion>,
}

impl Recorder {
    #[cfg(test)]
    pub(crate) fn retained_call(&self, call: &str) -> Option<&Value> {
        self.calls.get(call)
    }
    /// The exact selected private capture scope, independent of the record codec.
    pub const fn scope(&self) -> CaptureScope {
        self.scope
    }

    pub(crate) fn begin_caller_delivery(
        &mut self,
        call_id: &str,
        requested: bool,
    ) -> Result<(), String> {
        self.writable()?;
        if self.scope != CaptureScope::ResolutionSelection11 {
            return Err("caller installation requires the selected selection11 scope".into());
        }
        let call = self
            .calls
            .get(call_id)
            .ok_or("caller delivery has no native call")?;
        if !matches!(
            call["owner_method"].as_str(),
            Some(
                "GameState::advance"
                    | "GameState::initialize"
                    | "GameState::initialize_scheduled"
                    | "GameState::initialize_opening"
            )
        ) {
            return Err("caller delivery requires an actual root game call".into());
        }
        if call["adopted"] != false {
            return Err("deferred native call must begin without claimed installation".into());
        }
        if self.caller_delivery.contains_key(call_id) {
            return Err("duplicate caller delivery registration".into());
        }
        self.caller_delivery.insert(
            call_id.into(),
            PendingCallerDelivery {
                requested,
                completion: None,
            },
        );
        Ok(())
    }

    pub(crate) fn validate_caller_completion(
        &self,
        call_id: &str,
        completion: &CallerCompletion,
    ) -> Result<(), String> {
        self.writable()?;
        let pending = self
            .caller_delivery
            .get(call_id)
            .ok_or("unknown caller ticket")?;
        if pending.completion.is_some() {
            return Err("caller ticket was already completed".into());
        }
        let call = self
            .calls
            .get(call_id)
            .ok_or("caller ticket lost native call")?;
        let native_ok = call["result"]["tag"] == "Ok";
        if native_ok == matches!(completion, CallerCompletion::NativeError) {
            return Err("caller completion differs from the actual native Result".into());
        }
        if let CallerCompletion::Installed {
            accepted_record_pointer,
            accepted_append_pointer,
        } = completion
        {
            if !pending.requested
                || call["after_point"].is_null()
                || call["append"]["tag"] != "RulesJournal"
            {
                return Err(
                    "installed caller requires requested native Ok with returned append".into(),
                );
            }
            pointer(accepted_record_pointer)?;
            if accepted_record_pointer
                .split('/')
                .nth(1)
                .is_some_and(|field| {
                    matches!(
                        field,
                        "type_context_witness" | "schema_version" | "record_codec"
                    )
                })
            {
                return Err(
                    "caller accepted pointer enters a reserved generated record field".into(),
                );
            }
            // Report5 initializes at /initialization; accepted rows are an array.
            // The concrete report adapter owns the prefix before this suffix.
            if !(accepted_record_pointer.ends_with("/initialization")
                || accepted_record_pointer
                    .rsplit_once("/accepted/")
                    .is_some_and(|(_, ordinal)| {
                        ordinal
                            .parse::<usize>()
                            .is_ok_and(|value| value.to_string() == ordinal)
                    }))
            {
                return Err(
                    "caller installation requires an initialization or accepted row pointer".into(),
                );
            }
            if self.caller_delivery.values().any(|delivery| {
                matches!(&delivery.completion, Some(CallerCompletion::Installed { accepted_record_pointer: prior, .. }) if prior == accepted_record_pointer)
            }) {
                return Err("caller installation reuses an accepted record pointer".into());
            }
            if *accepted_append_pointer != self.call_append_pointer(call_id)? {
                return Err("caller installation references a different native append".into());
            }
        }
        Ok(())
    }

    pub(crate) fn call_append_pointer(&self, call_id: &str) -> Result<String, String> {
        let ordinal = self
            .call_order
            .iter()
            .position(|id| id == call_id)
            .ok_or("unknown actual native call ordinal")?;
        Ok(format!("/type_context_witness/calls/{ordinal}/append"))
    }

    pub(crate) fn complete_caller_delivery(
        &mut self,
        call_id: &str,
        completion: CallerCompletion,
    ) -> Result<(), String> {
        self.validate_caller_completion(call_id, &completion)?;
        // Both exact keys were checked above. No native operation occurs here.
        self.calls
            .get_mut(call_id)
            .ok_or("caller ticket lost native call")?["adopted"] = json!(completion.installed());
        self.caller_delivery
            .get_mut(call_id)
            .ok_or("unknown caller ticket")?
            .completion = Some(completion);
        Ok(())
    }

    /// Validate references only after the generated witness has its final place.
    pub(super) fn validate_caller_record_pointers(&self, record: &Value) -> Result<(), String> {
        for (id, delivery) in &self.caller_delivery {
            if let Some(CallerCompletion::Installed {
                accepted_record_pointer,
                accepted_append_pointer,
            }) = &delivery.completion
            {
                if record.pointer(accepted_record_pointer).is_none() {
                    return Err("installed caller record pointer is not emitted".into());
                }
                if record.pointer(accepted_append_pointer) != Some(&self.calls[id]["append"]) {
                    return Err(
                        "installed caller append pointer differs from its emitted native append"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn caller_delivery_value(&self) -> Result<Value, String> {
        let mut rows = BTreeMap::new();
        for id in &self.call_order {
            let call = &self.calls[id];
            if !matches!(
                call["owner_method"].as_str(),
                Some(
                    "GameState::advance"
                        | "GameState::initialize"
                        | "GameState::initialize_scheduled"
                        | "GameState::initialize_opening"
                )
            ) {
                continue;
            }
            let pending = self
                .caller_delivery
                .get(id)
                .ok_or("root native call lacks caller delivery")?;
            let completion = pending
                .completion
                .as_ref()
                .ok_or("caller ticket is not completed")?;
            if call["adopted"] != completion.installed() {
                return Err(
                    "native call adoption differs from finalized caller installation".into(),
                );
            }
            rows.insert(
                id,
                json!({"requested":pending.requested,"completion":completion}),
            );
        }
        if rows.len() != self.caller_delivery.len() {
            return Err("caller delivery contains a foreign native call".into());
        }
        Ok(json!({"semantics":"ActualCallerInstallation.v1","calls":rows}))
    }
}
