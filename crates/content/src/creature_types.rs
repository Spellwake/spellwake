//! Pure publication of the complete creature-type declaration from a supported raw CR pin.
//! This is not a card compiler, subtype alias resolver or runtime choice normalizer.

use std::{collections::BTreeSet, fmt, io, path::Path};

use serde::{Serialize, ser::SerializeStruct};
use sha2::{Digest, Sha256};
use spellwake_contracts::{
    ApiResponse,
    creature_types::{CreatureTypeVocabulary, CreatureVocabularyError},
};

const RULES_REVISION: &str = "CR-20260925";
const RULES_SHA256: [u8; 32] = [
    0x8d, 0x86, 0x0e, 0x45, 0x1f, 0x20, 0xf3, 0x88, 0x65, 0xb7, 0x25, 0xb4, 0x2d, 0x82, 0xfe, 0xb7,
    0x14, 0xc7, 0x25, 0x37, 0x3d, 0xd8, 0xf3, 0xb3, 0x2b, 0x86, 0x52, 0xb3, 0xee, 0xb0, 0x70, 0xca,
];
const SECTION: &str = "205.3m";
const INTRODUCTION: &str = "205.3m Creatures and kindreds share their lists of subtypes; these subtypes are called creature types. ";
const MULTIWORD_PREFIX: &str = "One creature type is two words long: ";
const MULTIWORD_MEMBER: &str = "Time Lord";
const MULTIWORD_SUFFIX: &str = ". ";
const LIST_INTRODUCTION: &str = "All other creature types are one word long: ";

/// Explicit caller-declared source. Unsupported pairs never select a guessed parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureTypeSource {
    pub rules_revision: String,
    pub rules_sha256: [u8; 32],
}
impl Serialize for CreatureTypeSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("CreatureTypeSource", 2)?;
        record.serialize_field("rules_revision", &self.rules_revision)?;
        record.serialize_field("rules_sha256", &hex(&self.rules_sha256))?;
        record.end()
    }
}

/// Source-qualified vocabulary or explicit incompleteness without executable members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatureTypeCompilation {
    Complete(CreatureTypePublication),
    Incomplete(CreatureTypeIncomplete),
}
impl Serialize for CreatureTypeCompilation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("CreatureTypeCompilation", 2)?;
        match self {
            Self::Complete(publication) => {
                record.serialize_field("tag", "Complete")?;
                record.serialize_field("publication", publication)?;
            }
            Self::Incomplete(incomplete) => {
                record.serialize_field("tag", "Incomplete")?;
                record.serialize_field("incomplete", incomplete)?;
            }
        }
        record.end()
    }
}

/// Complete publication receipt and the actual immutable native vocabulary it describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreatureTypePublication {
    #[serde(flatten)]
    receipt: Box<PublicationReceipt>,
    #[serde(skip)]
    vocabulary: CreatureTypeVocabulary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PublicationReceipt {
    schema_version: u8,
    kind: &'static str,
    source: PublicationSource,
    section: SectionReceipt,
    members: Vec<PublishedMember>,
    identity: PublicationIdentity,
    provider_subtype_aliases: Vec<ProviderSubtypeAlias>,
}
impl CreatureTypePublication {
    pub fn vocabulary(&self) -> &CreatureTypeVocabulary {
        &self.vocabulary
    }

