//! Server-only one-use exchange. User tokens are temporary identity proofs, never Spellwake login methods.
use super::{
    PatreonError,
    association::ServerTerms,
    client::{PatreonClient, PatreonConfig, UserAccessToken},
    credential,
};
use crate::{
    http_transport::bounded_body,
    provider_json::{Object, integer, string},
};
use reqwest::Method;
use serde::Deserialize;

/// Explicit private credentials, with no Debug/Serialize implementation or constructor network access.
#[derive(Clone)]
pub(crate) struct OAuthConfig {
    pub(super) source: PatreonConfig,
    pub(super) terms: ServerTerms,
    client_secret: String,
}
impl OAuthConfig {
    pub fn from_env() -> Result<Self, PatreonError> {
        let read = |name| std::env::var(name).map_err(|_| PatreonError::Configuration);
        Self::new(
            read("SPELLWAKE_PATREON_CAMPAIGN_ID")?,
            read("SPELLWAKE_PATREON_CREATOR_ACCESS_TOKEN")?,
            read("SPELLWAKE_PATREON_CLIENT_ID")?,
            read("SPELLWAKE_PATREON_CLIENT_SECRET")?,
            read("SPELLWAKE_PATREON_REDIRECT_URI")?,
        )
    }
    pub fn new(
        campaign_id: String,
        creator_token: String,
        client_id: String,
        client_secret: String,
        redirect_uri: String,
    ) -> Result<Self, PatreonError> {
        let source = PatreonConfig::new(campaign_id.clone(), creator_token)?;
        let terms = ServerTerms {
            version: 1,
            campaign_id,
            client_id,
            redirect_uri,
        };
        terms.validate()?;
        if !credential(&client_secret) {
            return Err(PatreonError::Configuration);
        }
        Ok(Self {
            source,
            terms,
            client_secret,
        })
    }
    pub fn matches(&self, terms: &ServerTerms) -> bool {
        self.terms == *terms
    }
    pub(crate) fn source(&self) -> &PatreonConfig {
        &self.source
    }
    #[cfg(test)]
    pub(crate) fn fixture(base: &str) -> Self {
        Self {
            source: PatreonConfig::fixture(base),
            terms: ServerTerms {
                version: 1,
                campaign_id: "123".into(),
                client_id: "client_test".into(),
                redirect_uri: "https://spellwake.example.invalid/auth/patreon".into(),
            },
            client_secret: "patreon_test_client_secret".into(),
        }
    }
}
impl PatreonClient {
    /// Dispatch exactly once after the coordinator has a confirmed first-claim permission.
    pub(super) fn exchange(
        &self,
        config: &OAuthConfig,
        frozen: &ServerTerms,
        code: &str,
    ) -> Result<UserAccessToken, PatreonError> {
        if !config.matches(frozen) || !credential(code) {
            return Err(PatreonError::Contract);
        }
        let url = self
            .http
            .url("api/oauth2/token")
            .map_err(|_| PatreonError::Configuration)?;
        let response = self
            .http
            .request(Method::POST, url)
            .map_err(|_| PatreonError::Configuration)?
            .header("Accept", "application/json")
            .form(&[
                ("code", code),
                ("grant_type", "authorization_code"),
                ("client_id", frozen.client_id.as_str()),
                ("client_secret", config.client_secret.as_str()),
                ("redirect_uri", frozen.redirect_uri.as_str()),
            ])
            .send()?;
        if response.status().as_u16() != 200 {
            return Err(PatreonError::Status(response.status().as_u16()));
        }
        let body = bounded_body(response, 65536).map_err(PatreonError::Body)?;
        #[derive(Deserialize)]
        struct Token {
            #[serde(deserialize_with = "string")]
            access_token: String,
            #[serde(deserialize_with = "string")]
            token_type: String,
            #[serde(deserialize_with = "string")]
            scope: String,
            #[serde(deserialize_with = "integer")]
            expires_in: i64,
        }
        let token = serde_json::from_slice::<Object<Token>>(&body)
            .map_err(PatreonError::Json)?
            .0;
        if token.token_type != "Bearer"
            || token.expires_in <= 0
            || token.scope.len() > 4096
            || !token
                .scope
                .split_ascii_whitespace()
                .any(|scope| scope == "identity")
        {
            return Err(PatreonError::Contract);
        }
        UserAccessToken::new(token.access_token)
    }
    pub(super) fn linked_identity(
        &self,
        user: &UserAccessToken,
    ) -> Result<super::source::LinkedIdentity, PatreonError> {
        let body = self.get(
            "api/oauth2/v2/identity",
            &[
                ("include", "memberships.campaign"),
                ("fields[user]", "memberships"),
                ("fields[member]", "campaign,user"),
            ],
            user.value(),
        )?;
        super::source::parse_linked_identity(&body, &self.config.campaign_id)
    }
}
