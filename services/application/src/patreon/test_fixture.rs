//! Synthetic native/API compatibility and selected source protocol fixtures.
use serde_json::Value;
pub(crate) fn source() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/patreon-source.json")).unwrap()
}
pub(crate) fn association() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/patreon-association.json"
    ))
    .unwrap()
}
pub(crate) fn member_path() -> String {
    let mut url = reqwest::Url::parse("http://127.0.0.1/api/oauth2/v2/members/member-abc").unwrap();
    url.query_pairs_mut().append_pair("include","campaign,user,currently_entitled_tiers")
        .append_pair("fields[member]","patron_status,last_charge_status,last_charge_date,campaign,user,currently_entitled_tiers");
    format!("{}?{}", url.path(), url.query().unwrap())
}
pub(crate) fn identity_path() -> String {
    let mut url = reqwest::Url::parse("http://127.0.0.1/api/oauth2/v2/identity").unwrap();
    url.query_pairs_mut()
        .append_pair("include", "memberships.campaign")
        .append_pair("fields[user]", "memberships")
        .append_pair("fields[member]", "campaign,user");
    format!("{}?{}", url.path(), url.query().unwrap())
}
