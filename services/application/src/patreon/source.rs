//! Selected source projection only. These observations do not authorize grants or prove exhaustive payment history.
use super::{One, PatreonError, Reference, identifier};
use crate::provider_json::{Array, Object, integer, optional_string, string};
use chrono::{DateTime, Datelike, Utc};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::collections::{BTreeMap, BTreeSet};

/// Literal provider payment state, independent of current tier entitlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentStatus {
    Paid,
    Declined,
    Deleted,
    Pending,
    Refunded,
    Fraud,
    RefundedByPatreon,
    Other,
    PartiallyRefunded,
    FreeTrial,
    RefundPending,
    RefundDeclined,
}
impl PaymentStatus {
    fn parse(value: &str) -> Result<Self, PatreonError> {
        Ok(match value {
            "Paid" => Self::Paid,
            "Declined" => Self::Declined,
            "Deleted" => Self::Deleted,
            "Pending" => Self::Pending,
            "Refunded" => Self::Refunded,
            "Fraud" => Self::Fraud,
            "Refunded by Patreon" => Self::RefundedByPatreon,
            "Other" => Self::Other,
            "Partially Refunded" => Self::PartiallyRefunded,
            "Free Trial" => Self::FreeTrial,
            "Refund Pending" => Self::RefundPending,
            "Refund Declined" => Self::RefundDeclined,
            _ => return Err(PatreonError::Contract),
        })
    }
}
/// A pledge action and a successful underlying charge are separate source facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PledgeKind {
    Start,
    Upgrade,
    Downgrade,
    Delete,
    Subscription,
}
/// Source observation, not a constructed application payment or grant proof.
/// No Debug or Serialize: provider identity is not public telemetry or an API payload.
pub struct PledgeObservation {
    pub event_id: String,
    pub tier_id: String,
    pub date: DateTime<Utc>,
    pub amount_cents: i64,
    pub currency_code: String,
    pub payment_status: PaymentStatus,
    pub pledge_payment_status: String,
    pub kind: PledgeKind,
}
/// Current membership only; it does not imply any successful historical payment.
pub struct CurrentMemberObservation {
    pub member_id: String,
    pub user_id: String,
    pub campaign_id: String,
    pub entitled_tier_ids: Vec<String>,
    pub patron_status: Option<String>,
    pub last_charge_status: Option<String>,
    pub last_charge_date: Option<DateTime<Utc>>,
    pub body_sha256: String,
}
/// Related pledge observations from a campaign-list response, separate from current entitlement.
pub struct MemberObservation {
    pub current: CurrentMemberObservation,
    pub pledge_history: Vec<PledgeObservation>,
}
/// One bounded source page, never a claim of exhaustive payment/refund history.
pub struct CampaignMemberPage {
    pub members: Vec<MemberObservation>,
    pub next_cursor: Option<String>,
    pub body_sha256: String,
}
#[derive(Deserialize)]
struct MemberEnvelope {
    data: Object<Member>,
}
#[derive(Deserialize)]
struct PageEnvelope {
    data: Array<Object<Member>>,
    #[serde(default)]
    included: Array<Object<Included>>,
    meta: Object<PageMeta>,
    #[serde(default)]
    links: Option<Object<PageLinks>>,
}
#[derive(Deserialize)]
struct PageMeta {
    pagination: Object<Pagination>,
}
#[derive(Deserialize)]
struct Pagination {
    #[serde(default)]
    cursors: Option<Object<Cursors>>,
}
#[derive(Deserialize)]
struct Cursors {
    #[serde(default, deserialize_with = "optional_string")]
    next: Option<String>,
}
#[derive(Deserialize)]
struct PageLinks {
    #[serde(default, deserialize_with = "optional_string")]
    next: Option<String>,
}
#[derive(Deserialize)]
struct Member {
    #[serde(rename = "type", deserialize_with = "string")]
    kind: String,
    #[serde(deserialize_with = "string")]
    id: String,
    attributes: Object<MemberAttributes>,
    relationships: Object<Relationships>,
}
#[derive(Deserialize)]
struct MemberAttributes {
    #[serde(deserialize_with = "optional_string")]
    patron_status: Option<String>,
    #[serde(deserialize_with = "optional_string")]
    last_charge_status: Option<String>,
    #[serde(deserialize_with = "optional_string")]
    last_charge_date: Option<String>,
}
#[derive(Deserialize)]
struct Relationships {
    campaign: Object<One>,
    user: Object<One>,
    currently_entitled_tiers: Object<Many>,
    #[serde(default)]
    pledge_history: Option<Box<RawValue>>,
}
#[derive(Deserialize)]
struct Many {
    data: Array<Object<Reference>>,
}
#[derive(Deserialize)]
struct Included {
    #[serde(rename = "type", deserialize_with = "string")]
    kind: String,
    #[serde(deserialize_with = "string")]
    id: String,
    #[serde(default)]
    attributes: Option<Box<RawValue>>,
    #[serde(default)]
    relationships: Option<Box<RawValue>>,
}
#[derive(Deserialize)]
struct PledgeAttributes {
    #[serde(deserialize_with = "integer")]
    amount_cents: i64,
    #[serde(deserialize_with = "string")]
    currency_code: String,
    #[serde(deserialize_with = "string")]
    date: String,
    #[serde(deserialize_with = "string")]
    payment_status: String,
    #[serde(deserialize_with = "string")]
    pledge_payment_status: String,
    #[serde(deserialize_with = "string")]
    tier_id: String,
    #[serde(rename = "type", deserialize_with = "string")]
    kind: String,
}
#[derive(Deserialize)]
struct PledgeRelationships {
    campaign: Object<One>,
    patron: Object<One>,
    tier: Object<One>,
}
fn instant(raw: &str) -> Result<DateTime<Utc>, PatreonError> {
    let value = DateTime::parse_from_rfc3339(raw)
        .map_err(|_| PatreonError::Contract)?
        .to_utc();
    if !(1..=9999).contains(&value.year()) || value.timestamp_subsec_nanos() % 1000 != 0 {
        return Err(PatreonError::Contract);
    }
    Ok(value)
}
fn references(
    values: Vec<Object<Reference>>,
    kind: &str,
) -> Result<BTreeSet<String>, PatreonError> {
    let mut seen = BTreeSet::new();
    for Object(reference) in values {
        if !reference.matches(kind) || !seen.insert(reference.id) {
            return Err(PatreonError::Contract);
        }
    }
    Ok(seen)
}
fn current(
    data: &Member,
    campaign: &str,
    body_sha256: &str,
) -> Result<CurrentMemberObservation, PatreonError> {
    let refs = &data.relationships.0;
    let attributes = &data.attributes.0;
    if data.kind != "member"
        || !identifier(&data.id)
        || !refs.user.0.data.0.matches("user")
        || !refs.campaign.0.data.0.matches("campaign")
        || refs.campaign.0.data.0.id != campaign
        || attributes.patron_status.as_ref().is_some_and(|v| {
            !matches!(
                v.as_str(),
                "active_patron" | "declined_patron" | "former_patron"
            )
        })
        || attributes.last_charge_status.as_ref().is_some_and(|v| {
            v.is_empty() || v.len() > 64 || !v.bytes().all(|b| b.is_ascii_alphabetic() || b == b' ')
        })
    {
        return Err(PatreonError::Contract);
    }
    let mut tiers = BTreeSet::new();
    for Object(tier) in &refs.currently_entitled_tiers.0.data.0 {
        if !tier.matches("tier") || !tiers.insert(tier.id.clone()) {
            return Err(PatreonError::Contract);
        }
    }
    Ok(CurrentMemberObservation {
        member_id: data.id.clone(),
        user_id: refs.user.0.data.0.id.clone(),
        campaign_id: campaign.into(),
        entitled_tier_ids: tiers.into_iter().collect(),
        patron_status: attributes.patron_status.clone(),
        last_charge_status: attributes.last_charge_status.clone(),
        last_charge_date: attributes
            .last_charge_date
            .as_deref()
            .map(instant)
            .transpose()?,
        body_sha256: body_sha256.into(),
    })
}
pub(super) fn parse_member(
    body: &[u8],
    member: &str,
    user: &str,
    campaign: &str,
) -> Result<CurrentMemberObservation, PatreonError> {
    let data = serde_json::from_slice::<Object<MemberEnvelope>>(body)
        .map_err(PatreonError::Json)?
        .0
        .data
        .0;
    let selected = current(&data, campaign, &crate::provider_json::fingerprint(body))?;
    if selected.member_id != member || selected.user_id != user {
        return Err(PatreonError::Contract);
    }
    Ok(selected)
}
pub(super) fn cursor(raw: &str) -> bool {
    !raw.is_empty() && raw.len() <= 1024 && raw.bytes().all(|b| b.is_ascii_graphic())
}
pub(super) fn parse_page(
    body: &[u8],
    campaign: &str,
    requested_cursor: Option<&str>,
) -> Result<CampaignMemberPage, PatreonError> {
    let envelope = serde_json::from_slice::<Object<PageEnvelope>>(body)
        .map_err(PatreonError::Json)?
        .0;
    if envelope.data.0.len() > 25 {
        return Err(PatreonError::Contract);
    }
    let next = envelope.meta.0.pagination.0.cursors.and_then(|v| v.0.next);
    if next
        .as_deref()
        .is_some_and(|v| !cursor(v) || Some(v) == requested_cursor)
        || (next.is_none() && envelope.links.and_then(|v| v.0.next).is_some())
    {
        return Err(PatreonError::Contract);
    }
    let mut included = BTreeMap::new();
    for Object(resource) in envelope.included.0 {
        if !identifier(&resource.id)
            || resource.kind.is_empty()
            || resource.kind.len() > 64
            || included
                .insert((resource.kind.clone(), resource.id.clone()), resource)
                .is_some()
        {
            return Err(PatreonError::Contract);
        }
    }
    let hash = crate::provider_json::fingerprint(body);
    let mut seen = BTreeSet::new();
    let mut members = Vec::new();
    for Object(data) in envelope.data.0 {
        if !seen.insert(data.id.clone()) {
            return Err(PatreonError::Contract);
        }
        let selected = current(&data, campaign, &hash)?;
        let raw_history = data
            .relationships
            .0
            .pledge_history
            .ok_or(PatreonError::Contract)?;
        let Object(refs) =
            serde_json::from_str::<Object<Many>>(raw_history.get()).map_err(PatreonError::Json)?;
        let history_ids = references(refs.data.0, "pledge-event")?;
        let mut history = Vec::new();
        for id in history_ids {
            let resource = included
                .get(&("pledge-event".into(), id.clone()))
                .ok_or(PatreonError::Contract)?;
            let Object(a) = serde_json::from_str::<Object<PledgeAttributes>>(
                resource
                    .attributes
                    .as_ref()
                    .ok_or(PatreonError::Contract)?
                    .get(),
            )
            .map_err(PatreonError::Json)?;
            let Object(r) = serde_json::from_str::<Object<PledgeRelationships>>(
                resource
                    .relationships
                    .as_ref()
                    .ok_or(PatreonError::Contract)?
                    .get(),
            )
            .map_err(PatreonError::Json)?;
            if !r.campaign.0.data.0.matches("campaign")
                || r.campaign.0.data.0.id != campaign
                || !r.patron.0.data.0.matches("user")
                || r.patron.0.data.0.id != selected.user_id
                || !r.tier.0.data.0.matches("tier")
                || r.tier.0.data.0.id != a.tier_id
                || a.currency_code.len() != 3
                || !a.currency_code.bytes().all(|b| b.is_ascii_uppercase())
                || !matches!(
                    a.pledge_payment_status.as_str(),
                    "queued" | "pending" | "valid" | "declined" | "fraud" | "disabled"
                )
            {
                return Err(PatreonError::Contract);
            }
            let kind = match a.kind.as_str() {
                "pledge_start" => PledgeKind::Start,
                "pledge_upgrade" => PledgeKind::Upgrade,
                "pledge_downgrade" => PledgeKind::Downgrade,
                "pledge_delete" => PledgeKind::Delete,
                "subscription" => PledgeKind::Subscription,
                _ => return Err(PatreonError::Contract),
            };
            history.push(PledgeObservation {
                event_id: id,
                tier_id: a.tier_id,
                date: instant(&a.date)?,
                amount_cents: a.amount_cents,
                currency_code: a.currency_code,
                payment_status: PaymentStatus::parse(&a.payment_status)?,
                pledge_payment_status: a.pledge_payment_status,
                kind,
            });
        }
        members.push(MemberObservation {
            current: selected,
            pledge_history: history,
        });
    }
    Ok(CampaignMemberPage {
        members,
        next_cursor: next,
        body_sha256: hash,
    })
}

