use codex_code_mode_protocol::host::Capability;
use codex_code_mode_protocol::host::CapabilitySet;
use codex_code_mode_protocol::host::ClientHello;
use codex_code_mode_protocol::host::ClientToHost;
use codex_code_mode_protocol::host::FramedReader;
use codex_code_mode_protocol::host::FramedWriter;
use codex_code_mode_protocol::host::HandshakeRejectReason;
use codex_code_mode_protocol::host::HostHello;
use codex_code_mode_protocol::host::HostToClient;
use codex_code_mode_protocol::host::ProtocolVersion;
use codex_code_mode_protocol::host::SessionId;
use codex_code_mode_protocol::host::SupportedProtocolVersions;
use pretty_assertions::assert_eq;

use super::run;

fn client_hello(
    versions: impl IntoIterator<Item = ProtocolVersion>,
    required_capabilities: CapabilitySet,
) -> ClientToHost {
    ClientToHost::ClientHello(
        ClientHello::new(
            SupportedProtocolVersions::try_new(versions).expect("supported versions"),
            required_capabilities,
            CapabilitySet::empty(),
        )
        .expect("client hello"),
    )
}

fn session_id(value: &str) -> SessionId {
    SessionId::new(value).expect("session ID")
}

#[tokio::test]
async fn handshake_and_multiple_session_lifecycles_are_ordered() {
    let (host_stream, client_stream) = tokio::io::duplex(/*max_buf_size*/ 4096);
    let (host_reader, host_writer) = tokio::io::split(host_stream);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let host = tokio::spawn(run(host_reader, host_writer));
    let mut reader = FramedReader::new(client_reader);
    let mut writer = FramedWriter::new(client_writer);

    writer
        .write(&client_hello([ProtocolVersion::V1], CapabilitySet::empty()))
        .await
        .expect("write hello");
    assert_eq!(
        reader.read::<HostToClient>().await.expect("read hello"),
        Some(HostToClient::HostHello(HostHello::new(
            ProtocolVersion::V1,
            CapabilitySet::empty(),
        )))
    );

    for id in ["session-1", "session-2"] {
        writer
            .write(&ClientToHost::OpenSession {
                session_id: session_id(id),
            })
            .await
            .expect("open session");
        assert_eq!(
            reader.read::<HostToClient>().await.expect("session ready"),
            Some(HostToClient::SessionReady {
                session_id: session_id(id),
            })
        );
    }

    for id in ["session-1", "session-2"] {
        writer
            .write(&ClientToHost::CloseSession {
                session_id: session_id(id),
            })
            .await
            .expect("close session");
        assert_eq!(
            reader.read::<HostToClient>().await.expect("session closed"),
            Some(HostToClient::SessionClosed {
                session_id: session_id(id),
            })
        );
    }

    drop(writer);
    host.await.expect("host task").expect("host connection");
}

#[tokio::test]
async fn incompatible_handshake_is_rejected_and_closes_connection() {
    let (host_stream, client_stream) = tokio::io::duplex(/*max_buf_size*/ 1024);
    let (host_reader, host_writer) = tokio::io::split(host_stream);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let host = tokio::spawn(run(host_reader, host_writer));
    let mut reader = FramedReader::new(client_reader);
    let mut writer = FramedWriter::new(client_writer);

    let version_two = ProtocolVersion::new(/*value*/ 2).expect("protocol version");
    writer
        .write(&client_hello([version_two], CapabilitySet::empty()))
        .await
        .expect("write hello");
    assert_eq!(
        reader.read::<HostToClient>().await.expect("rejection"),
        Some(HostToClient::HandshakeRejected {
            reason: HandshakeRejectReason::NoCompatibleVersion {
                supported_versions: SupportedProtocolVersions::try_new([ProtocolVersion::V1])
                    .expect("host versions"),
            },
        })
    );
    assert_eq!(
        reader.read::<HostToClient>().await.expect("connection eof"),
        None
    );
    host.await.expect("host task").expect("host connection");
}

