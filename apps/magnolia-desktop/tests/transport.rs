use futures::{SinkExt, StreamExt};
use magnolia_desktop::{HostConfiguration, MagnoliaHost};
use magnolia_domain::{ClientId, DocumentRevision, RequestId, WorkspaceEdit, WorkspaceEditBatch};
use magnolia_protocol::{
    CommandEnvelope, ConnectRequest, ConnectResponse, ControlClientMessage, ControlServerMessage,
    ProtocolVersionRange, ReconnectCursor, RequestSequence, SemanticCommand, SessionCredential,
    TransportErrorCode, PROTOCOL_VERSION,
};
use serde_json::json;
use std::fs;
use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        http::{HeaderValue, StatusCode},
        Error as WebSocketError, Message,
    },
    MaybeTlsStream, WebSocketStream,
};

type TestSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[tokio::test]
async fn authenticated_control_round_trip_replays_receipts_without_reexecution() {
    let (_assets, host) = start_host().await;
    let ready = host.ready_info();
    let client_id = ClientId::from_u128(41);
    let mut socket = connect(
        &ready.origin,
        ready.launch_url.split("#token=").nth(1).unwrap(),
        client_id,
    )
    .await;
    let connected = next_control(&mut socket).await;
    let ControlServerMessage::Connected {
        response, resumed, ..
    } = connected
    else {
        panic!("expected immediate connected snapshot");
    };
    assert!(!resumed);
    let ConnectResponse::Accepted { snapshot, .. } = response else {
        panic!("expected accepted protocol handshake");
    };
    assert_eq!(snapshot.document_revision, DocumentRevision::ZERO);

    let command = CommandEnvelope {
        protocol_version: PROTOCOL_VERSION,
        client_id,
        request_id: RequestId::from_u128(88),
        request_sequence: RequestSequence::new(1),
        expected_document_revision: DocumentRevision::ZERO,
        command: SemanticCommand::ApplyWorkspaceEdit {
            batch: WorkspaceEditBatch::new(vec![WorkspaceEdit::SetPromotedSetting {
                key: "transport.test".to_owned(),
                value: json!(true),
            }]),
        },
    };
    send_control(
        &mut socket,
        &ControlClientMessage::Command {
            command: command.clone(),
        },
    )
    .await;
    let first = next_receipt(&mut socket).await;
    assert!(first.accepted());
    assert_eq!(first.document_revision, DocumentRevision::new(1));
    assert_eq!(first.target_graph_revision.get(), 0);
    assert!(first.operation_id.is_none());

    send_control(&mut socket, &ControlClientMessage::Command { command }).await;
    let replay = next_receipt(&mut socket).await;
    assert_eq!(replay, first);
    socket.close(None).await.unwrap();
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn control_upgrade_rejects_wrong_origin_and_wrong_credential() {
    let (_assets, host) = start_host().await;
    let ready = host.ready_info();
    let url = ready.origin.replace("http://", "ws://") + "/api/control";
    let mut bad_origin = url.clone().into_client_request().unwrap();
    bad_origin
        .headers_mut()
        .insert("origin", HeaderValue::from_static("http://example.invalid"));
    let error = connect_async(bad_origin).await.unwrap_err();
    assert!(matches!(
        error,
        WebSocketError::Http(response) if response.status() == StatusCode::FORBIDDEN
    ));

    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_str(&ready.origin).unwrap());
    let (mut socket, _) = connect_async(request).await.unwrap();
    send_control(
        &mut socket,
        &ControlClientMessage::Authenticate {
            credential: SessionCredential::LaunchToken("not-a-valid-token".to_owned()),
            connect: connect_request(ClientId::from_u128(7), PROTOCOL_VERSION.major),
            cursor: ReconnectCursor::default(),
        },
    )
    .await;
    assert!(matches!(
        next_control(&mut socket).await,
        ControlServerMessage::Error {
            code: TransportErrorCode::InvalidCredential,
            fatal: true,
            ..
        }
    ));
    host.shutdown().await.unwrap();
}

#[tokio::test]
async fn unsupported_protocol_major_is_explicitly_rejected() {
    let (_assets, host) = start_host().await;
    let ready = host.ready_info();
    let token = ready.launch_url.split("#token=").nth(1).unwrap();
    let client_id = ClientId::from_u128(99);
    let url = ready.origin.replace("http://", "ws://") + "/api/control";
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_str(&ready.origin).unwrap());
    let (mut socket, _) = connect_async(request).await.unwrap();
    send_control(
        &mut socket,
        &ControlClientMessage::Authenticate {
            credential: SessionCredential::LaunchToken(token.to_owned()),
            connect: connect_request(client_id, 99),
            cursor: ReconnectCursor::default(),
        },
    )
    .await;
    let ControlServerMessage::Connected { response, .. } = next_control(&mut socket).await else {
        panic!("expected negotiated rejection response");
    };
    assert!(matches!(response, ConnectResponse::Rejected { .. }));
    host.shutdown().await.unwrap();
}

async fn start_host() -> (TempDir, MagnoliaHost) {
    let assets = tempfile::tempdir().unwrap();
    fs::write(
        assets.path().join("index.html"),
        "<!doctype html><title>test</title>",
    )
    .unwrap();
    let host = MagnoliaHost::start(HostConfiguration {
        assets: assets.path().to_path_buf(),
        launch_browser: false,
        test_mode: true,
        auto_activate: false,
        ..HostConfiguration::default()
    })
    .await
    .unwrap();
    (assets, host)
}

async fn connect(origin: &str, token: &str, client_id: ClientId) -> TestSocket {
    let url = origin.replace("http://", "ws://") + "/api/control";
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_str(origin).unwrap());
    let (mut socket, _) = connect_async(request).await.unwrap();
    send_control(
        &mut socket,
        &ControlClientMessage::Authenticate {
            credential: SessionCredential::LaunchToken(token.to_owned()),
            connect: connect_request(client_id, PROTOCOL_VERSION.major),
            cursor: ReconnectCursor::default(),
        },
    )
    .await;
    socket
}

fn connect_request(client_id: ClientId, major: u16) -> ConnectRequest {
    ConnectRequest {
        client_id,
        supported_versions: vec![ProtocolVersionRange {
            major,
            minimum_minor: PROTOCOL_VERSION.minor,
            maximum_minor: PROTOCOL_VERSION.minor,
        }],
    }
}

async fn send_control(socket: &mut TestSocket, message: &ControlClientMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(message).unwrap().into(),
        ))
        .await
        .unwrap();
}

async fn next_control(socket: &mut TestSocket) -> ControlServerMessage {
    loop {
        let message = socket.next().await.unwrap().unwrap();
        if let Message::Text(text) = message {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

async fn next_receipt(socket: &mut TestSocket) -> magnolia_protocol::CommandReceipt {
    loop {
        if let ControlServerMessage::Receipt { receipt } = next_control(socket).await {
            return receipt;
        }
    }
}