    pub fn into_vocabulary(self) -> CreatureTypeVocabulary {
        self.vocabulary
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PublicationSource {
    rules_revision: String,
    rules_sha256: String,
    section: &'static str,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SectionReceipt {
    physical_lf_line: String,
    source_byte_start: String,
    source_byte_end_exclusive: String,
    raw_line_utf8: String,
    raw_line_sha256: String,
    line_ending_hex: String,
    consumed_bytes: String,
    unconsumed_bytes: String,
    tokens: Vec<SourceToken>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PublishedMember {
    ordinal: String,
    canonical_label: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PublicationIdentity {
    algorithm: &'static str,
    framed_bytes: String,
    vocabulary_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ProviderSubtypeAlias {
    provider_raw: &'static str,
    canonical_label: &'static str,
    scope: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum TokenRole {
    Introduction,
    MultiwordSentencePrefix,
    MultiwordSentenceSuffix,
    ListIntroduction,
    Separator,
    FinalConjunction,
    FinalPeriod,
    Member,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SourceToken {
    role: TokenRole,
    line_byte_start: String,
    line_byte_end_exclusive: String,
    raw_utf8: String,
    member_ordinal: Option<String>,
}

/// Incompleteness has no partial vocabulary, members or claimed executable identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreatureTypeIncomplete {
    schema_version: u8,
    kind: &'static str,
    declared_source: CreatureTypeSource,
    reason: CreatureTypeIncompleteReason,
    diagnostics: Vec<CreatureTypeDiagnostic>,
}
impl CreatureTypeIncomplete {
    pub fn reason(&self) -> CreatureTypeIncompleteReason {
        self.reason
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CreatureTypeIncompleteReason {
    UnsupportedSource,
    MissingSection,
    AmbiguousSection,
    UnrecognizedSectionSyntax,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CreatureTypeDiagnosticCode {
    UnsupportedSource,
    MissingSection,
    AmbiguousSection,
    UnexpectedSentence,
    MalformedSeparator,
    MissingFinalConjunction,
    DuplicateMember,
    InvalidMemberToken,
    UnconsumedText,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreatureTypeDiagnostic {
    code: CreatureTypeDiagnosticCode,
    physical_lf_line: Option<String>,
    line_byte_start: Option<String>,
    line_byte_end_exclusive: Option<String>,
}

/// Raw-source, decoding or shared-contract failure. None is a gameplay result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatureTypeCompileError {
    SourceDigestMismatch {
        expected: [u8; 32],
        actual: [u8; 32],
    },
    InvalidUtf8 {
        valid_up_to: usize,
        error_len: Option<usize>,
    },
    Vocabulary(CreatureVocabularyError),
}
impl Serialize for CreatureTypeCompileError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::SourceDigestMismatch { expected, actual } => {
                let mut record = serializer.serialize_struct("CreatureTypeCompileError", 3)?;
                record.serialize_field("tag", "SourceDigestMismatch")?;
                record.serialize_field("expected", &hex(expected))?;
                record.serialize_field("actual", &hex(actual))?;
                record.end()
            }
            Self::InvalidUtf8 {
                valid_up_to,
                error_len,
            } => {
                let mut record = serializer.serialize_struct("CreatureTypeCompileError", 3)?;
                record.serialize_field("tag", "InvalidUtf8")?;
                record.serialize_field("valid_up_to", valid_up_to)?;
                record.serialize_field("error_len", error_len)?;
                record.end()
            }
            Self::Vocabulary(error) => {
                let mut record = serializer.serialize_struct("CreatureTypeCompileError", 2)?;
                record.serialize_field("tag", "Vocabulary")?;
                record.serialize_field("error", error)?;
                record.end()
            }
        }
    }
}
impl fmt::Display for CreatureTypeCompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceDigestMismatch { expected, actual } => write!(
                formatter,
                "creature-type source digest mismatch: expected {}, actual {}",
                hex(expected),
                hex(actual)
            ),
            Self::InvalidUtf8 {
                valid_up_to,
                error_len,
            } => write!(
                formatter,
                "invalid source UTF-8 at byte {valid_up_to} (length {error_len:?})"
            ),
            Self::Vocabulary(error) => fmt::Display::fmt(error, formatter),
        }
    }
}
impl std::error::Error for CreatureTypeCompileError {}
impl From<CreatureVocabularyError> for CreatureTypeCompileError {
    fn from(error: CreatureVocabularyError) -> Self {
        Self::Vocabulary(error)
    }
}

/// Operational failures outside the pure compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatureTypeCliError {
    Read { io_kind: io::ErrorKind },
    Serialization { io_kind: io::ErrorKind },
}
impl fmt::Display for CreatureTypeCliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { io_kind } => {
                write!(formatter, "creature-type source read failed: {io_kind}")
            }
            Self::Serialization { io_kind } => write!(
                formatter,
                "creature-type publication output failed: {io_kind}"
            ),
        }
    }
}
impl std::error::Error for CreatureTypeCliError {}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn incomplete(
    declared_source: CreatureTypeSource,
    failure: SectionFailure,
) -> CreatureTypeCompilation {
    CreatureTypeCompilation::Incomplete(CreatureTypeIncomplete {
        schema_version: 1,
        kind: "incomplete-creature-type-source",
        declared_source,
        reason: failure.reason,
        diagnostics: failure.diagnostics,
    })
}