#[tokio::test]
async fn unsupported_required_capability_is_rejected() {
    let (host_stream, client_stream) = tokio::io::duplex(/*max_buf_size*/ 1024);
    let (host_reader, host_writer) = tokio::io::split(host_stream);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let host = tokio::spawn(run(host_reader, host_writer));
    let mut reader = FramedReader::new(client_reader);
    let mut writer = FramedWriter::new(client_writer);
    let capability = Capability::new("required").expect("capability");

    writer
        .write(&client_hello(
            [ProtocolVersion::V1],
            CapabilitySet::try_new([capability.clone()]).expect("capabilities"),
        ))
        .await
        .expect("write hello");
    assert_eq!(
        reader.read::<HostToClient>().await.expect("rejection"),
        Some(HostToClient::HandshakeRejected {
            reason: HandshakeRejectReason::MissingRequiredCapability { capability },
        })
    );
    host.await.expect("host task").expect("host connection");
}

#[tokio::test]
async fn invalid_message_order_faults_the_connection() {
    let (host_stream, client_stream) = tokio::io::duplex(/*max_buf_size*/ 1024);
    let (host_reader, host_writer) = tokio::io::split(host_stream);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let host = tokio::spawn(run(host_reader, host_writer));
    let mut reader = FramedReader::new(client_reader);
    let mut writer = FramedWriter::new(client_writer);

    writer
        .write(&ClientToHost::OpenSession {
            session_id: session_id("session-1"),
        })
        .await
        .expect("write invalid first message");
    assert_eq!(
        reader.read::<HostToClient>().await.expect("rejection"),
        Some(HostToClient::HandshakeRejected {
            reason: HandshakeRejectReason::InvalidHello {
                message: "first message must be connection/hello".to_string(),
            },
        })
    );
    assert_eq!(
        reader.read::<HostToClient>().await.expect("connection eof"),
        None
    );
    host.await.expect("host task").expect("host connection");

    let (host_stream, client_stream) = tokio::io::duplex(/*max_buf_size*/ 1024);
    let (host_reader, host_writer) = tokio::io::split(host_stream);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let host = tokio::spawn(run(host_reader, host_writer));
    let mut reader = FramedReader::new(client_reader);
    let mut writer = FramedWriter::new(client_writer);
    writer
        .write(&client_hello([ProtocolVersion::V1], CapabilitySet::empty()))
        .await
        .expect("write hello");
    reader
        .read::<HostToClient>()
        .await
        .expect("read hello")
        .expect("host hello");
    writer
        .write(&ClientToHost::CloseSession {
            session_id: session_id("unknown"),
        })
        .await
        .expect("close unknown session");
    drop(writer);
    let err = host
        .await
        .expect("host task")
        .expect_err("invalid session transition");
    assert_eq!(
        err.to_string(),
        "cannot close unknown code-mode session `unknown`"
    );
}

#[tokio::test]
async fn session_id_cannot_be_reused_after_close() {
    let (host_stream, client_stream) = tokio::io::duplex(/*max_buf_size*/ 2048);
    let (host_reader, host_writer) = tokio::io::split(host_stream);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let host = tokio::spawn(run(host_reader, host_writer));
    let mut reader = FramedReader::new(client_reader);
    let mut writer = FramedWriter::new(client_writer);
    writer
        .write(&client_hello([ProtocolVersion::V1], CapabilitySet::empty()))
        .await
        .expect("write hello");
    reader
        .read::<HostToClient>()
        .await
        .expect("read hello")
        .expect("host hello");

    let id = session_id("session-1");
    writer
        .write(&ClientToHost::OpenSession {
            session_id: id.clone(),
        })
        .await
        .expect("open session");
    reader
        .read::<HostToClient>()
        .await
        .expect("session ready")
        .expect("session ready message");
    writer
        .write(&ClientToHost::CloseSession {
            session_id: id.clone(),
        })
        .await
        .expect("close session");
    reader
        .read::<HostToClient>()
        .await
        .expect("session closed")
        .expect("session closed message");
    writer
        .write(&ClientToHost::OpenSession { session_id: id })
        .await
        .expect("reuse session ID");
    drop(writer);

    let err = host
        .await
        .expect("host task")
        .expect_err("reused session ID");
    assert_eq!(
        err.to_string(),
        "code-mode session ID `session-1` was reused"
    );
}
