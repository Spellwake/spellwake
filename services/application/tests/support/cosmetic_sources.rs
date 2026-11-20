//! Normalized SQL entitlement fixtures; not Stripe signature or settlement proof.
use tokio_postgres::Client;
use uuid::Uuid;

pub(crate) async fn cosmetic(client: &Client, kind: &str) -> Result<Uuid, tokio_postgres::Error> {
    let id = Uuid::new_v4();
    client.execute("INSERT INTO app.cosmetic(id,kind,asset_ref,state) VALUES($1,$2,'test-only:paid-cosmetic','active')", &[&id,&kind]).await?;
    Ok(id)
}

pub(crate) async fn event(
    client: &Client,
    actor: Uuid,
    provider: &str,
) -> Result<Uuid, tokio_postgres::Error> {
    let id = Uuid::new_v4();
    client.execute("INSERT INTO app.provider_event(id,provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state) VALUES($1,$2,$1::uuid::text,$3,clock_timestamp(),'test-only:normalized-payment','test-only:evidence','verified')", &[&id,&provider,&actor]).await?;
    Ok(id)
}

pub(crate) async fn purchase(
    client: &Client,
    actor: Uuid,
    item: Uuid,
) -> Result<Uuid, tokio_postgres::Error> {
    let proof = event(client, actor, "stripe").await?;
    let id = Uuid::new_v4();
    client.execute("INSERT INTO app.purchase(id,account_id,cosmetic_id,provider_request_ref,provider_event_id,stripe_payment_ref,requested_at,state) VALUES($1,$2,$3,$1::uuid::text,$4,$1::uuid::text,clock_timestamp(),'paid')", &[&id,&actor,&item,&proof]).await?;
    Ok(id)
}