/// Compile only a supported declaration pin, checking raw bytes before decoding or parsing.
/// No filesystem, network, fallback source or expected-result artifact is consulted.
pub fn compile(
    source_bytes: &[u8],
    declared: CreatureTypeSource,
) -> Result<CreatureTypeCompilation, CreatureTypeCompileError> {
    if declared.rules_revision != RULES_REVISION || declared.rules_sha256 != RULES_SHA256 {
        return Ok(incomplete(
            declared,
            SectionFailure::unlocated(
                CreatureTypeIncompleteReason::UnsupportedSource,
                CreatureTypeDiagnosticCode::UnsupportedSource,
            ),
        ));
    }
    let actual: [u8; 32] = Sha256::digest(source_bytes).into();
    if actual != declared.rules_sha256 {
        return Err(CreatureTypeCompileError::SourceDigestMismatch {
            expected: declared.rules_sha256,
            actual,
        });
    }
    let source = decode_source_utf8(source_bytes)?;
    let located = match locate_creature_section(source) {
        Ok(section) => section,
        Err(failure) => return Ok(incomplete(declared, failure)),
    };
    let mut parsed = match parse_creature_section(located.line) {
        Ok(parsed) => parsed,
        Err(mut failure) => {
            for diagnostic in &mut failure.diagnostics {
                diagnostic.physical_lf_line = Some(located.physical_lf_line.to_string());
            }
            return Ok(incomplete(declared, failure));
        }
    };
    parsed.members.sort();
    let vocabulary = CreatureTypeVocabulary::new(
        declared.rules_revision.clone(),
        declared.rules_sha256,
        parsed.members,
    )?;
    for token in &mut parsed.tokens {
        if token.role == TokenRole::Member {
            let member = vocabulary
                .resolve_label(&token.raw_utf8)
                .ok_or(CreatureVocabularyError::InvalidLabel)?;
            token.member_ordinal = Some(member.ordinal().to_string());
        }
    }
    let members = vocabulary
        .members()
        .iter()
        .enumerate()
        .map(|(ordinal, label)| PublishedMember {
            ordinal: ordinal.to_string(),
            canonical_label: label.clone(),
        })
        .collect();
    // These are framing sizes only; identity itself is computed exclusively by contracts.
    let framed_bytes = b"spellwake.creature-types.v1\0".len()
        + 32
        + 4
        + declared.rules_revision.len()
        + 4
        + vocabulary
            .members()
            .iter()
            .map(|member| 4 + member.len())
            .sum::<usize>();
    let receipt = PublicationReceipt {
        schema_version: 1,
        kind: "source-qualified-creature-type-publication",
        source: PublicationSource {
            rules_revision: declared.rules_revision,
            rules_sha256: hex(&declared.rules_sha256),
            section: SECTION,
        },
        section: SectionReceipt {
            physical_lf_line: located.physical_lf_line.to_string(),
            source_byte_start: located.source_byte_start.to_string(),
            source_byte_end_exclusive: (located.source_byte_start + located.line.len()).to_string(),
            raw_line_utf8: located.line.to_owned(),
            raw_line_sha256: hex(&Sha256::digest(located.line.as_bytes())),
            line_ending_hex: hex(located.line_ending.as_bytes()),
            consumed_bytes: located.line.len().to_string(),
            unconsumed_bytes: "0".into(),
            tokens: parsed.tokens,
        },
        members,
        identity: PublicationIdentity {
            algorithm: "spellwake.creature-types.v1",
            framed_bytes: framed_bytes.to_string(),
            vocabulary_id: vocabulary.id().to_string(),
        },
        provider_subtype_aliases: vec![
            ProviderSubtypeAlias {
                provider_raw: "C'tan",
                canonical_label: "C’tan",
                scope: "provider-subtype-field",
            },
            ProviderSubtypeAlias {
                provider_raw: "Shi'ar",
                canonical_label: "Shi’ar",
                scope: "provider-subtype-field",
            },
        ],
    };
    let publication = CreatureTypePublication {
        receipt: Box::new(receipt),
        vocabulary,
    };
    Ok(CreatureTypeCompilation::Complete(publication))
}