/// User-token discovery. Construction is private to complete, typed identity response parsing.
pub(super) struct LinkedIdentity {
    user_id: String,
    member_id: Option<String>,
    body_sha256: String,
}
impl LinkedIdentity {
    pub fn user_id(&self) -> &str {
        &self.user_id
    }
    pub fn member_id(&self) -> Option<&str> {
        self.member_id.as_deref()
    }
    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }
}
pub(super) fn parse_linked_identity(
    body: &[u8],
    campaign: &str,
) -> Result<LinkedIdentity, PatreonError> {
    #[derive(Deserialize)]
    struct IdentityEnvelope {
        data: Object<IdentityUser>,
        #[serde(default)]
        included: Array<Object<Included>>,
    }
    #[derive(Deserialize)]
    struct IdentityUser {
        #[serde(rename = "type", deserialize_with = "string")]
        kind: String,
        #[serde(deserialize_with = "string")]
        id: String,
        relationships: Object<IdentityRelationships>,
    }
    #[derive(Deserialize)]
    struct IdentityRelationships {
        memberships: Object<Many>,
    }
    let envelope = serde_json::from_slice::<Object<IdentityEnvelope>>(body)
        .map_err(PatreonError::Json)?
        .0;
    let user = envelope.data.0;
    if user.kind != "user" || !identifier(&user.id) {
        return Err(PatreonError::Contract);
    }
    let ids = references(user.relationships.0.memberships.0.data.0, "member")?;
    let mut resources = BTreeMap::new();
    for Object(resource) in envelope.included.0 {
        if !identifier(&resource.id)
            || resource.kind.is_empty()
            || resource.kind.len() > 64
            || resources
                .insert((resource.kind.clone(), resource.id.clone()), resource)
                .is_some()
        {
            return Err(PatreonError::Contract);
        }
    }
    let mut selected = None;
    for id in ids {
        let resource = resources
            .get(&("member".into(), id.clone()))
            .ok_or(PatreonError::Contract)?;
        let Object(refs) = serde_json::from_str::<Object<super::MemberRelationships>>(
            resource
                .relationships
                .as_ref()
                .ok_or(PatreonError::Contract)?
                .get(),
        )
        .map_err(PatreonError::Json)?;
        if !refs.user.0.data.0.matches("user")
            || refs.user.0.data.0.id != user.id
            || !refs.campaign.0.data.0.matches("campaign")
        {
            return Err(PatreonError::Contract);
        }
        if refs.campaign.0.data.0.id == campaign && selected.replace(id).is_some() {
            return Err(PatreonError::Contract);
        }
    }
    Ok(LinkedIdentity {
        user_id: user.id,
        member_id: selected,
        body_sha256: crate::provider_json::fingerprint(body),
    })
}
