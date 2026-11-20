//! Actual current installer readback using the shared interception fixture.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires tools/test_local_patreon_workflow.py owned server/namespace"]
async fn production_installer_recovers_hidden_ack_reuses_running_and_rejects_wrong_type_or_terminal_state()
 {
    crate::patreon::member_installer_tests::exercise(
        super::runtime::client().await,
        crate::patreon::member_installer_tests::Domain::Current,
    )
    .await;
}
