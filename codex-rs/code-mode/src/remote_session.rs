use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use codex_code_mode_protocol::CellId;
use codex_code_mode_protocol::CodeModeSession;
use codex_code_mode_protocol::CodeModeSessionDelegate;
use codex_code_mode_protocol::CodeModeSessionProvider;
use codex_code_mode_protocol::CodeModeSessionProviderFuture;
use codex_code_mode_protocol::CodeModeSessionResultFuture;
use codex_code_mode_protocol::ExecuteRequest;
use codex_code_mode_protocol::StartedCell;
use codex_code_mode_protocol::WaitOutcome;
use codex_code_mode_protocol::WaitRequest;

use crate::NoopCodeModeSessionDelegate;

/// Creates code-mode sessions backed by one lazily initialized process host.
///
/// The transport is not wired up yet. Keeping process ownership in the provider
/// establishes the intended lifetime: sessions created by one provider share a
/// host, while each session retains its own delegate and logical session ID.
#[derive(Default)]
pub struct ProcessOwnedCodeModeSessionProvider {
    process_host: Mutex<Option<Arc<OwnedProcessHost>>>,
}

impl ProcessOwnedCodeModeSessionProvider {
    fn process_host(&self) -> Arc<OwnedProcessHost> {
        let mut process_host = self
            .process_host
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(process_host) = process_host.as_ref()
            && process_host.is_alive()
        {
            return Arc::clone(process_host);
        }

        let new_process_host = Arc::new(OwnedProcessHost::new());
        *process_host = Some(Arc::clone(&new_process_host));
        new_process_host
    }
}

impl CodeModeSessionProvider for ProcessOwnedCodeModeSessionProvider {
    fn create_session<'a>(
        &'a self,
        delegate: Arc<dyn CodeModeSessionDelegate>,
    ) -> CodeModeSessionProviderFuture<'a> {
        let session = ProcessOwnedCodeModeSession::with_process_host(delegate, self.process_host());
        Box::pin(async move {
            let session: Arc<dyn CodeModeSession> = Arc::new(session);
            Ok(session)
        })
    }
}

/// Owns the eventual child process and its single event-stream reader/writer.
///
/// Transport startup and failure propagation will be added with the remote
/// protocol. The liveness and session-ID state live here now so that work does
/// not leak into callers when that transport is introduced.
struct OwnedProcessHost {
    next_session_id: AtomicU64,
}

impl OwnedProcessHost {
    fn new() -> Self {
        Self {
            next_session_id: AtomicU64::new(1),
        }
    }

    fn allocate_session_id(&self) -> ProcessSessionId {
        ProcessSessionId(self.next_session_id.fetch_add(1, Ordering::Relaxed))
    }

    fn unimplemented_operation<T>(
        &self,
        session_id: ProcessSessionId,
        operation: &str,
    ) -> Result<T, String> {
        Err(format!(
            "remote code-mode operation `{operation}` is not implemented for session {}",
            session_id.0
        ))
    }
}

#[derive(Clone, Copy)]
struct ProcessSessionId(u64);

/// A logical code-mode session assigned to a process-owned host.
pub struct ProcessOwnedCodeModeSession {
    process_host: Arc<OwnedProcessHost>,
    session_id: ProcessSessionId,
    _delegate: Arc<dyn CodeModeSessionDelegate>,
}

impl ProcessOwnedCodeModeSession {
    pub fn new() -> Self {
        Self::with_process_host(
            Arc::new(NoopCodeModeSessionDelegate),
            Arc::new(OwnedProcessHost::new()),
        )
    }

    fn with_process_host(
        delegate: Arc<dyn CodeModeSessionDelegate>,
        process_host: Arc<OwnedProcessHost>,
    ) -> Self {
        let session_id = process_host.allocate_session_id();
        Self {
            process_host,
            session_id,
            _delegate: delegate,
        }
    }

    pub async fn execute(&self, _request: ExecuteRequest) -> Result<StartedCell, String> {
        self.process_host
            .unimplemented_operation(self.session_id, "execute")
    }

    pub async fn wait(&self, _request: WaitRequest) -> Result<WaitOutcome, String> {
        self.process_host
            .unimplemented_operation(self.session_id, "wait")
    }

    pub async fn terminate(&self, _cell_id: CellId) -> Result<WaitOutcome, String> {
        self.process_host
            .unimplemented_operation(self.session_id, "terminate")
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Default for ProcessOwnedCodeModeSession {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeModeSession for ProcessOwnedCodeModeSession {
    fn execute<'a>(
        &'a self,
        request: ExecuteRequest,
    ) -> CodeModeSessionResultFuture<'a, StartedCell> {
        Box::pin(ProcessOwnedCodeModeSession::execute(self, request))
    }

    fn wait<'a>(&'a self, request: WaitRequest) -> CodeModeSessionResultFuture<'a, WaitOutcome> {
        Box::pin(ProcessOwnedCodeModeSession::wait(self, request))
    }

    fn terminate<'a>(&'a self, cell_id: CellId) -> CodeModeSessionResultFuture<'a, WaitOutcome> {
        Box::pin(ProcessOwnedCodeModeSession::terminate(self, cell_id))
    }

    fn shutdown<'a>(&'a self) -> CodeModeSessionResultFuture<'a, ()> {
        Box::pin(ProcessOwnedCodeModeSession::shutdown(self))
    }
}

#[cfg(test)]
#[path = "remote_session_tests.rs"]
mod tests;
