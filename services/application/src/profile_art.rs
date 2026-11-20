//! Catalogue crop references only; no image URL or loadability claim crosses this boundary.
use crate::account::PublicAccountIdentity;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
/// Stored IEEE754 float64 crop, paired with the PostgreSQL float8 normalizer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardArtProfile {
    pub printing_id: Uuid,
    pub crop_x: f64,
    pub crop_y: f64,
    pub scale: f64,
}
impl<'de> Deserialize<'de> for CardArtProfile {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            printing_id: Uuid,
            crop_x: f64,
            crop_y: f64,
            scale: f64,
        }
        let w: Wire = crate::provider_json::object(input)?;
        if w.printing_id.is_nil()
            || !w.crop_x.is_finite()
            || !(0.0..=1.0).contains(&w.crop_x)
            || !w.crop_y.is_finite()
            || !(0.0..=1.0).contains(&w.crop_y)
            || !w.scale.is_finite()
            || w.scale <= 0.0
        {
            return Err(serde::de::Error::custom("invalid catalogue art crop"));
        }
        Ok(Self {
            printing_id: w.printing_id,
            crop_x: w.crop_x,
            crop_y: w.crop_y,
            scale: w.scale,
        })
    }
}
/// A state-based compare-and-set edit. Both null fields are explicit, not omitted.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeProfileArt {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub expected: Option<CardArtProfile>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub card_art_profile: Option<CardArtProfile>,
}
impl ChangeProfileArt {
    /// # Errors
    /// Rejects an absent request identity; crops are validated during decoding.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil() {
            return Err("invalid profile request");
        }
        Ok(())
    }
    pub(crate) fn failure(&self, errors: &std::collections::BTreeMap<String, String>) -> bool {
        errors.len() == 1
            && errors.iter().all(|(k, v)| {
                matches!(
                    (k.as_str(), v.as_str()),
                    ("account", "account unavailable")
                        | ("profile", "profile art changed; reload before saving")
                        | ("art", "catalogue art unavailable")
                )
            })
    }
}
/// Caller-owned retained crop and active-member badge, without provider identity.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnProfileArt {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub card_art_profile: Option<CardArtProfile>,
    pub supporter: bool,
}
impl OwnProfileArt {
    /// # Errors
    /// Rejects a response bound to a different or absent caller.
    pub fn validate(&self, owner: Uuid) -> Result<(), &'static str> {
        if owner.is_nil() || self.account_id != owner {
            return Err("invalid profile owner");
        }
        Ok(())
    }
}
/// Existing directory/retained-social identity visibility, never a contact permission.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicProfileArt {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub viewer_account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub person: PublicAccountIdentity,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub card_art_profile: Option<CardArtProfile>,
    pub supporter: bool,
}
impl PublicProfileArt {
    /// # Errors
    /// Rejects foreign query context or an unmasked retired target.
    pub fn validate(&self, viewer: Uuid, target: Uuid) -> Result<(), &'static str> {
        self.person.validate()?;
        if viewer.is_nil()
            || target.is_nil()
            || self.viewer_account_id != viewer
            || self.person.id != target
            || (self.person.is_deleted && (self.card_art_profile.is_some() || self.supporter))
        {
            return Err("invalid public profile context");
        }
        Ok(())
    }
}
