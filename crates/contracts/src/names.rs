//! Exact English name forms and a supplied immutable interchangeable-name relation.
//! Constructors validate representation, not Oracle membership or completeness.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// Malformed name forms, sets or interchangeable-name classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameDefinitionError {
    InvalidLabel,
    DuplicateName,
    TooSmallClass,
    OverlappingClasses,
    EncodingLengthOverflow,
}
impl fmt::Display for NameDefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLabel => "invalid English name label",
            Self::DuplicateName => "duplicate name form",
            Self::TooSmallClass => "interchangeable class needs at least two names",
            Self::OverlappingClasses => "overlapping interchangeable classes",
            Self::EncodingLengthOverflow => "name relation encoding length overflow",
        })
    }
}
impl std::error::Error for NameDefinitionError {}

/// An exact qualified English form, distinct from localization or cosmetic title.
/// Structural validation does not prove that a form belongs to Oracle.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EnglishCardName(String);
impl EnglishCardName {
    /// Preserve exact Unicode/case/punctuation, including protected separators.
    ///
    /// # Errors
    /// Rejects empty, edge-whitespace or control-containing labels.
    pub fn new(label: String) -> Result<Self, NameDefinitionError> {
        if label.is_empty() || label.trim() != label || label.chars().any(char::is_control) {
            return Err(NameDefinitionError::InvalidLabel);
        }
        Ok(Self(label))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for EnglishCardName {
    type Error = NameDefinitionError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<EnglishCardName> for String {
    fn from(value: EnglishCardName) -> Self {
        value.0
    }
}
impl fmt::Display for EnglishCardName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Exact forms held by one object. Empty means nameless, not an empty name label.
/// Equality is structural; use NameRules for gameplay name comparisons.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "Vec<EnglishCardName>", into = "Vec<EnglishCardName>")]
pub struct ObjectNames(BTreeSet<EnglishCardName>);
impl ObjectNames {
    pub fn nameless() -> Self {
        Self(BTreeSet::new())
    }
    pub fn single(name: EnglishCardName) -> Self {
        Self(BTreeSet::from([name]))
    }
    /// Canonicalize iteration order while retaining each actual selected form.
    ///
    /// # Errors
    /// Rejects repeated exact forms; different interchangeable forms remain distinct.
    pub fn new(names: Vec<EnglishCardName>) -> Result<Self, NameDefinitionError> {
        let mut result = BTreeSet::new();
        for name in names {
            if !result.insert(name) {
                return Err(NameDefinitionError::DuplicateName);
            }
        }
        Ok(Self(result))
    }
    pub fn iter(&self) -> impl Iterator<Item = &EnglishCardName> {
        self.0.iter()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Layer-3 addition is set union; adding a held form is idempotent.
    pub fn add(&mut self, names: &Self) {
        self.0.extend(names.0.iter().cloned());
    }
}
impl TryFrom<Vec<EnglishCardName>> for ObjectNames {
    type Error = NameDefinitionError;
    fn try_from(value: Vec<EnglishCardName>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<ObjectNames> for Vec<EnglishCardName> {
    fn from(value: ObjectNames) -> Self {
        value.0.into_iter().collect()
    }
}

/// Issued only by a validated relation value. Identity does not establish source truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(into = "String")]
pub struct NameRulesId([u8; 32]);
impl fmt::Display for NameRulesId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
impl From<NameRulesId> for String {
    fn from(value: NameRulesId) -> Self {
        value.to_string()
    }
}

/// One immutable supplied relation, not the Oracle name-choice domain.
/// The lookup index derives solely from the retained disjoint classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRules {
    id: NameRulesId,
    groups: Vec<ObjectNames>,
    representatives: BTreeMap<EnglishCardName, EnglishCardName>,
}
fn frame_length(hash: &mut Sha256, length: usize) -> Result<(), NameDefinitionError> {
    let length = u64::try_from(length).map_err(|_| NameDefinitionError::EncodingLengthOverflow)?;
    hash.update(length.to_be_bytes());
    Ok(())
}
impl NameRules {
    /// Explicit literal-name fixture policy. This makes no all-Magic alias claim.
    pub fn literal_names() -> Self {
        // Fixed empty input cannot fail class or length validation.
        Self::new(vec![]).expect("empty explicit name relation is valid")
    }
    /// Validate supplied complete classes and normalize their iteration order.
    ///
    /// # Errors
    /// Rejects classes smaller than two forms, overlapping classes or an encoding
    /// length that cannot be framed. Membership and source completeness are external.
    pub fn new(mut groups: Vec<ObjectNames>) -> Result<Self, NameDefinitionError> {
        groups.sort();
        let mut representatives = BTreeMap::new();
        let mut hash = Sha256::new();
        hash.update(b"spellwake.name-rules.v1\0");
        frame_length(&mut hash, groups.len())?;
        for group in &groups {
            if group.len() < 2 {
                return Err(NameDefinitionError::TooSmallClass);
            }
            let representative = group.iter().next().expect("validated nonempty name class");
            frame_length(&mut hash, group.len())?;
            for form in group.iter() {
                if representatives
                    .insert(form.clone(), representative.clone())
                    .is_some()
                {
                    return Err(NameDefinitionError::OverlappingClasses);
                }
                frame_length(&mut hash, form.as_str().len())?;
                hash.update(form.as_str().as_bytes());
            }
        }
        Ok(Self {
            id: NameRulesId(hash.finalize().into()),
            groups,
            representatives,
        })
    }
    pub fn id(&self) -> NameRulesId {
        self.id
    }
    pub fn groups(&self) -> &[ObjectNames] {
        &self.groups
    }
    fn canonical<'a>(&'a self, name: &'a EnglishCardName) -> &'a EnglishCardName {
        self.representatives.get(name).unwrap_or(name)
    }
    /// Pure, deduplicated comparison forms under this supplied relation.
    ///
    /// Iteration is deterministic. Interchangeable held forms contribute one
    /// representative; nameless objects contribute none. This does not change
    /// actual names or certify that the supplied relation covers all Magic names.
    pub fn comparison_forms<'a>(&'a self, names: &'a ObjectNames) -> BTreeSet<&'a EnglishCardName> {
        names.iter().map(|name| self.canonical(name)).collect()
    }
    /// Whether one held name matches this exact or explicitly interchangeable form.
    pub fn has_name(&self, names: &ObjectNames, name: &EnglishCardName) -> bool {
        let name = self.canonical(name);
        names.iter().any(|held| self.canonical(held) == name)
    }
    /// Binary CR 201.2a overlap. Nameless objects never share, even with themselves.
    pub fn shares_name(&self, left: &ObjectNames, right: &ObjectNames) -> bool {
        !self
            .comparison_forms(left)
            .is_disjoint(&self.comparison_forms(right))
    }
    /// CR 201.2a requires one name common to ALL supplied objects, not pairwise
    /// overlap. The signature requires at least two objects.
    pub fn common_name(
        &self,
        first: &ObjectNames,
        second: &ObjectNames,
        others: &[&ObjectNames],
    ) -> bool {
        let mut common = self.comparison_forms(first);
        for names in std::iter::once(second).chain(others.iter().copied()) {
            let next = self.comparison_forms(names);
            common.retain(|name| next.contains(name));
            if common.is_empty() {
                return false;
            }
        }
        !common.is_empty()
    }
    /// Named and pairwise disjoint (CR 201.2b). The empty constraint is true;
    /// a singleton passes only when named. Selection cardinality is separate.
    pub fn pairwise_different(&self, objects: &[&ObjectNames]) -> bool {
        let mut seen = BTreeSet::new();
        for names in objects {
            if names.is_empty() {
                return false;
            }
            for name in self.comparison_forms(names) {
                if !seen.insert(name) {
                    return false;
                }
            }
        }
        true
    }
    /// CR 201.2c: first is named and disjoint from every other object. Nameless
    /// others and an empty comparison group are allowed.
    pub fn different_from(&self, first: &ObjectNames, others: &[&ObjectNames]) -> bool {
        !first.is_empty() && others.iter().all(|other| !self.shares_name(first, other))
    }
}

#[cfg(test)]
mod tests;
