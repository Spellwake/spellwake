//! Connected application RPC/provider orchestration. No vendor payload is logged.
use super::{
    StripeEventKind, VerifiedStripeNotification,
    client::{API_VERSION, ClientError, Purchase, StripeClient, StripeConfig},
};
use crate::{
    ApiResponse,
    actor::{ActorState, RequestActor},
    backend::{BackendClient, BackendConfig, BackendError},
    commands::CommandAck,
    private_storage::EvidenceCapture as Capture,
};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{error::Error, fmt, time::Duration};
use uuid::Uuid;
mod wire;
use super::notification::{AssociatedAck, CompletionAck, IgnoredAck, evidence_locator_matches};
use wire::{EffectResult, Notification};

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

pub(crate) enum ServiceError {
    Backend(BackendError),
    Provider(ClientError),
    Contract,
    Storage(crate::private_storage::StorageError),
}
impl fmt::Debug for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("stripe purchase work is temporarily unresolved")
    }
}
impl Error for ServiceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Backend(e) => Some(e),
            Self::Provider(e) => Some(e),
            Self::Contract => None,
            Self::Storage(e) => Some(e),
        }
    }
}
impl From<ClientError> for ServiceError {
    fn from(e: ClientError) -> Self {
        Self::Provider(e)
    }
}
impl From<BackendError> for ServiceError {
    fn from(e: BackendError) -> Self {
        Self::Backend(e)
    }
}
pub(crate) fn hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) struct StripeService {
    backend: BackendClient,
    config: StripeConfig,
}

