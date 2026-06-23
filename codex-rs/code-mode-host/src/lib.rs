use std::collections::HashSet;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use codex_code_mode_protocol::host::CapabilitySet;
use codex_code_mode_protocol::host::ClientToHost;
use codex_code_mode_protocol::host::FramedReader;
use codex_code_mode_protocol::host::FramedWriter;
use codex_code_mode_protocol::host::HandshakeRejectReason;
use codex_code_mode_protocol::host::HostHello;
use codex_code_mode_protocol::host::HostToClient;
use codex_code_mode_protocol::host::ProtocolVersion;
use codex_code_mode_protocol::host::SupportedProtocolVersions;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;

/// Runs one code-mode host connection over the process standard streams.
pub async fn run_stdio() -> Result<()> {
    run(tokio::io::stdin(), tokio::io::stdout()).await
}

/// Runs one code-mode host connection over an ordered input/output pair.
async fn run<R, W>(reader: R, writer: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = FramedReader::new(reader);
    let mut writer = FramedWriter::new(writer);

    let Some(first_message) = reader
        .read::<ClientToHost>()
        .await
        .context("failed to read code-mode client hello")?
    else {
        return Ok(());
    };
    let ClientToHost::ClientHello(client_hello) = first_message else {
        writer
            .write(&HostToClient::HandshakeRejected {
                reason: HandshakeRejectReason::InvalidHello {
                    message: "first message must be connection/hello".to_string(),
                },
            })
            .await
            .context("failed to reject invalid code-mode client hello")?;
        return Ok(());
    };

    let supported_versions = SupportedProtocolVersions::try_new([ProtocolVersion::V1])?;
    if !client_hello
        .supported_versions()
        .contains(ProtocolVersion::V1)
    {
        writer
            .write(&HostToClient::HandshakeRejected {
                reason: HandshakeRejectReason::NoCompatibleVersion { supported_versions },
            })
            .await
            .context("failed to reject incompatible code-mode client")?;
        return Ok(());
    }

    let host_capabilities = CapabilitySet::empty();
    if let Some(capability) = client_hello
        .required_capabilities()
        .iter()
        .find(|capability| !host_capabilities.contains(capability))
    {
        writer
            .write(&HostToClient::HandshakeRejected {
                reason: HandshakeRejectReason::MissingRequiredCapability {
                    capability: capability.clone(),
                },
            })
            .await
            .context("failed to reject unsupported code-mode capability")?;
        return Ok(());
    }

    writer
        .write(&HostToClient::HostHello(HostHello::new(
            ProtocolVersion::V1,
            host_capabilities,
        )))
        .await
        .context("failed to write code-mode host hello")?;

    let mut seen_session_ids = HashSet::new();
    let mut open_session_ids = HashSet::new();
    while let Some(message) = reader
        .read::<ClientToHost>()
        .await
        .context("failed to read code-mode client message")?
    {
        match message {
            ClientToHost::ClientHello(_) => {
                bail!("received a second code-mode client hello");
            }
            ClientToHost::OpenSession { session_id } => {
                if !seen_session_ids.insert(session_id.clone()) {
                    bail!("code-mode session ID `{session_id}` was reused");
                }
                open_session_ids.insert(session_id.clone());
                writer
                    .write(&HostToClient::SessionReady { session_id })
                    .await
                    .context("failed to acknowledge opened code-mode session")?;
            }
            ClientToHost::CloseSession { session_id } => {
                if !open_session_ids.remove(&session_id) {
                    bail!("cannot close unknown code-mode session `{session_id}`");
                }
                writer
                    .write(&HostToClient::SessionClosed { session_id })
                    .await
                    .context("failed to acknowledge closed code-mode session")?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
