use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::mem::size_of;
use std::process::Command;
use std::process::Stdio;

use codex_code_mode_protocol::host::CapabilitySet;
use codex_code_mode_protocol::host::ClientHello;
use codex_code_mode_protocol::host::ClientToHost;
use codex_code_mode_protocol::host::HostHello;
use codex_code_mode_protocol::host::HostToClient;
use codex_code_mode_protocol::host::ProtocolVersion;
use codex_code_mode_protocol::host::SessionId;
use codex_code_mode_protocol::host::SupportedProtocolVersions;
use pretty_assertions::assert_eq;

fn encode_frame(message: &ClientToHost) -> anyhow::Result<Vec<u8>> {
    let payload = serde_json::to_vec(message)?;
    let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
    frame.extend(payload);
    Ok(frame)
}

fn decode_frame(cursor: &mut Cursor<Vec<u8>>) -> anyhow::Result<Option<HostToClient>> {
    let mut length = [0_u8; size_of::<u32>()];
    match cursor.read_exact(&mut length) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    let mut payload = vec![0; u32::from_le_bytes(length) as usize];
    cursor.read_exact(&mut payload)?;
    Ok(Some(serde_json::from_slice(&payload)?))
}

#[test]
fn binary_serves_protocol_over_stdin_and_stdout() -> anyhow::Result<()> {
    let host_binary =
        codex_utils_cargo_bin::cargo_bin("codex-code-mode-host").expect("host binary");
    let mut child = Command::new(host_binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn host");
    let session_id = SessionId::new("session-1").expect("session ID");
    let messages = [
        ClientToHost::ClientHello(
            ClientHello::new(
                SupportedProtocolVersions::try_new([ProtocolVersion::V1])
                    .expect("supported versions"),
                CapabilitySet::empty(),
                CapabilitySet::empty(),
            )
            .expect("client hello"),
        ),
        ClientToHost::OpenSession {
            session_id: session_id.clone(),
        },
        ClientToHost::CloseSession {
            session_id: session_id.clone(),
        },
    ];
    let mut stdin = child.stdin.take().expect("child stdin");
    for message in messages {
        stdin
            .write_all(&encode_frame(&message).expect("encode frame"))
            .expect("write frame");
    }
    drop(stdin);

    let output = child.wait_with_output().expect("wait for host");
    assert!(
        output.status.success(),
        "host failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stderr, Vec::<u8>::new());

    let mut stdout = Cursor::new(output.stdout);
    assert_eq!(
        decode_frame(&mut stdout)?,
        Some(HostToClient::HostHello(HostHello::new(
            ProtocolVersion::V1,
            CapabilitySet::empty(),
        )))
    );
    assert_eq!(
        decode_frame(&mut stdout)?,
        Some(HostToClient::SessionReady {
            session_id: session_id.clone(),
        })
    );
    assert_eq!(
        decode_frame(&mut stdout)?,
        Some(HostToClient::SessionClosed { session_id })
    );
    assert_eq!(decode_frame(&mut stdout)?, None);
    Ok(())
}
