//! Immutable, source-bound creature-type membership.
//!
//! Construction proves shape and membership, not completeness against a rules source.
//! Only a separately source-qualified content publication establishes that claim.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const IDENTITY_PREFIX: &[u8] = b"spellwake.creature-types.v1\0";
const MAX_MEMBERS: usize = 65_536;

/// A syntactically validated vocabulary digest. Parsing does not prove membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CreatureVocabularyId([u8; 32]);

/// A malformed canonical lowercase hexadecimal vocabulary identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCreatureVocabularyId;

impl fmt::Display for InvalidCreatureVocabularyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected exactly 64 lowercase ASCII hexadecimal characters")
    }
}
impl std::error::Error for InvalidCreatureVocabularyId {}

impl TryFrom<String> for CreatureVocabularyId {
    type Error = InvalidCreatureVocabularyId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(InvalidCreatureVocabularyId);
        }
        let mut bytes = [0; 32];
        for (destination, pair) in bytes.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
            let nibble = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                _ => byte - b'a' + 10,
            };
            *destination = (nibble(pair[0]) << 4) | nibble(pair[1]);
        }
        Ok(Self(bytes))
    }
}

impl CreatureVocabularyId {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
impl fmt::Display for CreatureVocabularyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}
impl From<CreatureVocabularyId> for String {
    fn from(value: CreatureVocabularyId) -> Self {
        value.to_string()
    }
}

/// An actual member issued by an immutable vocabulary, never by raw deserialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CreatureTypeId {
    vocabulary: CreatureVocabularyId,
    ordinal: u16,
}
impl CreatureTypeId {
    pub fn vocabulary(self) -> CreatureVocabularyId {
        self.vocabulary
    }

    pub fn ordinal(self) -> u16 {
        self.ordinal
    }
}

/// Invalid vocabulary metadata, member shape or binary framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CreatureVocabularyError {
    InvalidRulesRevision,
    EmptyVocabulary,
    InvalidLabel,
    UnsortedMembers,
    DuplicateMember,
    TooManyMembers,
    EncodingLengthOverflow,
}
impl fmt::Display for CreatureVocabularyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRulesRevision => "invalid rules revision",
            Self::EmptyVocabulary => "empty creature-type vocabulary",
            Self::InvalidLabel => "invalid creature-type label",
            Self::UnsortedMembers => "creature-type members are not in UTF-8 byte order",
            Self::DuplicateMember => "duplicate creature-type member",
            Self::TooManyMembers => "too many creature-type members for u16 ordinals",
            Self::EncodingLengthOverflow => "creature-type framing length exceeds u32",
        })
    }
}
impl std::error::Error for CreatureVocabularyError {}

/// Immutable sorted membership and its source-bound content identity.
/// Equal values compare equally, including behind independently allocated `Arc`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureTypeVocabulary {
    id: CreatureVocabularyId,
    rules_revision: String,
    rules_sha256: [u8; 32],
    members: Vec<String>,
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn checked_member_count(count: usize) -> Result<u32, CreatureVocabularyError> {
    if count == 0 {
        return Err(CreatureVocabularyError::EmptyVocabulary);
    }
    if count > MAX_MEMBERS {
        return Err(CreatureVocabularyError::TooManyMembers);
    }
    checked_encoding_length(count)
}

fn checked_member_ordinal(index: usize) -> Result<u16, CreatureVocabularyError> {
    u16::try_from(index).map_err(|_| CreatureVocabularyError::TooManyMembers)
}

fn checked_encoding_length(length: usize) -> Result<u32, CreatureVocabularyError> {
    u32::try_from(length).map_err(|_| CreatureVocabularyError::EncodingLengthOverflow)
}

fn identity_framing(
    rules_revision: &str,
    rules_sha256: &[u8; 32],
    members: &[String],
) -> Result<Vec<u8>, CreatureVocabularyError> {
    let count = checked_member_count(members.len())?;
    let mut framed = IDENTITY_PREFIX.to_vec();
    framed.extend_from_slice(rules_sha256);
    framed.extend_from_slice(&checked_encoding_length(rules_revision.len())?.to_be_bytes());
    framed.extend_from_slice(rules_revision.as_bytes());
    framed.extend_from_slice(&count.to_be_bytes());
    for (index, member) in members.iter().enumerate() {
        checked_member_ordinal(index)?;
        framed.extend_from_slice(&checked_encoding_length(member.len())?.to_be_bytes());
        framed.extend_from_slice(member.as_bytes());
    }
    Ok(framed)
}