fn decode_source_utf8(bytes: &[u8]) -> Result<&str, CreatureTypeCompileError> {
    std::str::from_utf8(bytes).map_err(|error| CreatureTypeCompileError::InvalidUtf8 {
        valid_up_to: error.valid_up_to(),
        error_len: error.error_len(),
    })
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, Eq))]
struct LocatedSection<'a> {
    line: &'a str,
    line_ending: &'a str,
    physical_lf_line: usize,
    source_byte_start: usize,
}
#[derive(Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, Eq))]
struct SectionFailure {
    reason: CreatureTypeIncompleteReason,
    diagnostics: Vec<CreatureTypeDiagnostic>,
}
impl SectionFailure {
    fn unlocated(reason: CreatureTypeIncompleteReason, code: CreatureTypeDiagnosticCode) -> Self {
        Self {
            reason,
            diagnostics: vec![CreatureTypeDiagnostic {
                code,
                physical_lf_line: None,
                line_byte_start: None,
                line_byte_end_exclusive: None,
            }],
        }
    }

    fn syntax(code: CreatureTypeDiagnosticCode, start: usize, end: usize) -> Self {
        Self {
            reason: CreatureTypeIncompleteReason::UnrecognizedSectionSyntax,
            diagnostics: vec![CreatureTypeDiagnostic {
                code,
                physical_lf_line: None,
                line_byte_start: Some(start.to_string()),
                line_byte_end_exclusive: Some(end.to_string()),
            }],
        }
    }
}

fn locate_creature_section(source: &str) -> Result<LocatedSection<'_>, SectionFailure> {
    let mut found = None;
    let mut source_byte_start = 0;
    for (index, physical) in source.split_inclusive('\n').enumerate() {
        let (line, line_ending) = physical
            .strip_suffix('\n')
            .map_or((physical, ""), |line| (line, "\n"));
        if line.starts_with("205.3m ") {
            if found.is_some() {
                return Err(SectionFailure::unlocated(
                    CreatureTypeIncompleteReason::AmbiguousSection,
                    CreatureTypeDiagnosticCode::AmbiguousSection,
                ));
            }
            found = Some(LocatedSection {
                line,
                line_ending,
                physical_lf_line: index + 1,
                source_byte_start,
            });
        }
        source_byte_start += physical.len();
    }
    found.ok_or_else(|| {
        SectionFailure::unlocated(
            CreatureTypeIncompleteReason::MissingSection,
            CreatureTypeDiagnosticCode::MissingSection,
        )
    })
}

#[derive(Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, Eq))]
struct ParsedSection {
    members: Vec<String>,
    tokens: Vec<SourceToken>,
}

fn push_token(
    tokens: &mut Vec<SourceToken>,
    role: TokenRole,
    line: &str,
    start: usize,
    end: usize,
) {
    tokens.push(SourceToken {
        role,
        line_byte_start: start.to_string(),
        line_byte_end_exclusive: end.to_string(),
        raw_utf8: line[start..end].to_owned(),
        member_ordinal: None,
    });
}

