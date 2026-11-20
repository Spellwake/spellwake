//! Fixed-origin, credential-separated Patreon reads. No account binding or entitlement writer.
pub use super::source::{
    CampaignMemberPage, CurrentMemberObservation, MemberObservation, PaymentStatus, PledgeKind,
    PledgeObservation,
};
use super::{
    PatreonError, credential, identifier,
    source::{cursor, parse_member, parse_page},
};
use crate::{
    http_transport::{HttpTransport, Origin, bounded_body, standard_client},
    provider_json::{Object, string},
};
use reqwest::{
    Method,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::Deserialize;
use std::time::Duration;
const ORIGIN: &str = "https://www.patreon.com";
const MEMBER_FIELDS: &str =
    "patron_status,last_charge_status,last_charge_date,campaign,user,currently_entitled_tiers";
const MEMBER_QUERY: &[(&str, &str)] = &[
    ("include", "campaign,user,currently_entitled_tiers"),
    ("fields[member]", MEMBER_FIELDS),
];
const HISTORY_FIELDS: &str = "amount_cents,currency_code,date,payment_status,pledge_payment_status,tier_id,type,campaign,patron,tier";
/// Explicit server-selected campaign/creator credential. Never serialize, debug or put in a client bundle.
#[derive(Clone)]
pub struct PatreonConfig {
    pub(super) campaign_id: String,
    pub(super) creator_token: String,
    base: String,
}
impl PatreonConfig {
    /// Read only explicitly configured Patreon server credentials; no request is dispatched.
    /// # Errors
    /// Rejects missing or malformed values.
    pub fn from_env() -> Result<Self, PatreonError> {
        let read = |name| std::env::var(name).map_err(|_| PatreonError::Configuration);
        Self::new(
            read("SPELLWAKE_PATREON_CAMPAIGN_ID")?,
            read("SPELLWAKE_PATREON_CREATOR_ACCESS_TOKEN")?,
        )
    }
    /// Bind the official HTTPS origin. The caller cannot supply a redirect or alternate host.
    /// # Errors
    /// Rejects malformed campaign identity or bearer token.
    pub fn new(campaign_id: String, creator_token: String) -> Result<Self, PatreonError> {
        if !identifier(&campaign_id) || !credential(&creator_token) {
            return Err(PatreonError::Configuration);
        }
        Ok(Self {
            campaign_id,
            creator_token,
            base: ORIGIN.into(),
        })
    }
    #[cfg(test)]
    pub(crate) fn fixture(base: &str) -> Self {
        Self {
            campaign_id: "123".into(),
            creator_token: "patreon_test_creator_synthetic".into(),
            base: base.into(),
        }
    }
}
/// Temporary user-authorized token. Distinct from the campaign creator credential and Supabase JWT.
pub struct UserAccessToken(String);
impl UserAccessToken {
    pub(super) fn value(&self) -> &str {
        &self.0
    }
    /// Accept a token returned by the eventual server-side OAuth exchange.
    /// # Errors
    /// Rejects malformed tokens; construction alone does not verify an identity.
    pub fn new(value: String) -> Result<Self, PatreonError> {
        if !credential(&value) {
            return Err(PatreonError::Configuration);
        }
        Ok(Self(value))
    }
}
/// Identity returned by the HTTPS identity endpoint under the temporary user's token.
/// This is not yet a Spellwake association or a second Spellwake sign-in method.
pub struct PatreonIdentity {
    user_id: String,
    body_sha256: String,
}
impl PatreonIdentity {
    pub fn user_id(&self) -> &str {
        &self.user_id
    }
    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }
}
/// Blocking service facade. Call from a bounded blocking worker, never an async runtime thread.
pub struct PatreonClient {
    pub(super) config: PatreonConfig,
    pub(super) http: HttpTransport,
}
impl PatreonClient {
    /// Build the fixed-origin client with redirects/retries disabled and a bounded timeout.
    /// # Errors
    /// Returns Configuration if client construction fails. Does not contact Patreon.
    pub fn new(config: PatreonConfig) -> Result<Self, PatreonError> {
        let http = standard_client(
            HeaderMap::new(),
            "Spellwake - Membership Sync",
            Duration::from_secs(30),
        )?;
        Ok(Self {
            http: HttpTransport::from_client(
                Origin::new(&config.base).map_err(|_| PatreonError::Configuration)?,
                http,
            ),
            config,
        })
    }
    pub(super) fn get(
        &self,
        path: &str,
        query: &[(&str, &str)],
        token: &str,
    ) -> Result<Vec<u8>, PatreonError> {
        let mut auth = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| PatreonError::Configuration)?;
        auth.set_sensitive(true);
        let url = self
            .http
            .url(path)
            .map_err(|_| PatreonError::Configuration)?;
        let response = self
            .http
            .request(Method::GET, url)
            .map_err(|_| PatreonError::Configuration)?
            .query(query)
            .header(AUTHORIZATION, auth)
            .header("Accept", "application/json")
            .send()?;
        if response.status().as_u16() != 200 {
            return Err(PatreonError::Status(response.status().as_u16()));
        }
        bounded_body(response, 1024 * 1024).map_err(PatreonError::Body)
    }
    /// Fetch the identity of the authorized user without email, name, address or linked sign-in data.
    /// # Errors
    /// Rejects HTTP/shape failures or a missing/wrong-type user identity.
    pub fn identity(&self, user: &UserAccessToken) -> Result<PatreonIdentity, PatreonError> {
        #[derive(Deserialize)]
        struct Envelope {
            data: Object<Identity>,
        }
        #[derive(Deserialize)]
        struct Identity {
            #[serde(rename = "type", deserialize_with = "string")]
            kind: String,
            #[serde(deserialize_with = "string")]
            id: String,
        }
        let body = self.get("api/oauth2/v2/identity", &[], &user.0)?;
        let data = serde_json::from_slice::<Object<Envelope>>(&body)
            .map_err(PatreonError::Json)?
            .0
            .data
            .0;
        if data.kind != "user" || !identifier(&data.id) {
            return Err(PatreonError::Contract);
        }
        Ok(PatreonIdentity {
            user_id: data.id,
            body_sha256: crate::provider_json::fingerprint(&body),
        })
    }
    /// Fetch one member using the creator token and verify campaign/user/member references.
    /// # Errors
    /// Rejects malformed/mismatched identities, unavailable/incomplete source shape and transport failures.
    /// Current entitlement does not prove a successful payment or authorize permanent cosmetics.
    pub fn member(
        &self,
        member_id: &str,
        expected_user: &str,
    ) -> Result<CurrentMemberObservation, PatreonError> {
        if !identifier(member_id) || !identifier(expected_user) {
            return Err(PatreonError::Contract);
        }
        let body = self.get(
            &format!("api/oauth2/v2/members/{member_id}"),
            MEMBER_QUERY,
            &self.config.creator_token,
        )?;
        parse_member(&body, member_id, expected_user, &self.config.campaign_id)
    }
    /// Read one campaign-member page from the endpoint that advertises pledge history.
    /// # Errors
    /// Rejects malformed cursors, missing linked observations, repeated cursor, wrong lineage and HTTP failures.
    /// The consumer must persist/deduplicate traversal, detect longer cursor cycles and retain incomplete state.
    /// Cursor exhaustion does not prove exhaustive successful-charge/refund coverage.
    pub fn campaign_members(
        &self,
        after: Option<&str>,
    ) -> Result<CampaignMemberPage, PatreonError> {
        if after.is_some_and(|value| !cursor(value)) {
            return Err(PatreonError::Contract);
        }
        let fields = format!("{MEMBER_FIELDS},pledge_history");
        let mut query = vec![
            (
                "include",
                "campaign,user,currently_entitled_tiers,pledge_history",
            ),
            ("fields[member]", fields.as_str()),
            ("fields[pledge-event]", HISTORY_FIELDS),
            ("page[count]", "25"),
        ];
        if let Some(after) = after {
            query.push(("page[cursor]", after));
        }
        let body = self.get(
            &format!(
                "api/oauth2/v2/campaigns/{}/members",
                self.config.campaign_id
            ),
            &query,
            &self.config.creator_token,
        )?;
        parse_page(&body, &self.config.campaign_id, after)
    }
}
