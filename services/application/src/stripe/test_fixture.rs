//! Synthetic provider wire profiles shared by HTTP client and application route tests.
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;
pub(crate) fn price() -> Value {
    json!({"id":"price_synthetic","object":"price","active":true,"livemode":false,"currency":"usd","unit_amount":500,"billing_scheme":"per_unit","type":"one_time","recurring":null,"custom_unit_amount":null,"transform_quantity":null})
}
pub(crate) fn checkout(id: Uuid) -> Value {
    json!({"id":"cs_test_synthetic","object":"checkout.session","status":"open","payment_status":"unpaid","mode":"payment","livemode":false,"payment_intent":null,"client_reference_id":id,"currency":"usd","amount_total":500,"url":"https://checkout.stripe.com/c/pay/synthetic","expires_at":(Utc::now()+chrono::Duration::hours(1)).timestamp(),"metadata":{"spellwake_purchase_id":id},"customer_details":{"email":"PRIVATE_BILLING_CANARY"}})
}