/// Only canonical failed command receipts authorize replacing a client intent.
/// Call after the reservation RPC or its receipt read, never for preflight failures.
fn settled_reservation_response(
    response: ApiResponse<CommandAck>,
    request: Uuid,
    cosmetic: Uuid,
) -> ApiResponse<CommandAck> {
    if let ApiResponse::Fail { data } = &response {
        let reason = if data.len() == 1
            && data.get("cosmetic").map(String::as_str) == Some("unavailable")
        {
            Some("cosmetic_unavailable")
        } else if data.len() == 1 && data.get("account").map(String::as_str) == Some("unavailable")
        {
            Some("account_unavailable")
        } else {
            None
        };
        if let Some(reason) = reason {
            return ApiResponse::Fail {
                data: std::collections::BTreeMap::from([
                    ("request".into(), "terminal_rejection".into()),
                    ("requestId".into(), request.to_string()),
                    ("cosmeticId".into(), cosmetic.to_string()),
                    ("reason".into(), reason.into()),
                ]),
            };
        }
    }
    response
}
impl StripeService {
    pub fn from_env(stopped: Arc<AtomicBool>) -> Result<Self, ServiceError> {
        let mut config = StripeConfig::from_env()?;
        config.stopped = stopped;
        Self::new(BackendConfig::from_env()?, config)
    }
    pub fn new(backend: BackendConfig, config: StripeConfig) -> Result<Self, ServiceError> {
        Ok(Self {
            backend: BackendClient::new(
                backend,
                "spellwake-stripe-service",
                Duration::from_secs(60),
            )?,
            config,
        })
    }
    fn rpc<T: DeserializeOwned>(&self, name: &str, payload: Value) -> Result<T, ServiceError> {
        Ok(self.backend.rpc(name, &payload)?)
    }
    pub fn reserve(
        &self,
        actor: &RequestActor,
        request: Uuid,
        cosmetic: Uuid,
    ) -> Result<ApiResponse<CommandAck>, ServiceError> {
        let identity = json!({"p_subject":actor.subject(),"p_session":actor.session(),"p_request":request,"p_cosmetic":cosmetic});
        let prior: Option<ApiResponse<CommandAck>> =
            self.rpc("read_stripe_reservation", identity.clone())?;
        if let Some(response) = prior {
            return Ok(settled_reservation_response(response, request, cosmetic));
        }
        if actor.state() != ActorState::Active {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("account".into(), "unavailable".into())]),
            });
        }
        if !self.config.prices.contains_key(&cosmetic) {
            return Ok(ApiResponse::Fail {
                data: std::collections::BTreeMap::from([("cosmetic".into(), "unavailable".into())]),
            });
        }
        let client = StripeClient::new(self.config.clone())?;
        let terms = client.terms(cosmetic)?;
        let mut reserve = identity;
        reserve["p_terms"] = serde_json::to_value(terms).map_err(|_| ServiceError::Contract)?;
        let response: ApiResponse<CommandAck> = self.rpc("reserve_stripe_purchase", reserve)?;
        if let ApiResponse::Success { data } = &response {
            // Accepted intent survives an unknown external acknowledgment. The
            // worker revisits the SAME stored request key instead of recreating it.
            if self.create_with(&client, data.resource_id).is_err() {
                eprintln!("stripe event=creation_ack_unconfirmed");
            }
        }
        Ok(settled_reservation_response(response, request, cosmetic))
    }
    fn ticket(&self, id: Uuid) -> Result<Purchase, ServiceError> {
        self.backend
            .rpc_optional_object::<Purchase>("read_stripe_purchase", &json!({"p_purchase":id}))?
            .ok_or(ServiceError::Contract)
    }
    fn acknowledge(
        &self,
        ticket: &Purchase,
        checkout: &super::client::Checkout,
    ) -> Result<(), ServiceError> {
        let expires = DateTime::<Utc>::from_timestamp(checkout.expires_at, 0)
            .ok_or(ServiceError::Contract)?;
        let _:crate::provider_json::Object<Value>=self.rpc("ack_stripe_checkout",json!({"p_purchase":ticket.id,"p_checkout":checkout.id,"p_payment":checkout.payment_intent,
            "p_url":checkout.url,"p_expires":expires,"p_terms_hash":ticket.terms_hash}))?;
        Ok(())
    }
    fn create_with(&self, client: &StripeClient, id: Uuid) -> Result<(), ServiceError> {
        let Some(ticket) = self
            .backend
            .rpc_optional_object::<Purchase>("claim_stripe_creation", &json!({"p_purchase":id}))?
        else {
            return Ok(());
        };
        let checkout = client.create(&ticket)?;
        self.acknowledge(&ticket, &checkout)
    }
    pub fn capture(
        &self,
        notice: &VerifiedStripeNotification,
        received: DateTime<Utc>,
    ) -> Result<Capture, ServiceError> {
        let target = notice.signal().map(|(_, id)| id);
        self.backend.rpc_object("capture_stripe_event",&json!({"p_external":notice.event_id(),"p_type":notice.event_type(),"p_delivery":received,
            "p_capture":{"version":1,"kind":"webhook","bodyHash":notice.body_sha256(),"apiVersion":notice.api_version(),"live":notice.is_live(),"targetId":target,"createdAt":notice.created()}}))
            .map_err(ServiceError::Backend)
    }
    pub fn confirm_evidence(&self, event: Uuid, body_hash: &str) -> Result<(), ServiceError> {
        if !self.rpc::<bool>(
            "confirm_stripe_evidence",
            json!({"p_event":event,"p_hash":body_hash}),
        )? {
            return Err(ServiceError::Contract);
        }
        Ok(())
    }
    pub fn reconcile(&self, id: Uuid) -> Result<Value, ServiceError> {
        let client = StripeClient::new(self.config.clone())?;
        let mut ticket = self.ticket(id)?;
        if ticket.state == "creating" && ticket.checkout_ref.is_none() {
            if let Some(found) = client.find_checkout(&ticket)? {
                self.acknowledge(&ticket, &found)?;
                ticket = self.ticket(id)?;
            } else {
                self.create_with(&client, id)?;
                ticket = self.ticket(id)?;
            }
        }
        let Some(checkout_id) = ticket.checkout_ref.as_deref() else {
            return Ok(json!({"state":"unresolved"}));
        };
        let mut checkout = client.checkout(checkout_id)?;
        client.match_checkout(&checkout, &ticket)?;
        if ticket.account_state != "active" && checkout.status == "open" {
            checkout = client.expire(checkout_id, &format!("{}-expire", ticket.request_key))?;
        }
        let mut proof = json!({"version":1,"kind":"pending","effectKey":hash(&format!("pending:{id}")),"termsHash":ticket.terms_hash,
            "sourceObjectKey":hash(checkout_id),"checkoutKey":hash(checkout_id),"paymentKey":checkout.payment_intent.as_deref().map(hash)});
        if checkout.status == "expired"
            && checkout.payment_status == "unpaid"
            && checkout.payment_intent.is_none()
        {
            proof["kind"] = json!("canceled");
            proof["effectKey"] = json!(hash(&format!("expired:{checkout_id}")));
        }
        if let Some(payment_id) = &checkout.payment_intent {
            let payment = client.payment(payment_id)?;
            if payment.metadata.0.spellwake_purchase_id.as_deref() != Some(id.to_string().as_str())
                || payment.currency != ticket.terms.currency
                || payment.amount.to_string() != ticket.terms.minor_units
            {
                return Err(ServiceError::Contract);
            }
            if checkout.status == "expired"
                && checkout.payment_status == "unpaid"
                && payment.status == "canceled"
                && payment.amount_received == 0
            {
                proof["kind"] = json!("canceled");
            }
            if checkout.payment_status == "paid" && payment.status == "succeeded" {
                let charge = client.charge(
                    payment
                        .latest_charge
                        .as_deref()
                        .ok_or(ServiceError::Contract)?,
                )?;
                if charge.payment_intent.as_deref() != Some(payment_id)
                    || !charge.paid
                    || !charge.captured
                    || charge.amount != payment.amount
                    || charge.amount_captured != payment.amount
                    || payment.amount_received != payment.amount
                    || charge.currency != payment.currency
                {
                    return Err(ServiceError::Contract);
                }
                let refunds = client.refunds(payment_id)?;
                let mut refund_total = 0_i64;
                for refund in &refunds {
                    if refund.charge.as_deref() != Some(charge.id.as_str())
                        || refund.currency != payment.currency
                        || refund.amount < 0
                    {
                        return Err(ServiceError::Contract);
                    }
                    if refund.status == "succeeded" {
                        refund_total = refund_total
                            .checked_add(refund.amount)
                            .ok_or(ServiceError::Contract)?;
                    }
                }
                if refund_total != charge.amount_refunded || refund_total > payment.amount {
                    return Err(ServiceError::Contract);
                }
                proof["kind"] = json!("paid");
                proof["paid"] = json!(true);
                proof["amountMinor"] = json!(payment.amount.to_string());
                proof["currency"] = json!(payment.currency);
                proof["chargeKey"] = json!(hash(&charge.id));
                proof["effectKey"] = json!(hash(&format!("paid:{payment_id}")));
                if let Some(refund) = refunds
                    .iter()
                    .find(|r| r.status == "succeeded" && r.amount > 0)
                {
                    proof["kind"] = json!("refunded");
                    proof["movementKey"] = json!(hash(&refund.id));
                    proof["refundMinorUnits"] = json!(refund_total.to_string());
                    proof["effectKey"] = json!(hash(&format!("refund:{payment_id}:{}", refund.id)));
                } else {
                    if ticket.state == "refunded" {
                        for refund in &refunds {
                            if refund.status != "failed"
                                || ticket.reversal_key.as_deref() != Some(hash(&refund.id).as_str())
                            {
                                continue;
                            }
                            let Some(return_id) = &refund.failure_balance_transaction else {
                                continue;
                            };
                            let returned = client.refund_return(return_id)?;
                            // The refund's own reversal reference plus the retrieved source
                            // must identify this exact loss. Unrelated positive balances
                            // cannot resurrect an entitlement.
                            if returned.source.as_deref() != Some(refund.id.as_str())
                                || returned.kind != "refund_failure"
                                || returned.amount <= 0
                                || returned.currency != refund.currency
                                || returned.amount != refund.amount
                            {
                                return Err(ServiceError::Contract);
                            }
                            proof["kind"] = json!("restored");
                            proof["movementKey"] = json!(hash(return_id));
                            proof["reversalKey"] = json!(ticket.reversal_key);
                            proof["settledReversalKey"] = json!(ticket.reversal_key);
                            proof["reversalEventId"] = json!(ticket.reversal_event_id);
                            proof["refundMinorUnits"] = json!("0");
                            proof["fundsReturned"] = json!(true);
                        }
                    }
                    let disputes = client.disputes(payment_id)?;
                    let mut unresolved_loss = false;
                    for dispute in disputes {
                        if dispute.charge != charge.id {
                            return Err(ServiceError::Contract);
                        }
                        let negatives: Vec<_> = dispute
                            .balance_transactions
                            .0
                            .iter()
                            .map(|x| &x.0)
                            .filter(|x| x.amount < 0)
                            .collect();
                        let positives: Vec<_> = dispute
                            .balance_transactions
                            .0
                            .iter()
                            .map(|x| &x.0)
                            .filter(|x| x.amount > 0)
                            .collect();
                        for movement in dispute.balance_transactions.0.iter().map(|x| &x.0) {
                            if !super::provider_id(&movement.id, "txn_")
                                || movement.currency.len() != 3
                                || movement.created < 0
                            {
                                return Err(ServiceError::Contract);
                            }
                        }
                        if negatives.is_empty() {
                            continue;
                        }
                        if negatives.len() != 1 {
                            return Err(ServiceError::Contract);
                        }
                        let withdrawal = hash(&negatives[0].id);
                        if dispute.status == "won" && !positives.is_empty() {
                            if positives.len() != 1 {
                                return Err(ServiceError::Contract);
                            }
                            if positives[0].currency != negatives[0].currency
                                || negatives[0].amount.checked_neg() != Some(positives[0].amount)
                            {
                                return Err(ServiceError::Contract);
                            }
                            if unresolved_loss {
                                continue;
                            }
                            if matches!(ticket.state.as_str(), "refunded" | "reversed")
                                && ticket.reversal_key.as_deref() != Some(withdrawal.as_str())
                            {
                                continue;
                            }
                            proof["disputeKey"] = json!(hash(&dispute.id));
                            let credit = hash(&positives[0].id);
                            proof["kind"] = json!(if matches!(
                                ticket.state.as_str(),
                                "creating" | "pending" | "canceled"
                            ) {
                                "paid"
                            } else {
                                "restored"
                            });
                            proof["movementKey"] = json!(credit);
                            proof["settledReversalKey"] = json!(withdrawal);
                            proof["reversalKey"] = json!(withdrawal);
                            proof["reversalEventId"] = json!(ticket.reversal_event_id);
                            proof["refundMinorUnits"] = json!("0");
                            proof["fundsReturned"] = json!(true);
                            proof["effectKey"] =
                                json!(hash(&format!("restored:{payment_id}:{}", positives[0].id)));
                        } else {
                            if !positives.is_empty() {
                                return Err(ServiceError::Contract);
                            }
                            unresolved_loss = true;
                            let fields = proof.as_object_mut().ok_or(ServiceError::Contract)?;
                            for field in [
                                "settledReversalKey",
                                "reversalKey",
                                "reversalEventId",
                                "refundMinorUnits",
                                "fundsReturned",
                            ] {
                                fields.remove(field);
                            }
                            proof["disputeKey"] = json!(hash(&dispute.id));
                            proof["kind"] = json!("reversed");
                            proof["movementKey"] = json!(withdrawal);
                            proof["effectKey"] = json!(hash(&format!(
                                "withdrawal:{payment_id}:{}",
                                negatives[0].id
                            )));
                        }
                    }
                }
            }
        }
        let kind = proof["kind"].as_str().ok_or(ServiceError::Contract)?;
        let economic = match kind {
            "paid" => format!(
                "paid:{}",
                proof["paymentKey"].as_str().ok_or(ServiceError::Contract)?
            ),
            "refunded" | "reversed" | "restored" => format!(
                "{kind}:{}:{}",
                proof["paymentKey"].as_str().ok_or(ServiceError::Contract)?,
                proof["movementKey"]
                    .as_str()
                    .ok_or(ServiceError::Contract)?
            ),
            _ => format!(
                "{kind}:{}",
                proof["checkoutKey"]
                    .as_str()
                    .ok_or(ServiceError::Contract)?
            ),
        };
        proof["effectKey"] = json!(hash(&economic));
        let body = serde_json::to_string(&proof).map_err(|_| ServiceError::Contract)?;
        let capture_hash = hash(&format!("{}:{}:{body}", id, ticket.revision));
        let capture:Capture=self.backend.rpc_object("capture_stripe_event",&json!({"p_external":format!("api-read:{capture_hash}"),"p_type":"api:checkout_reconciliation","p_delivery":null,
            "p_capture":{"version":1,"kind":"api_read","bodyHash":capture_hash,"apiVersion":API_VERSION,"live":ticket.terms.live,"targetId":checkout_id,"purchaseRevision":ticket.revision}}))?;
        let result:EffectResult=self.backend.rpc_object("commit_stripe_effect",&json!({"p_purchase":id,"p_event":capture.id,"p_revision":ticket.revision,"p_effect":proof,"p_checkout":checkout.id,"p_payment":checkout.payment_intent}))?;
        if !result.matches(id, &ticket.revision) {
            // A superseded capture is not payment truth. Keep the native
            // notification pending until a fresh current read can commit.
            return Err(ServiceError::Contract);
        }
        Ok(json!({"observationId":capture.id,"effect":result}))
    }
    pub fn ingest(
        &self,
        notice: &VerifiedStripeNotification,
        body: &[u8],
    ) -> Result<(), ServiceError> {
        let actual: String = Sha256::digest(body)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if actual != notice.body_sha256() {
            return Err(ServiceError::Contract);
        }
        let capture = self.capture(notice, Utc::now())?;
        if capture.evidence_state == "pending" {
            if capture.expires_at > Utc::now() {
                crate::private_storage::retain_bytes(
                    &self.backend,
                    &self.config.evidence_bucket,
                    &capture.payload_ref,
                    body,
                )
                .map_err(ServiceError::Storage)?;
            }
            self.confirm_evidence(capture.id, notice.body_sha256())?;
            if capture.expires_at <= Utc::now() {
                // An upload already in flight can cross the retention boundary.
                // Delete it before acknowledging when this process survives.
                // The metadata-aware expiry sweep also recovers a crash here.
                self.expire_evidence(capture.id)?;
            }
        }
        if !capture.processed {
            if let Some((kind, target)) = notice.signal() {
                let attempt = self
                    .notification_purchase(kind, target)
                    .and_then(|id| {
                        self.associate_notification(capture.id, id)?;
                        self.reconcile(id).map(|result| (id, result))
                    })
                    .and_then(|(purchase, result)| {
                        let observation = result
                            .get("observationId")
                            .and_then(Value::as_str)
                            .ok_or(ServiceError::Contract)?;
                        let observation = crate::identifiers::parse(observation)
                            .map_err(|_| ServiceError::Contract)?;
                        self.complete_notification(capture.id, observation, purchase)
                    });
                if attempt.is_err() {
                    eprintln!("stripe event=notification_reconciliation_pending");
                }
            } else {
                self.ignore_notification(capture.id)?;
            }
        }
        Ok(())
    }
    pub fn notification_purchase(
        &self,
        kind: StripeEventKind,
        target: &str,
    ) -> Result<Uuid, ServiceError> {
        let client = StripeClient::new(self.config.clone())?;
        let payment = match kind {
            StripeEventKind::CheckoutCompleted
            | StripeEventKind::CheckoutAsyncSucceeded
            | StripeEventKind::CheckoutAsyncFailed
            | StripeEventKind::CheckoutExpired => {
                let checkout = client.checkout(target)?;
                let id = checkout
                    .metadata
                    .0
                    .spellwake_purchase_id
                    .as_deref()
                    .ok_or(ServiceError::Contract)?
                    .parse()
                    .map_err(|_| ServiceError::Contract)?;
                let ticket = self.ticket(id)?;
                client.match_checkout(&checkout, &ticket)?;
                if ticket.checkout_ref.is_none()
                    || (ticket.payment_ref.is_none() && checkout.payment_intent.is_some())
                {
                    self.acknowledge(&ticket, &checkout)?;
                }
                return Ok(id);
            }
            StripeEventKind::ChargeSucceeded
            | StripeEventKind::ChargeUpdated
            | StripeEventKind::ChargeRefunded => client
                .charge(target)?
                .payment_intent
                .ok_or(ServiceError::Contract)?,
            StripeEventKind::RefundCreated
            | StripeEventKind::RefundUpdated
            | StripeEventKind::RefundFailed => {
                let refund = client.refund(target)?;
                if let Some(payment) = refund.payment_intent {
                    payment
                } else {
                    client
                        .charge(refund.charge.as_deref().ok_or(ServiceError::Contract)?)?
                        .payment_intent
                        .ok_or(ServiceError::Contract)?
                }
            }
            _ => {
                let dispute = client.dispute(target)?;
                if let Some(payment) = dispute.payment_intent {
                    payment
                } else {
                    client
                        .charge(&dispute.charge)?
                        .payment_intent
                        .ok_or(ServiceError::Contract)?
                }
            }
        };
        client
            .payment(&payment)?
            .metadata
            .0
            .spellwake_purchase_id
            .as_deref()
            .ok_or(ServiceError::Contract)?
            .parse()
            .map_err(|_| ServiceError::Contract)
    }
    pub fn work(
        &self,
        kind: &str,
        after: Option<Uuid>,
    ) -> Result<crate::stripe_workflow::StripeBatch, ServiceError> {
        if self.config.stopped.load(Ordering::Acquire) {
            return Err(ServiceError::Provider(ClientError::Canceled));
        }
        let ids: Vec<Uuid> = self.rpc(
            "read_stripe_work",
            json!({"p_kind":kind,"p_after":after,"p_limit":1,"p_bucket":self.config.evidence_bucket}),
        )?;
        if ids.len() > 1
            || ids
                .iter()
                .any(|id| id.is_nil() || after.is_some_and(|a| *id <= a))
        {
            return Err(ServiceError::Contract);
        }
        let Some(id) = ids.first().copied() else {
            return Ok(crate::stripe_workflow::StripeBatch {
                cursor: None,
                completed: 0,
                tail: true,
            });
        };
        let result = match kind {
            "purchase" => self.reconcile(id).map(|_| ()),
            "event" => self.process_notification(id),
            "evidence" => self.expire_evidence(id),
            _ => return Err(ServiceError::Contract),
        };
        if self.config.stopped.load(Ordering::Acquire) {
            return Err(ServiceError::Provider(ClientError::Canceled));
        }
        if result.is_err() {
            eprintln!("stripe event=work_unresolved");
        }
        Ok(crate::stripe_workflow::StripeBatch {
            cursor: Some(id),
            completed: u8::from(result.is_ok()),
            tail: false,
        })
    }
    fn associate_notification(&self, event: Uuid, purchase: Uuid) -> Result<(), ServiceError> {
        let ack: AssociatedAck = self.backend.rpc_object(
            "associate_stripe_notification",
            &json!({"p_event":event,"p_purchase":purchase}),
        )?;
        if !ack.confirmed() {
            return Err(ServiceError::Contract);
        }
        Ok(())
    }
    fn ignore_notification(&self, event: Uuid) -> Result<(), ServiceError> {
        let ack: IgnoredAck = self
            .backend
            .rpc_object("ignore_stripe_notification", &json!({"p_event":event}))?;
        if !ack.confirmed() {
            return Err(ServiceError::Contract);
        }
        Ok(())
    }
    fn complete_notification(
        &self,
        event: Uuid,
        observation: Uuid,
        purchase: Uuid,
    ) -> Result<(), ServiceError> {
        let ack: CompletionAck = self.backend.rpc_object(
            "complete_stripe_notification",
            &json!({"p_event":event,"p_observation":observation}),
        )?;
        // SQL can return a historical original observation on retry. The purchase
        // is independently known; the fresh observation is not a replay fence.
        if !ack.matches_observed(purchase, None) {
            return Err(ServiceError::Contract);
        }
        Ok(())
    }
    fn process_notification(&self, id: Uuid) -> Result<(), ServiceError> {
        let Some(row) = self.backend.rpc_optional_object::<Notification>(
            "read_stripe_notification",
            &json!({"p_event":id}),
        )?
        else {
            return Ok(());
        };
        let event_type = &row.event_type;
        if let Some((kind, _, _)) = StripeEventKind::classify(event_type) {
            let target = row
                .capture
                .target_id
                .as_deref()
                .ok_or(ServiceError::Contract)?;
            let purchase = self.notification_purchase(kind, target)?;
            self.associate_notification(id, purchase)?;
            let observed = self.reconcile(purchase)?;
            let observation = observed
                .get("observationId")
                .and_then(Value::as_str)
                .and_then(|value| crate::identifiers::parse(value).ok())
                .ok_or(ServiceError::Contract)?;
            self.complete_notification(id, observation, purchase)?;
        } else {
            self.ignore_notification(id)?;
        }
        Ok(())
    }
    fn expire_evidence(&self, id: Uuid) -> Result<(), ServiceError> {
        let Some(row) = self
            .backend
            .rpc_optional_object::<crate::private_storage::EvidenceExpiry>(
                "begin_stripe_evidence_expiry",
                &json!({"p_event":id}),
            )?
        else {
            return Ok(());
        };
        if !evidence_locator_matches(&row.payload_ref, &row.body_hash) {
            return Err(ServiceError::Contract);
        }
        let key = &row.payload_ref;
        crate::private_storage::delete_payment_evidence(
            &self.backend,
            &self.config.evidence_bucket,
            key,
        )
        .map_err(ServiceError::Storage)?;
        self.backend
            .rpc_void("finish_stripe_evidence_expiry", &json!({"p_event":id}))?;
        Ok(())
    }
}