impl CreatureTypeVocabulary {
    /// Validate shape and construct the exact domain-separated identity.
    /// Members must already be canonical and sorted; nothing is normalized or deduplicated.
    pub fn new(
        rules_revision: String,
        rules_sha256: [u8; 32],
        members: Vec<String>,
    ) -> Result<Self, CreatureVocabularyError> {
        if !valid_text(&rules_revision) {
            return Err(CreatureVocabularyError::InvalidRulesRevision);
        }
        checked_member_count(members.len())?;
        if members.iter().any(|member| !valid_text(member)) {
            return Err(CreatureVocabularyError::InvalidLabel);
        }
        if members.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(CreatureVocabularyError::DuplicateMember);
        }
        if members.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(CreatureVocabularyError::UnsortedMembers);
        }
        let framing = identity_framing(&rules_revision, &rules_sha256, &members)?;
        let id = CreatureVocabularyId(Sha256::digest(framing).into());
        Ok(Self {
            id,
            rules_revision,
            rules_sha256,
            members,
        })
    }

    pub fn id(&self) -> CreatureVocabularyId {
        self.id
    }

    pub fn rules_revision(&self) -> &str {
        &self.rules_revision
    }

    pub fn rules_sha256(&self) -> &[u8; 32] {
        &self.rules_sha256
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn members(&self) -> &[String] {
        &self.members
    }

    pub fn resolve_label(&self, label: &str) -> Option<CreatureTypeId> {
        let index = self
            .members
            .binary_search_by(|member| member.as_str().cmp(label))
            .ok()?;
        self.resolve_member(self.id, checked_member_ordinal(index).ok()?)
    }

    /// Resolve a boundary pair against this actual vocabulary.
    pub fn resolve_member(
        &self,
        vocabulary: CreatureVocabularyId,
        ordinal: u16,
    ) -> Option<CreatureTypeId> {
        if vocabulary != self.id || usize::from(ordinal) >= self.members.len() {
            return None;
        }
        Some(CreatureTypeId {
            vocabulary,
            ordinal,
        })
    }

    pub fn label(&self, member: CreatureTypeId) -> Option<&str> {
        self.resolve_member(member.vocabulary, member.ordinal)?;
        self.members
            .get(usize::from(member.ordinal))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod contract_cases;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn vocabulary() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "synthetic".into(),
            [7; 32],
            vec!["Elf".into(), "Time Lord".into()],
        )
        .unwrap()
    }

    // Independent preimplementation source/identity vector. Test-only input, never a default.
    fn full_vector() -> (Vec<u8>, String, [u8; 32], Vec<String>) {
        let bytes = include_bytes!("../tests/fixtures/creature-vocabulary-identity.v1.bin");
        let mut cursor = IDENTITY_PREFIX.len();
        assert_eq!(&bytes[..cursor], IDENTITY_PREFIX);
        let source: [u8; 32] = bytes[cursor..cursor + 32].try_into().unwrap();
        cursor += 32;
        let read_string = |cursor: &mut usize| {
            let length =
                u32::from_be_bytes(bytes[*cursor..*cursor + 4].try_into().unwrap()) as usize;
            *cursor += 4;
            let value = std::str::from_utf8(&bytes[*cursor..*cursor + length])
                .unwrap()
                .to_owned();
            *cursor += length;
            value
        };
        let revision = read_string(&mut cursor);
        let count = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;
        let members = (0..count).map(|_| read_string(&mut cursor)).collect();
        assert_eq!(cursor, bytes.len());
        (bytes.to_vec(), revision, source, members)
    }

    #[test]
    fn matches_full_3423_byte_independent_identity_vector_and_exact_membership() {
        let (bytes, revision, source, members) = full_vector();
        assert_eq!(bytes.len(), 3423);
        assert_eq!(
            identity_framing(&revision, &source, &members).unwrap(),
            bytes
        );
        let full = CreatureTypeVocabulary::new(revision, source, members).unwrap();
        assert_eq!(
            full.id().to_string(),
            "39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792"
        );
        assert_eq!(full.len(), 324);
        for (label, ordinal) in [
            ("Advisor", 0),
            ("Child", 44),
            ("C’tan", 58),
            ("Elf", 85),
            ("Goblin", 109),
            ("Shi’ar", 254),
            ("Time Lord", 291),
            ("Wall", 306),
            ("Zubera", 323),
        ] {
            let member = full.resolve_member(full.id(), ordinal).unwrap();
            assert_eq!(full.label(member), Some(label));
            assert_eq!(full.resolve_label(label), Some(member));
        }
        for ordinal in [324, u16::MAX] {
            assert_eq!(full.resolve_member(full.id(), ordinal), None);
        }
        for raw in ["C'tan", "Shi'ar", "c’tan", "Time  Lord"] {
            assert_eq!(full.resolve_label(raw), None);
        }
    }

    #[test]
    fn full_shape_controls_do_not_claim_source_completeness() {
        let (_, revision, source, members) = full_vector();
        let full = CreatureTypeVocabulary::new(revision.clone(), source, members.clone()).unwrap();
        let mut controls = Vec::new();
        let mut removed = members.clone();
        removed.retain(|label| label != "Child");
        controls.push((revision.clone(), source, removed));
        let mut extra = members.clone();
        extra.push("ForgeOnlySynthetic".into());
        extra.sort();
        controls.push((revision.clone(), source, extra));
        let mut renamed = members.clone();
        *renamed
            .iter_mut()
            .find(|label| label.as_str() == "C’tan")
            .unwrap() = "C'tan".into();
        renamed.sort();
        controls.push((revision.clone(), source, renamed));
        controls.push((revision, [0; 32], members.clone()));
        controls.push(("SYNTHETIC-SHAPE-REV2".into(), source, members.clone()));
        for (revision, source, labels) in controls {
            let changed = CreatureTypeVocabulary::new(revision, source, labels).unwrap();
            assert_ne!(changed.id(), full.id());
        }
        assert_eq!(Arc::new(full.clone()), Arc::new(full));
        let mut unsorted = members;
        unsorted.swap(0, 1);
        assert_eq!(
            CreatureTypeVocabulary::new("CR-20260925".into(), source, unsorted),
            Err(CreatureVocabularyError::UnsortedMembers)
        );
    }

    #[test]
    fn identity_is_canonical_hex_and_not_membership_authority() {
        let id = vocabulary().id();
        assert_eq!(CreatureVocabularyId::try_from(id.to_string()), Ok(id));
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{id}\""));
        for text in [
            "".into(),
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            format!(" {}", "a".repeat(64)),
            "é".repeat(32),
        ] {
            assert_eq!(
                CreatureVocabularyId::try_from(text),
                Err(InvalidCreatureVocabularyId)
            );
        }
        assert!(serde_json::from_str::<CreatureVocabularyId>("42").is_err());
    }

    #[test]
    fn lookup_issues_only_actual_exact_members_and_rejects_foreign_members() {
        let local = vocabulary();
        let foreign =
            CreatureTypeVocabulary::new("other".into(), [7; 32], local.members().to_vec()).unwrap();
        let elf = local.resolve_label("Elf").unwrap();
        assert_eq!(elf.ordinal(), 0);
        assert_eq!(elf.vocabulary(), local.id());
        assert_eq!(local.label(elf), Some("Elf"));
        assert_eq!(
            local.resolve_member(local.id(), 1),
            local.resolve_label("Time Lord")
        );
        assert_eq!(local.resolve_member(local.id(), 2), None);
        assert_eq!(local.resolve_member(local.id(), u16::MAX), None);
        assert_eq!(local.resolve_member(foreign.id(), 0), None);
        assert_eq!(local.label(foreign.resolve_label("Elf").unwrap()), None);
        for label in ["elf", " Elf", "Elf ", "Time  Lord", "Time", "Lord"] {
            assert_eq!(local.resolve_label(label), None);
        }
    }

    #[test]
    fn validates_shape_without_silently_sorting_or_normalizing() {
        let make = |revision: &str, members: Vec<&str>| {
            CreatureTypeVocabulary::new(
                revision.into(),
                [0; 32],
                members.into_iter().map(String::from).collect(),
            )
        };
        for revision in ["", " rules", "rules ", "rules\n", "ru\0les"] {
            assert_eq!(
                make(revision, vec!["Elf"]),
                Err(CreatureVocabularyError::InvalidRulesRevision)
            );
        }
        assert_eq!(
            make("rules", vec![]),
            Err(CreatureVocabularyError::EmptyVocabulary)
        );
        for label in ["", " Elf", "Elf ", "Elf\n", "El\0f"] {
            assert_eq!(
                make("rules", vec![label]),
                Err(CreatureVocabularyError::InvalidLabel)
            );
        }
        assert_eq!(
            make("rules", vec!["Time Lord", "Elf"]),
            Err(CreatureVocabularyError::UnsortedMembers)
        );
        assert_eq!(
            make("rules", vec!["Elf", "Elf"]),
            Err(CreatureVocabularyError::DuplicateMember)
        );
        assert_eq!(
            make("rules", vec!["Time Lord", "Elf", "Elf"]),
            Err(CreatureVocabularyError::DuplicateMember)
        );
        assert!(
            make(
                "rules",
                vec!["C'tan", "C’tan", "Shi'ar", "Shi’ar", "Time Lord"]
            )
            .is_ok()
        );
    }

    #[test]
    fn immutable_allocations_compare_by_value_and_source_changes_identity() {
        let first = Arc::new(vocabulary());
        let second = Arc::new(vocabulary());
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(first, second);
        let source = CreatureTypeVocabulary::new(
            first.rules_revision().into(),
            [8; 32],
            first.members().to_vec(),
        )
        .unwrap();
        assert_ne!(first.id(), source.id());
    }

    #[test]
    fn exact_framing_is_binary_length_prefixed() {
        let members = vec!["Elf".into(), "Time Lord".into()];
        let actual = identity_framing("R", &[3; 32], &members).unwrap();
        let mut expected = b"spellwake.creature-types.v1\0".to_vec();
        expected.extend_from_slice(&[3; 32]);
        expected.extend_from_slice(b"\0\0\0\x01R\0\0\0\x02\0\0\0\x03Elf\0\0\0\x09Time Lord");
        assert_eq!(actual, expected);
    }

    #[test]
    fn checked_metadata_boundaries_do_not_require_impossible_allocations() {
        assert_eq!(checked_member_ordinal(65_535), Ok(u16::MAX));
        assert_eq!(
            checked_member_ordinal(65_536),
            Err(CreatureVocabularyError::TooManyMembers)
        );
        assert_eq!(
            checked_member_count(0),
            Err(CreatureVocabularyError::EmptyVocabulary)
        );
        assert_eq!(checked_member_count(65_536), Ok(65_536_u32));
        assert_eq!(
            checked_member_count(65_537),
            Err(CreatureVocabularyError::TooManyMembers)
        );
        let maximum = checked_encoding_length(u32::MAX as usize).unwrap();
        assert_eq!(maximum.to_be_bytes(), [255; 4]);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn rejects_64_bit_framing_length_overflow() {
        assert_eq!(
            checked_encoding_length(4_294_967_296),
            Err(CreatureVocabularyError::EncodingLengthOverflow)
        );
    }

    #[test]
    fn maximum_actual_vocabulary_issues_last_ordinal_without_truncation() {
        let members: Vec<_> = (0..65_536).map(|index| format!("Type{index:05}")).collect();
        let maximum =
            CreatureTypeVocabulary::new("synthetic".into(), [0; 32], members.clone()).unwrap();
        assert_eq!(
            maximum.resolve_label("Type65535").unwrap().ordinal(),
            u16::MAX
        );
        let mut oversized = members;
        oversized.push("Type65536".into());
        assert_eq!(
            CreatureTypeVocabulary::new("synthetic".into(), [0; 32], oversized),
            Err(CreatureVocabularyError::TooManyMembers)
        );
    }
}
