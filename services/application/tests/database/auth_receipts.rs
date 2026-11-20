//! Privileged contract fixtures, not real Auth issuance or source qualification.
use super::{TestResult, connection};
use serde_json::{Value, json};
use tokio_postgres::{Client, GenericClient, error::SqlState};
use uuid::Uuid;

async fn pending<C: GenericClient + Sync>(
    client: &C,
    principal: &str,
    scope: &str,
    request: &str,
    target: &str,
    payload: &Value,
) -> Result<Uuid, tokio_postgres::Error> {
    Ok(client.query_one(
        "INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref) VALUES($1,$2,$3,$4,app.auth_json_hash($5),'pending','') RETURNING id",
        &[&principal,&scope,&request,&target,&payload],
    ).await?.get(0))
}

async fn accept<C: GenericClient + Sync>(
    client: &C,
    id: Uuid,
    result: &Value,
) -> Result<(), tokio_postgres::Error> {
    client.execute(
        "UPDATE app.command_receipt SET state='accepted',result_ref=$2::jsonb::text,response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',id::text)) WHERE id=$1",
        &[&id,&result],
    ).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn cleanup_contract_requires_exact_retirement_and_atomic_minimal_proof() -> TestResult {
    let mut owner = connection().await?;
    let subject = Uuid::new_v4();
    let hash: String = owner
        .query_one("SELECT app.auth_uuid_hash($1)", &[&subject])
        .await?
        .get(0);
    let principal = format!("auth:{hash}");
    let tx = owner.transaction().await?;
    let retirement = pending(&tx,&principal,"auth_retirement_v1","primary","unregistered",
        &json!({"version":1,"scope":"auth_retirement_v1","subjectHash":hash,"target":"unregistered"})).await?;
    accept(
        &tx,
        retirement,
        &json!({"version":1,"proofHash":"c".repeat(64)}),
    )
    .await?;
    tx.commit().await?;
    let payload =
        json!({"version":1,"scope":"auth_cleanup_v1","subjectHash":hash,"retirementId":retirement});
    for variant in [
        "parent",
        "principal",
        "scope_owner",
        "target",
        "request",
        "payload",
    ] {
        let tx = owner.transaction().await?;
        let mut terms = payload.clone();
        let mut source = principal.clone();
        let mut target = "unregistered".to_owned();
        let mut request = "primary";
        match variant {
            "parent" => source = format!("auth:{}", "a".repeat(64)),
            "principal" => source = format!("human:{subject}"),
            "scope_owner" => source = "worker:unqualified".to_owned(),
            "target" => target = Uuid::new_v4().to_string(),
            "request" => request = "another",
            "payload" => terms["retirementId"] = json!(Uuid::new_v4()),
            _ => unreachable!(),
        }
        assert_eq!(
            pending(&tx, &source, "auth_cleanup_v1", request, &target, &terms)
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::CHECK_VIOLATION),
            "{variant}"
        );
        tx.rollback().await?;
    }
    let valid = json!({"version":1,"retirementId":retirement,"proofHash":"d".repeat(64)});
    for variant in ["missing", "retirement", "proof", "extra"] {
        let tx = owner.transaction().await?;
        let id = pending(
            &tx,
            &principal,
            "auth_cleanup_v1",
            "primary",
            "unregistered",
            &payload,
        )
        .await?;
        let mut result = valid.clone();
        match variant {
            "missing" => {
                result.as_object_mut().unwrap().remove("retirementId");
            }
            "retirement" => result["retirementId"] = json!(Uuid::new_v4()),
            "proof" => result["proofHash"] = json!("invalid"),
            "extra" => result["subjectId"] = json!(subject),
            _ => unreachable!(),
        }
        assert_eq!(
            accept(&tx, id, &result).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION),
            "{variant}"
        );
        tx.rollback().await?;
    }
    let tx = owner.transaction().await?;
    pending(
        &tx,
        &principal,
        "auth_cleanup_v1",
        "primary",
        "unregistered",
        &payload,
    )
    .await?;
    assert_eq!(
        tx.commit().await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let tx = owner.transaction().await?;
    let id = pending(
        &tx,
        &principal,
        "auth_cleanup_v1",
        "primary",
        "unregistered",
        &payload,
    )
    .await?;
    accept(&tx, id, &valid).await?;
    tx.commit().await?;
    assert_eq!(
        owner
            .execute(
                "UPDATE app.command_receipt SET result_ref='{}' WHERE id=$1",
                &[&id]
            )
            .await
            .unwrap_err()
            .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    Ok(())
}

async fn binding(
    client: &mut Client,
    method: &str,
) -> Result<(Uuid, String, String), Box<dyn std::error::Error + Send + Sync>> {
    let subject = Uuid::new_v4();
    let hash: String = client
        .query_one("SELECT app.auth_uuid_hash($1)", &[&subject])
        .await?
        .get(0);
    let principal = format!("auth:{hash}");
    let transaction = client.transaction().await?;
    let id = pending(
        &transaction,
        &principal,
        "auth_binding_v1",
        "primary",
        "primary",
        &json!({"version":1,"scope":"auth_binding_v1","subjectHash":hash}),
    )
    .await?;
    accept(
        &transaction,
        id,
        &json!({"version":1,"method":method,"proofHash":"a".repeat(64)}),
    )
    .await?;
    transaction.commit().await?;
    Ok((id, principal, hash))
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn auth_receipt_namespaces_preserve_terminal_history_and_private_acl() -> TestResult {
    let mut owner = connection().await?;
    let (id, _, _) = binding(&mut owner, "email_code").await?;
    for sql in [
        "UPDATE app.command_receipt SET result_ref=result_ref WHERE id=$1",
        "DELETE FROM app.command_receipt WHERE id=$1",
    ] {
        assert_eq!(
            owner.execute(sql, &[&id]).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    for role in ["anon", "authenticated", "service_role"] {
        let caller = connection().await?;
        caller.batch_execute(&format!("SET ROLE {role}")).await?;
        assert_eq!(
            caller
                .query_one("SELECT app.auth_uuid_hash($1)", &[&Uuid::new_v4()])
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
        for sql in [
            "INSERT INTO app.command_receipt SELECT * FROM app.command_receipt WHERE id=$1",
            "DELETE FROM app.command_receipt WHERE id=$1",
        ] {
            assert_eq!(
                caller.execute(sql, &[&id]).await.unwrap_err().code(),
                Some(&SqlState::INSUFFICIENT_PRIVILEGE)
            );
        }
        assert_eq!(owner.query_one("SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='app' AND p.proname=ANY($2) AND has_function_privilege($1,p.oid,'EXECUTE')", &[&role,&vec!["auth_uuid_hash","auth_json_hash","check_auth_receipt_shape","guard_auth_receipt","check_auth_receipt_commit"]]).await?.get::<_,i64>(0),0);
        assert_eq!(
            caller
                .execute(
                    "UPDATE app.command_receipt SET result_ref='forged' WHERE id=$1",
                    &[&id]
                )
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::INSUFFICIENT_PRIVILEGE)
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn auth_receipt_pending_and_malformed_authority_cannot_commit() -> TestResult {
    let mut owner = connection().await?;
    let hash: String = owner
        .query_one("SELECT app.auth_uuid_hash($1)", &[&Uuid::new_v4()])
        .await?
        .get(0);
    let principal = format!("auth:{hash}");
    let payload = json!({"version":1,"scope":"auth_binding_v1","subjectHash":hash});
    let transaction = owner.transaction().await?;
    let id = pending(
        &transaction,
        &principal,
        "auth_binding_v1",
        "primary",
        "primary",
        &payload,
    )
    .await?;
    assert_eq!(
        transaction.commit().await.unwrap_err().code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    assert_eq!(
        owner
            .query_one(
                "SELECT count(*) FROM app.command_receipt WHERE id=$1",
                &[&id]
            )
            .await?
            .get::<_, i64>(0),
        0
    );
    for (who, scope, request, target, body) in [
        (
            "human:forged".to_owned(),
            "auth_binding_v1",
            "primary",
            "primary",
            payload.clone(),
        ),
        (
            principal.clone(),
            "other_scope",
            "primary",
            "primary",
            payload.clone(),
        ),
        (
            principal.clone(),
            "auth_binding_v1",
            "different",
            "primary",
            payload.clone(),
        ),
        (
            principal.clone(),
            "auth_binding_v1",
            "primary",
            "different",
            payload.clone(),
        ),
        (
            principal.clone(),
            "auth_binding_v1",
            "primary",
            "primary",
            json!({"forged":true}),
        ),
    ] {
        assert_eq!(
            pending(&owner, &who, scope, request, target, &body)
                .await
                .unwrap_err()
                .code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
    }
    for result in [
        json!({"version":1,"method":"password","proofHash":"a".repeat(64)}),
        json!({"version":1,"method":"email_code","proofHash":"a".repeat(64),"rawEmail":"forbidden"}),
        json!({"version":1,"method":"email_code","proofHash":"bad"}),
    ] {
        let transaction = owner.transaction().await?;
        let id = pending(
            &transaction,
            &principal,
            "auth_binding_v1",
            "primary",
            "primary",
            &payload,
        )
        .await?;
        assert_eq!(
            accept(&transaction, id, &result).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        transaction.rollback().await?;
    }
    let valid = json!({"version":1,"method":"email_code","proofHash":"d".repeat(64)});
    let transaction = owner.transaction().await?;
    let id = pending(
        &transaction,
        &principal,
        "auth_binding_v1",
        "primary",
        "primary",
        &payload,
    )
    .await?;
    let wrong_ack = json!({"status":"success","data":{"resourceId":Uuid::new_v4().to_string()}});
    assert_eq!(transaction.execute("UPDATE app.command_receipt SET state='accepted',result_ref=$2::jsonb::text,response_payload=$3 WHERE id=$1", &[&id,&valid,&wrong_ack]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    transaction.rollback().await?;
    for state in ["accepted", "rejected"] {
        let id = Uuid::new_v4();
        let ack = json!({"status":"success","data":{"resourceId":id.to_string()}});
        assert_eq!(owner.execute("INSERT INTO app.command_receipt(id,principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref,response_payload) VALUES($1,$2,'auth_binding_v1','primary','primary',app.auth_json_hash($3),$4,$5::jsonb::text,$6)", &[&id,&principal,&payload,&state,&valid,&ack]).await.unwrap_err().code(),Some(&SqlState::CHECK_VIOLATION));
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated migrated Postgres"]
async fn auth_session_contract_is_bound_to_subject_method_and_exact_result() -> TestResult {
    let mut owner = connection().await?;
    let (binding_id, principal, hash) = binding(&mut owner, "google").await?;
    let session: String = owner
        .query_one("SELECT app.auth_uuid_hash($1)", &[&Uuid::new_v4()])
        .await?
        .get(0);
    let target = binding_id.to_string();
    let payload = json!({"version":1,"scope":"auth_session_v1","subjectHash":hash,"sessionHash":session,"bindingId":target});
    let valid = json!({"version":1,"method":"google","initialClass":"oauth","bindingId":target,"proofHash":"b".repeat(64)});
    let other_hash: String = owner
        .query_one("SELECT app.auth_uuid_hash($1)", &[&Uuid::new_v4()])
        .await?
        .get(0);
    let other_principal = format!("auth:{other_hash}");
    let wrong_subject = json!({"version":1,"scope":"auth_session_v1","subjectHash":other_hash,"sessionHash":session,"bindingId":target});
    assert_eq!(
        pending(
            &owner,
            &other_principal,
            "auth_session_v1",
            &session,
            &target,
            &wrong_subject
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    let missing = Uuid::new_v4().to_string();
    let dangling = json!({"version":1,"scope":"auth_session_v1","subjectHash":hash,"sessionHash":session,"bindingId":missing});
    assert_eq!(
        pending(
            &owner,
            &principal,
            "auth_session_v1",
            &session,
            &missing,
            &dangling
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::CHECK_VIOLATION)
    );
    for (key, value) in [
        ("method", json!("email_code")),
        ("initialClass", json!("otp")),
        ("bindingId", json!(Uuid::new_v4().to_string())),
        ("extra", json!("not part of authority")),
    ] {
        let transaction = owner.transaction().await?;
        let id = pending(
            &transaction,
            &principal,
            "auth_session_v1",
            &session,
            &target,
            &payload,
        )
        .await?;
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert_eq!(
            accept(&transaction, id, &invalid).await.unwrap_err().code(),
            Some(&SqlState::CHECK_VIOLATION)
        );
        transaction.rollback().await?;
    }
    let transaction = owner.transaction().await?;
    let id = pending(
        &transaction,
        &principal,
        "auth_session_v1",
        &session,
        &target,
        &payload,
    )
    .await?;
    accept(&transaction, id, &valid).await?;
    transaction.commit().await?;
    assert_eq!(
        pending(
            &owner,
            &principal,
            "auth_session_v1",
            &session,
            &target,
            &payload
        )
        .await
        .unwrap_err()
        .code(),
        Some(&SqlState::UNIQUE_VIOLATION)
    );
    let retirement = json!({"version":1,"scope":"auth_retirement_v1","subjectHash":hash,"target":"unregistered"});
    let transaction = owner.transaction().await?;
    let id = pending(
        &transaction,
        &principal,
        "auth_retirement_v1",
        "primary",
        "unregistered",
        &retirement,
    )
    .await?;
    accept(
        &transaction,
        id,
        &json!({"version":1,"proofHash":"c".repeat(64)}),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}
