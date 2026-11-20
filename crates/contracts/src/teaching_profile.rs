//! Shared identities for the finite, source-pinned teaching qualification profile.
//! These identify an internal pool; they do not certify cards or format coverage.

/// Exact compressed Oracle source admitted by this profile.
pub const TEACHING_SOURCE_SHA256: &str =
    "c1c9dc28fa9a7a4bae61c02330ba5a1190205aab1a93361e590ff5d9a146af08";
/// Registry report revision, independent of the default reference registry.
pub const TEACHING_REGISTRY_VERSION: u32 = 5;
/// Internal pool and program revision.
pub const TEACHING_PROFILE: &str = "teaching-vanilla-v1";
/// Actual source-qualified creature vocabulary required by admission and captures.
pub const TEACHING_VOCABULARY_ID: &str =
    "39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792";

/// Selected saved-Legacy optional-effect and resolution-payment pool.
/// These identities do not establish complete card or format qualification.
pub const OPTIONAL_PAYMENT_PROFILE: &str = "optional-payment-v1";
/// Coverage epoch for the explicit optional/payment pool, independent of Report4.
pub const OPTIONAL_PAYMENT_REGISTRY_VERSION: u32 = 8;
