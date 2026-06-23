use std::sync::Arc;

use codex_code_mode_protocol::CodeModeSession;
use codex_code_mode_protocol::CodeModeSessionProvider;
use pretty_assertions::assert_eq;

use super::ProcessOwnedCodeModeSession;
use super::ProcessOwnedCodeModeSessionProvider;
use crate::NoopCodeModeSessionDelegate;

#[test]
fn provider_reuses_its_live_process_host() {
    let provider = ProcessOwnedCodeModeSessionProvider::default();

    let first = provider.process_host();
    let second = provider.process_host();

    assert!(Arc::ptr_eq(&first, &second));
}

#[tokio::test]
async fn provider_allocates_distinct_logical_sessions() {
    let provider = ProcessOwnedCodeModeSessionProvider::default();

    let first = provider
        .create_session(Arc::new(NoopCodeModeSessionDelegate))
        .await
        .expect("first session");
    let second = provider
        .create_session(Arc::new(NoopCodeModeSessionDelegate))
        .await
        .expect("second session");

    assert!(first.is_alive());
    assert!(second.is_alive());
    assert!(!Arc::ptr_eq(&first, &second));
}

#[tokio::test]
async fn shutdown_only_closes_the_logical_session() {
    let first = ProcessOwnedCodeModeSession::new();
    let second = ProcessOwnedCodeModeSession::new();

    first.shutdown().await.expect("shutdown session");

    assert_eq!(first.is_alive(), false);
    assert_eq!(second.is_alive(), true);
}
