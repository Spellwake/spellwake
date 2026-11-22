//! Exact selectors over actual candidate appends, never reconstructed identities.
use spellwake_engine::{
    object_identity::ObjectId,
    occurrence::{
        EventCause, RulesJournal, RulesOccurrence, RulesRecord, TriggerInstanceId, TriggerRelated,
    },
    stack_window::StackEntryId,
};

/// One authoritative raw birth-selector contract shared by root/private recipes.
/// JSON indices are canonical decimal strings; native consumers receive checked
/// addressable coordinates. Arrays, extras and unsupported selector flags reject.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "serde_json::Value")]
pub struct TokenCreationSelector {
    pub token_created_by_entry_ref: String,
    pub instruction: usize,
    pub birth_ordinal: usize,
}
impl TryFrom<serde_json::Value> for TokenCreationSelector {
    type Error = String;
    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        if !value.is_object() {
            return Err("token birth selector must be an object".into());
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            token_created_by_entry_ref: String,
            instruction: String,
            birth_ordinal: String,
            field: String,
            require_unique: bool,
        }
        let wire: Wire = serde_json::from_value(value).map_err(|error| error.to_string())?;
        if wire.field != "after_object"
            || !wire.require_unique
            || wire.token_created_by_entry_ref.is_empty()
        {
            return Err("unsupported token birth receipt binding".into());
        }
        let index = |value: String| -> Result<usize, String> {
            let parsed = value.parse::<usize>().map_err(|error| error.to_string())?;
            if parsed.to_string() != value {
                return Err("noncanonical token birth selector coordinate".into());
            }
            Ok(parsed)
        };
        Ok(Self {
            token_created_by_entry_ref: wire.token_created_by_entry_ref,
            instruction: index(wire.instruction)?,
            birth_ordinal: index(wire.birth_ordinal)?,
        })
    }
}
/// Select real births from one exact creating event, in its occurrence order.
/// Absence remains unbound. Duplicate event or object receipts reject before
/// narrowing so an ordinal cannot hide ambiguous actual causality.
pub fn token_creations(
    append: &RulesJournal,
    entry: StackEntryId,
    instruction: usize,
) -> Result<Option<Vec<ObjectId>>, String> {
    let mut matched = None;
    for record in &append.records {
        let RulesRecord::Event(event) = record else {
            continue;
        };
        if !matches!(event.cause, EventCause::Instruction { entry: actual, instruction: index } if actual == entry && index == instruction)
        {
            continue;
        }
        if matched.is_some() {
            return Err("nonunique actual token creating event".into());
        }
        let mut objects = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for occurrence in &event.occurrences {
            if let RulesOccurrence::TokenCreated { after, .. } = &occurrence.value {
                let object = after.object.id();
                if !seen.insert(object) {
                    return Err("duplicate actual token birth identity".into());
                }
                objects.push(object);
            }
        }
        matched = Some(objects);
    }
    Ok(matched)
}
/// Select a required zero-based position from an actual nonmissing creating event.
/// Missing events remain unbound; present out-of-range coordinates reject.
pub fn token_creation_at(
    append: &RulesJournal,
    entry: StackEntryId,
    instruction: usize,
    ordinal: usize,
) -> Result<Option<ObjectId>, String> {
    token_creations(append, entry, instruction)?
        .map(|objects| {
            objects
                .get(ordinal)
                .copied()
                .ok_or_else(|| "actual token birth ordinal out of range".into())
        })
        .transpose()
}
/// Singleton convenience. Plural receipts must never silently
/// choose the first object; callers needing a batch use an explicit ordinal.
pub fn token_creation(
    append: &RulesJournal,
    entry: StackEntryId,
    instruction: usize,
) -> Result<Option<ObjectId>, String> {
    match token_creations(append, entry, instruction)? {
        None => Ok(None),
        Some(objects) if objects.is_empty() => Ok(None),
        Some(objects) if objects.len() == 1 => Ok(objects.first().copied()),
        Some(_) => Err("nonunique actual token birth receipt".into()),
    }
}
/// Birth discovery relates to the stored AFTER appearance, independently of
/// physical movement/discard selectors that relate to their BEFORE appearance.
pub fn birth_discovery(
    append: &RulesJournal,
    source: ObjectId,
    ability: u64,
    related_after: ObjectId,
) -> Result<Option<TriggerInstanceId>, String> {
    discovery(
        append,
        source,
        ability,
        "nonunique actual token birth discovery",
        |related| {
            Ok(
                matches!(related, TriggerRelated::TokenCreated { after, .. } if after.object.id() == related_after),
            )
        },
    )
}

/// Select an actual discovered instance by any member of its complete causes.
/// Multiple matching members in one instance are one match; distinct matching
/// instances and duplicate matching discovery records reject. Caller-specific
/// relation and required-stop policies stay
/// with their native recipe adapters.
///
/// Returns the supplied ambiguity error for nonunique matching records (including
/// a duplicated instance record), and propagates
/// errors from the caller's relation predicate without manufacturing a binding.
pub fn discovery(
    append: &RulesJournal,
    source: ObjectId,
    ability: u64,
    ambiguity: &str,
    mut matches: impl FnMut(&TriggerRelated) -> Result<bool, String>,
) -> Result<Option<TriggerInstanceId>, String> {
    let mut found = None;
    for record in &append.records {
        let RulesRecord::TriggerDiscovered(waiting) = record else {
            continue;
        };
        if waiting.granting_effect.is_some()
            || waiting.source.object.id() != source
            || waiting.definition.0 != ability
        {
            continue;
        }
        for member in waiting.causes.members() {
            if matches(&member.related)? {
                if found.is_some() {
                    return Err(ambiguity.into());
                }
                found = Some(waiting.id);
                break;
            }
        }
    }
    Ok(found)
}