fn parse_creature_section(line: &str) -> Result<ParsedSection, SectionFailure> {
    let mut cursor = 0;
    let mut tokens = Vec::new();
    for (role, expected) in [
        (TokenRole::Introduction, INTRODUCTION),
        (TokenRole::MultiwordSentencePrefix, MULTIWORD_PREFIX),
        (TokenRole::Member, MULTIWORD_MEMBER),
        (TokenRole::MultiwordSentenceSuffix, MULTIWORD_SUFFIX),
        (TokenRole::ListIntroduction, LIST_INTRODUCTION),
    ] {
        if !line[cursor..].starts_with(expected) {
            return Err(SectionFailure::syntax(
                CreatureTypeDiagnosticCode::UnexpectedSentence,
                cursor,
                line.len(),
            ));
        }
        push_token(&mut tokens, role, line, cursor, cursor + expected.len());
        cursor += expected.len();
    }
    let remainder = &line[cursor..];
    let period = remainder.find('.').ok_or_else(|| {
        SectionFailure::syntax(
            CreatureTypeDiagnosticCode::UnexpectedSentence,
            cursor,
            line.len(),
        )
    })?;
    if period + 1 != remainder.len() {
        return Err(SectionFailure::syntax(
            CreatureTypeDiagnosticCode::UnconsumedText,
            cursor + period + 1,
            line.len(),
        ));
    }
    let list = &remainder[..period];
    let (before_last, last) = list.rsplit_once(", and ").ok_or_else(|| {
        SectionFailure::syntax(
            CreatureTypeDiagnosticCode::MissingFinalConjunction,
            cursor,
            line.len() - 1,
        )
    })?;
    let mut members = vec![MULTIWORD_MEMBER.to_owned()];
    let mut seen = BTreeSet::from([MULTIWORD_MEMBER]);
    let mut remaining = before_last;
    loop {
        let (member, rest) = remaining
            .split_once(", ")
            .map_or((remaining, None), |(member, rest)| (member, Some(rest)));
        parse_member(line, member, cursor, &mut members, &mut seen, &mut tokens)?;
        cursor += member.len();
        match rest {
            Some(rest) => {
                push_token(&mut tokens, TokenRole::Separator, line, cursor, cursor + 2);
                cursor += 2;
                remaining = rest;
            }
            None => break,
        }
    }
    push_token(
        &mut tokens,
        TokenRole::FinalConjunction,
        line,
        cursor,
        cursor + 6,
    );
    cursor += 6;
    parse_member(line, last, cursor, &mut members, &mut seen, &mut tokens)?;
    cursor += last.len();
    push_token(
        &mut tokens,
        TokenRole::FinalPeriod,
        line,
        cursor,
        cursor + 1,
    );
    Ok(ParsedSection { members, tokens })
}

fn parse_member<'a>(
    line: &str,
    member: &'a str,
    cursor: usize,
    members: &mut Vec<String>,
    seen: &mut BTreeSet<&'a str>,
    tokens: &mut Vec<SourceToken>,
) -> Result<(), SectionFailure> {
    let end = cursor + member.len();
    if member.is_empty() || member.contains(',') {
        return Err(SectionFailure::syntax(
            CreatureTypeDiagnosticCode::MalformedSeparator,
            cursor,
            end,
        ));
    }
    if !member
        .chars()
        .all(|character| character.is_alphabetic() || matches!(character, '-' | '’'))
    {
        return Err(SectionFailure::syntax(
            CreatureTypeDiagnosticCode::InvalidMemberToken,
            cursor,
            end,
        ));
    }
    if !seen.insert(member) {
        return Err(SectionFailure::syntax(
            CreatureTypeDiagnosticCode::DuplicateMember,
            cursor,
            end,
        ));
    }
    members.push(member.to_owned());
    push_token(tokens, TokenRole::Member, line, cursor, end);
    Ok(())
}

/// Read the caller's explicit local source. Read failures precede pure compilation.
pub fn read_source(path: &Path) -> Result<Vec<u8>, CreatureTypeCliError> {
    std::fs::read(path).map_err(|error| CreatureTypeCliError::Read {
        io_kind: error.kind(),
    })
}

/// Serialize once to the provided sink. A failed sink is never retried or replaced.
pub fn write_output<T: Serialize>(
    mut writer: impl io::Write,
    response: &ApiResponse<T>,
) -> Result<(), CreatureTypeCliError> {
    serde_json::to_writer(&mut writer, response).map_err(|error| {
        CreatureTypeCliError::Serialization {
            io_kind: error.io_error_kind().unwrap_or(io::ErrorKind::Other),
        }
    })?;
    writer
        .flush()
        .map_err(|error| CreatureTypeCliError::Serialization {
            io_kind: error.kind(),
        })
}

#[cfg(all(test, feature = "private-rule-fixtures"))]
mod tests;

#[cfg(all(test, feature = "private-rule-fixtures"))]
mod contract_cases;
