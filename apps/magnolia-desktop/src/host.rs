use crate::{
    AuthError, BoundedTelemetryQueue, BrowserLaunchError, BrowserProcess, SessionAuthority,
    TelemetryError, TelemetryHub, TelemetryStatus, CAPTION_SESSION,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{header::ORIGIN, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures::{SinkExt, StreamExt};
use magnolia_application::{ApplicationService, InMemoryPersistence};
use magnolia_domain::{synthetic, EntityId, RuntimeEpochId, TargetGraphRevision};
use magnolia_protocol::{
    encode_telemetry_postcard, ConnectResponse, ControlClientMessage, ControlServerMessage,
    TelemetryClientMessage, TelemetryServerMessage, TranscriptSegment, TransportErrorCode,
    PROTOCOL_VERSION,
};
use magnolia_runtime::{MockRuntime, MockRuntimeError};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::{
    net::TcpListener,
    sync::watch,
    task::JoinHandle,
    time::{interval, MissedTickBehavior},
};
use tower_http::services::{ServeDir, ServeFile};

type NativeService = ApplicationService<InMemoryPersistence, MockRuntime>;

const DEFAULT_LAUNCH_TTL: Duration = Duration::from_secs(5 * 60);
const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const TEST_AUTHORITY_HEADER: &str = "x-magnolia-test-authority";

#[derive(Debug, Clone)]
pub struct HostConfiguration {
    pub assets: PathBuf,
    pub port: u16,
    pub chromium: Option<PathBuf>,
    pub launch_browser: bool,
    pub auto_activate: bool,
    pub test_mode: bool,
    pub launch_token_ttl: Duration,
    pub session_ttl: Duration,
}

impl Default for HostConfiguration {
    fn default() -> Self {
        Self {
            assets: PathBuf::from("target/magnolia-studio-web-dist"),
            port: 0,
            chromium: None,
            launch_browser: true,
            auto_activate: true,
            test_mode: false,
            launch_token_ttl: DEFAULT_LAUNCH_TTL,
            session_ttl: DEFAULT_SESSION_TTL,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct HostReadyInfo {
    pub origin: String,
    pub launch_url: String,
    pub runtime_epoch: RuntimeEpochId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_authority: Option<String>,
}

impl fmt::Debug for HostReadyInfo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostReadyInfo")
            .field("origin", &self.origin)
            .field(
                "launch_url",
                &format_args!("{}/#token=[redacted]", self.origin),
            )
            .field("runtime_epoch", &self.runtime_epoch)
            .field(
                "test_authority",
                &self.test_authority.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

pub struct MagnoliaHost {
    ready: HostReadyInfo,
    browser: Option<BrowserProcess>,
    browser_error: Option<String>,
    shutdown: watch::Sender<bool>,
    server_task: JoinHandle<Result<(), std::io::Error>>,
    transcript_task: JoinHandle<()>,
}

impl MagnoliaHost {
    pub async fn start(configuration: HostConfiguration) -> Result<Self, HostError> {
        let index = configuration.assets.join("index.html");
        if !index.is_file() {
            return Err(HostError::MissingAssets(index));
        }

        let listener = TcpListener::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            configuration.port,
        ))
        .await
        .map_err(HostError::Bind)?;
        let address = listener.local_addr().map_err(HostError::Bind)?;
        if address.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
            return Err(HostError::NonLoopback(address));
        }
        let origin = format!("http://127.0.0.1:{}", address.port());
        let authority =
            SessionAuthority::new(configuration.launch_token_ttl, configuration.session_ttl)?;
        let launch_url = format!("{origin}/#token={}", authority.launch_token());
        let runtime_epoch = RuntimeEpochId::new();
        let runtime = MockRuntime::new();
        let service = ApplicationService::new(
            InMemoryPersistence::default(),
            runtime.clone(),
            synthetic::registry(),
            runtime_epoch,
        )?;
        let telemetry = TelemetryHub::default();
        let test_authority = configuration.test_mode.then(random_token).transpose()?;
        let (test_disconnect, _) = watch::channel(0_u64);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let state = HostState {
            origin: Arc::<str>::from(origin.clone()),
            authority,
            service: service.clone(),
            runtime,
            telemetry,
            auto_activate: configuration.auto_activate,
            test_authority: test_authority.clone(),
            test_disconnect,
            shutdown: shutdown_rx.clone(),
        };
        let app = router(
            state,
            &configuration.assets,
            &index,
            configuration.test_mode,
        );
        let server_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(wait_for_shutdown(shutdown_rx))
                .await
        });
        let transcript_task = tokio::spawn(run_synthetic_transcript(
            service,
            runtime_epoch,
            shutdown.subscribe(),
        ));

        let (browser, browser_error) = if configuration.launch_browser {
            match BrowserProcess::launch(&launch_url, configuration.chromium.as_deref()) {
                Ok(browser) => (Some(browser), None),
                Err(error) => (None, Some(error.to_string())),
            }
        } else {
            (None, None)
        };

        Ok(Self {
            ready: HostReadyInfo {
                origin,
                launch_url,
                runtime_epoch,
                test_authority,
            },
            browser,
            browser_error,
            shutdown,
            server_task,
            transcript_task,
        })
    }

    #[must_use]
    pub fn ready_info(&self) -> HostReadyInfo {
        self.ready.clone()
    }

    #[must_use]
    pub fn launch_url(&self) -> &str {
        &self.ready.launch_url
    }

    #[must_use]
    pub fn browser_launch_error(&self) -> Option<&str> {
        self.browser_error.as_deref()
    }

    pub async fn shutdown(mut self) -> Result<(), HostError> {
        let _ = self.shutdown.send(true);
        if let Some(browser) = self.browser.take() {
            browser.shutdown().await?;
        }
        self.transcript_task
            .await
            .map_err(|error| HostError::Task(error.to_string()))?;
        self.server_task
            .await
            .map_err(|error| HostError::Task(error.to_string()))??;
        Ok(())
    }
}

#[derive(Clone)]
struct HostState {
    origin: Arc<str>,
    authority: SessionAuthority,
    service: NativeService,
    runtime: MockRuntime,
    telemetry: TelemetryHub,
    auto_activate: bool,
    test_authority: Option<String>,
    test_disconnect: watch::Sender<u64>,
    shutdown: watch::Receiver<bool>,
}

fn router(state: HostState, assets: &PathBuf, index: &PathBuf, test_mode: bool) -> Router {
    let mut router = Router::new()
        .route("/api/health", get(health))
        .route("/api/control", get(control_upgrade))
        .route("/api/telemetry", get(telemetry_upgrade));
    if test_mode {
        router = router
            .route("/__test/status", get(test_status))
            .route("/__test/runtime", post(test_runtime))
            .route("/__test/telemetry", post(test_telemetry))
            .route("/__test/disconnect", post(test_disconnect));
    }
    router
        .fallback_service(ServeDir::new(assets).not_found_service(ServeFile::new(index)))
        .with_state(state)
}

async fn health(State(state): State<HostState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ready",
        runtime_epoch: state
            .service
            .snapshot()
            .map(|projection| projection.runtime_epoch)
            .unwrap_or_else(|_| RuntimeEpochId::from_u128(0)),
    })
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    runtime_epoch: RuntimeEpochId,
}

async fn control_upgrade(
    State(state): State<HostState>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Response {
    if !origin_is_exact(&headers, &state.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    websocket
        .on_upgrade(move |socket| run_control_socket(socket, state))
        .into_response()
}

async fn run_control_socket(mut socket: WebSocket, state: HostState) {
    let Some(Ok(Message::Text(first))) = socket.recv().await else {
        let _ = send_control_error(
            &mut socket,
            TransportErrorCode::AuthenticationRequired,
            "the first control message must authenticate",
            true,
        )
        .await;
        return;
    };
    let authentication = match serde_json::from_str::<ControlClientMessage>(&first) {
        Ok(ControlClientMessage::Authenticate {
            credential,
            connect,
            cursor,
        }) => (credential, connect, cursor),
        Ok(_) => {
            let _ = send_control_error(
                &mut socket,
                TransportErrorCode::AuthenticationRequired,
                "the first control message must authenticate",
                true,
            )
            .await;
            return;
        }
        Err(_) => {
            let _ = send_control_error(
                &mut socket,
                TransportErrorCode::MalformedMessage,
                "the authentication message is malformed",
                true,
            )
            .await;
            return;
        }
    };
    let (credential, connect, cursor) = authentication;
    let authenticated = match state.authority.authenticate(&credential, connect.client_id) {
        Ok(session) => session,
        Err(error) => {
            let _ = send_control_error(
                &mut socket,
                auth_error_code(&error),
                &error.to_string(),
                true,
            )
            .await;
            return;
        }
    };
    let response = match state.service.connect(connect.clone()) {
        Ok(response) => response,
        Err(error) => {
            let _ = send_control_error(
                &mut socket,
                TransportErrorCode::Internal,
                &error.to_string(),
                true,
            )
            .await;
            return;
        }
    };
    let transcript = match state.service.transcript_page(cursor.transcript_after, 128) {
        Ok(page) => page,
        Err(error) => {
            let _ = send_control_error(
                &mut socket,
                TransportErrorCode::Internal,
                &error.to_string(),
                true,
            )
            .await;
            return;
        }
    };
    let mut after = match &response {
        ConnectResponse::Accepted { snapshot, .. } => snapshot.revision,
        ConnectResponse::Rejected { .. } => cursor.projection_revision,
    };
    if !send_control(
        &mut socket,
        &ControlServerMessage::Connected {
            session_id: authenticated.session_id.clone(),
            resumed: authenticated.resumed,
            response: response.clone(),
            transcript,
        },
    )
    .await
    {
        return;
    }
    if matches!(response, ConnectResponse::Rejected { .. }) {
        return;
    }

    let mut shutdown = state.shutdown.clone();
    let mut forced_disconnect = state.test_disconnect.subscribe();
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            _ = forced_disconnect.changed() => break,
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                match message {
                    Message::Text(text) => {
                        let message = match serde_json::from_str::<ControlClientMessage>(&text) {
                            Ok(message) => message,
                            Err(_) => {
                                if !send_control_error(&mut socket, TransportErrorCode::MalformedMessage, "control message is malformed", false).await {
                                    break;
                                }
                                continue;
                            }
                        };
                        if !handle_control_message(
                            &mut socket,
                            &state,
                            &authenticated.session_id,
                            connect.client_id,
                            message,
                        ).await {
                            break;
                        }
                    }
                    Message::Ping(payload) => {
                        if socket.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    Message::Close(_) => break,
                    Message::Binary(_) | Message::Pong(_) => {}
                }
            }
            projection = state.service.wait_for_projection(after) => {
                let Ok(projection) = projection else { break };
                after = projection.revision;
                if !send_control(
                    &mut socket,
                    &ControlServerMessage::Projection { projection: Box::new((*projection).clone()) },
                ).await {
                    break;
                }
            }
        }
    }
    let _ = state
        .telemetry
        .release_session_leases(&authenticated.session_id);
    publish_telemetry_diagnostics(&state);
}

async fn handle_control_message(
    socket: &mut WebSocket,
    state: &HostState,
    session_id: &str,
    client_id: magnolia_domain::ClientId,
    message: ControlClientMessage,
) -> bool {
    match message {
        ControlClientMessage::Authenticate { .. } => {
            send_control_error(
                socket,
                TransportErrorCode::RequestConflict,
                "the connection is already authenticated",
                false,
            )
            .await
        }
        ControlClientMessage::Command { command } => {
            if command.client_id != client_id {
                return send_control_error(
                    socket,
                    TransportErrorCode::InvalidCredential,
                    "command client ID does not match the authenticated session",
                    false,
                )
                .await;
            }
            let receipt = match state.service.dispatch(command) {
                Ok(receipt) => receipt,
                Err(error) => {
                    return send_control_error(
                        socket,
                        TransportErrorCode::Internal,
                        &error.to_string(),
                        false,
                    )
                    .await;
                }
            };
            if !send_control(
                socket,
                &ControlServerMessage::Receipt {
                    receipt: receipt.clone(),
                },
            )
            .await
            {
                return false;
            }
            if state.auto_activate
                && receipt.accepted()
                && receipt.operation_id.is_some()
                && state
                    .runtime
                    .complete_target_success(receipt.target_graph_revision)
                    .is_ok()
            {
                let _ = state.service.pump_runtime_events();
            }
            true
        }
        ControlClientMessage::SubscribeTelemetry {
            request_id,
            subscription,
        } => match state.telemetry.subscribe(session_id, subscription) {
            Ok(lease) => {
                publish_telemetry_diagnostics(state);
                send_control(
                    socket,
                    &ControlServerMessage::TelemetryLease { request_id, lease },
                )
                .await
            }
            Err(error) => send_control_request_error(socket, request_id, &error.to_string()).await,
        },
        ControlClientMessage::ReleaseTelemetry {
            request_id,
            stream_id,
        } => match state.telemetry.release(session_id, stream_id) {
            Ok(_) => {
                publish_telemetry_diagnostics(state);
                send_control(
                    socket,
                    &ControlServerMessage::TelemetryReleased {
                        request_id,
                        stream_id,
                    },
                )
                .await
            }
            Err(error) => send_control_request_error(socket, request_id, &error.to_string()).await,
        },
        ControlClientMessage::TranscriptPage {
            request_id,
            after,
            limit,
        } => match state.service.transcript_page(after, limit) {
            Ok(page) => {
                send_control(
                    socket,
                    &ControlServerMessage::TranscriptPage { request_id, page },
                )
                .await
            }
            Err(error) => send_control_request_error(socket, request_id, &error.to_string()).await,
        },
        ControlClientMessage::Ping { nonce } => {
            send_control(socket, &ControlServerMessage::Pong { nonce }).await
        }
    }
}

async fn telemetry_upgrade(
    State(state): State<HostState>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Response {
    if !origin_is_exact(&headers, &state.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    websocket
        .on_upgrade(move |socket| run_telemetry_socket(socket, state))
        .into_response()
}

async fn run_telemetry_socket(mut socket: WebSocket, state: HostState) {
    let Some(Ok(Message::Text(first))) = socket.recv().await else {
        let _ = send_telemetry_error(
            &mut socket,
            TransportErrorCode::AuthenticationRequired,
            "the first telemetry message must authenticate",
        )
        .await;
        return;
    };
    let (session_id, requested_epoch) = match serde_json::from_str::<TelemetryClientMessage>(&first)
    {
        Ok(TelemetryClientMessage::Authenticate {
            session_id,
            protocol_version,
            runtime_epoch,
        }) if protocol_version == PROTOCOL_VERSION => (session_id, runtime_epoch),
        Ok(TelemetryClientMessage::Authenticate { .. }) => {
            let _ = send_telemetry_error(
                &mut socket,
                TransportErrorCode::ProtocolRejected,
                "telemetry protocol version is unsupported",
            )
            .await;
            return;
        }
        _ => {
            let _ = send_telemetry_error(
                &mut socket,
                TransportErrorCode::MalformedMessage,
                "telemetry authentication message is malformed",
            )
            .await;
            return;
        }
    };
    if let Err(error) = state.authority.authenticate_telemetry(&session_id) {
        let _ =
            send_telemetry_error(&mut socket, auth_error_code(&error), &error.to_string()).await;
        return;
    }
    let epoch = match state.service.snapshot() {
        Ok(projection) => projection.runtime_epoch,
        Err(error) => {
            let _ = send_telemetry_error(
                &mut socket,
                TransportErrorCode::Internal,
                &error.to_string(),
            )
            .await;
            return;
        }
    };
    let _epoch_changed = requested_epoch.is_some_and(|requested| requested != epoch);
    if !send_telemetry(
        &mut socket,
        &TelemetryServerMessage::Ready {
            runtime_epoch: epoch,
        },
    )
    .await
    {
        return;
    }
    if state.telemetry.mark_connection_open().is_err() {
        return;
    }
    publish_telemetry_diagnostics(&state);

    let queue = Arc::new(BoundedTelemetryQueue::new(64));
    let (connection_shutdown, connection_rx) = watch::channel(false);
    let producer = tokio::spawn(run_telemetry_producer(
        Arc::clone(&queue),
        state.clone(),
        session_id.clone(),
        epoch,
        connection_rx,
    ));
    let (mut sender, mut receiver) = socket.split();
    let mut shutdown = state.shutdown.clone();
    let mut forced_disconnect = state.test_disconnect.subscribe();
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
            }
            _ = forced_disconnect.changed() => break,
            incoming = receiver.next() => {
                let Some(Ok(message)) = incoming else { break };
                match message {
                    Message::Text(text) => {
                        if let Ok(TelemetryClientMessage::Ping { nonce }) = serde_json::from_str(&text) {
                            let response = TelemetryServerMessage::Pong { nonce };
                            let Ok(json) = serde_json::to_string(&response) else { break };
                            if sender.send(Message::Text(json.into())).await.is_err() { break; }
                        }
                    }
                    Message::Ping(payload) => {
                        if sender.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    Message::Close(_) => break,
                    Message::Binary(_) | Message::Pong(_) => {}
                }
            }
            queued = queue.pop() => {
                let Ok(Some(frame)) = queued else { break };
                let Ok(encoded) = encode_telemetry_postcard(&frame) else { break };
                if sender.send(Message::Binary(encoded.into())).await.is_err() { break; }
            }
        }
    }
    let _ = connection_shutdown.send(true);
    let _ = queue.close();
    let _ = producer.await;
    let _ = state.telemetry.disconnect(&session_id);
    publish_telemetry_diagnostics(&state);
}

async fn run_telemetry_producer(
    queue: Arc<BoundedTelemetryQueue>,
    state: HostState,
    session_id: String,
    epoch: RuntimeEpochId,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut ticker = interval(Duration::from_millis(33));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    ticker.tick().await;
    let mut ticks_since_diagnostic_projection = 0_u8;
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
            }
            _ = ticker.tick() => {
                let Ok(frames) = state.telemetry.generate_frames(&session_id, epoch) else { break };
                let mut any_drop = false;
                for frame in frames {
                    let Ok(dropped) = queue.push(frame.envelope, frame.delivery, frame.capacity) else { return };
                    if dropped.total > 0 {
                        any_drop = true;
                        for (dropped_stream, count) in dropped.per_stream {
                            let _ = state.telemetry.record_drop(dropped_stream, count);
                        }
                    }
                }
                if any_drop {
                    ticks_since_diagnostic_projection = ticks_since_diagnostic_projection.saturating_add(1);
                    if ticks_since_diagnostic_projection >= 30 {
                        publish_telemetry_diagnostics(&state);
                        ticks_since_diagnostic_projection = 0;
                    }
                }
            }
        }
    }
}

async fn run_synthetic_transcript(
    service: NativeService,
    epoch: RuntimeEpochId,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut ticker = interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    ticker.tick().await;
    let mut sequence = 0_u64;
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { break; }
            }
            _ = ticker.tick() => {
                sequence = sequence.saturating_add(1);
                let segment = TranscriptSegment {
                    session_id: CAPTION_SESSION,
                    segment_id: EntityId::from_u128(0x2_300 + u128::from(sequence)),
                    segment_revision: 1,
                    sequence,
                    text: format!("synthetic final {sequence:02} ({})", &epoch.to_string()[..8]),
                };
                let _ = service.append_transcript(segment);
            }
        }
    }
}

fn publish_telemetry_diagnostics(state: &HostState) {
    if let Ok(status) = state.telemetry.status() {
        let _ = state.service.set_diagnostic_counters([
            (
                "telemetry.active_connections".to_owned(),
                status.active_connections,
            ),
            (
                "telemetry.active_leases".to_owned(),
                u64::try_from(status.active_leases).unwrap_or(u64::MAX),
            ),
            ("telemetry.dropped".to_owned(), status.cumulative_dropped),
            (
                "telemetry.released_leases".to_owned(),
                status.released_leases,
            ),
        ]);
    }
}

fn origin_is_exact(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == expected)
}

async fn send_control(socket: &mut WebSocket, message: &ControlServerMessage) -> bool {
    let Ok(json) = serde_json::to_string(message) else {
        return false;
    };
    socket.send(Message::Text(json.into())).await.is_ok()
}

async fn send_control_error(
    socket: &mut WebSocket,
    code: TransportErrorCode,
    message: &str,
    fatal: bool,
) -> bool {
    send_control(
        socket,
        &ControlServerMessage::Error {
            request_id: None,
            code,
            message: message.to_owned(),
            fatal,
        },
    )
    .await
}

async fn send_control_request_error(
    socket: &mut WebSocket,
    request_id: magnolia_domain::RequestId,
    message: &str,
) -> bool {
    send_control(
        socket,
        &ControlServerMessage::Error {
            request_id: Some(request_id),
            code: TransportErrorCode::RequestConflict,
            message: message.to_owned(),
            fatal: false,
        },
    )
    .await
}

async fn send_telemetry(socket: &mut WebSocket, message: &TelemetryServerMessage) -> bool {
    let Ok(json) = serde_json::to_string(message) else {
        return false;
    };
    socket.send(Message::Text(json.into())).await.is_ok()
}

async fn send_telemetry_error(
    socket: &mut WebSocket,
    code: TransportErrorCode,
    message: &str,
) -> bool {
    send_telemetry(
        socket,
        &TelemetryServerMessage::Error {
            code,
            message: message.to_owned(),
            fatal: true,
        },
    )
    .await
}

fn auth_error_code(error: &AuthError) -> TransportErrorCode {
    match error {
        AuthError::ExpiredCredential => TransportErrorCode::ExpiredCredential,
        AuthError::MalformedCredential
        | AuthError::InvalidCredential
        | AuthError::ConsumedCredential
        | AuthError::ClientMismatch => TransportErrorCode::InvalidCredential,
        AuthError::Poisoned | AuthError::Entropy(_) => TransportErrorCode::Internal,
    }
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            return;
        }
    }
}

fn random_token() -> Result<String, HostError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| HostError::Entropy(error.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[derive(Debug, Serialize)]
struct TestStatus {
    projection: magnolia_protocol::RuntimeProjection,
    pending_activations: usize,
    observed_activations: usize,
    telemetry: TelemetryStatus,
}

async fn test_status(State(state): State<HostState>, headers: HeaderMap) -> Response {
    if !test_authorized(&state, &headers) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match (state.service.snapshot(), state.telemetry.status()) {
        (Ok(projection), Ok(telemetry)) => Json(TestStatus {
            projection,
            pending_activations: state.runtime.pending_requests().len(),
            observed_activations: state.runtime.observed_requests().len(),
            telemetry,
        })
        .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeTestRequest {
    action: RuntimeTestAction,
    target_revision: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeTestAction {
    SucceedNext,
    FailNext,
    SucceedTarget,
    Pump,
}

#[derive(Debug, Serialize)]
struct RuntimeTestResponse {
    completed_target: Option<TargetGraphRevision>,
    handled: usize,
    ignored_stale: usize,
}

async fn test_runtime(
    State(state): State<HostState>,
    headers: HeaderMap,
    Json(request): Json<RuntimeTestRequest>,
) -> Response {
    if !test_authorized(&state, &headers) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let completed = match request.action {
        RuntimeTestAction::SucceedNext => state.runtime.complete_next_success(),
        RuntimeTestAction::FailNext => state
            .runtime
            .complete_next_failure("synthetic_activation", "induced activation failure"),
        RuntimeTestAction::SucceedTarget => request
            .target_revision
            .ok_or(MockRuntimeError::NoPendingActivation)
            .and_then(|target| {
                state
                    .runtime
                    .complete_target_success(TargetGraphRevision::new(target))
            }),
        RuntimeTestAction::Pump => {
            let pump = state.service.pump_runtime_events();
            return match pump {
                Ok(pump) => Json(RuntimeTestResponse {
                    completed_target: None,
                    handled: pump.handled,
                    ignored_stale: pump.ignored_stale,
                })
                .into_response(),
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
        }
    };
    let completed = match completed {
        Ok(request) => request,
        Err(error) => {
            return (StatusCode::CONFLICT, error.to_string()).into_response();
        }
    };
    match state.service.pump_runtime_events() {
        Ok(pump) => Json(RuntimeTestResponse {
            completed_target: Some(completed.target_graph_revision),
            handled: pump.handled,
            ignored_stale: pump.ignored_stale,
        })
        .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct TelemetryTestRequest {
    flood_multiplier: u32,
}

async fn test_telemetry(
    State(state): State<HostState>,
    headers: HeaderMap,
    Json(request): Json<TelemetryTestRequest>,
) -> Response {
    if !test_authorized(&state, &headers) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match state
        .telemetry
        .set_flood_multiplier(request.flood_multiplier)
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn test_disconnect(State(state): State<HostState>, headers: HeaderMap) -> Response {
    if !test_authorized(&state, &headers) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let generation = (*state.test_disconnect.borrow()).saturating_add(1);
    match state.test_disconnect.send(generation) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn test_authorized(state: &HostState, headers: &HeaderMap) -> bool {
    let Some(expected) = state.test_authority.as_deref() else {
        return false;
    };
    let Some(received) = headers
        .get(TEST_AUTHORITY_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    expected.as_bytes().ct_eq(received.as_bytes()).unwrap_u8() == 1
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error("Trunk assets are missing; expected {0}. Run scripts/run-phase-2.sh")]
    MissingAssets(PathBuf),
    #[error("could not bind the Magnolia loopback listener: {0}")]
    Bind(std::io::Error),
    #[error("refusing to serve Magnolia on non-loopback address {0}")]
    NonLoopback(SocketAddr),
    #[error(transparent)]
    Authentication(#[from] AuthError),
    #[error(transparent)]
    Application(#[from] magnolia_application::ApplicationError),
    #[error(transparent)]
    Browser(#[from] BrowserLaunchError),
    #[error(transparent)]
    Telemetry(#[from] TelemetryError),
    #[error("operating system entropy is unavailable: {0}")]
    Entropy(String),
    #[error("host task failed: {0}")]
    Task(String),
    #[error("host server failed: {0}")]
    Server(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};

    #[test]
    fn origin_check_requires_the_exact_expected_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "http://127.0.0.1:1234".parse().unwrap());
        assert!(origin_is_exact(&headers, "http://127.0.0.1:1234"));
        assert!(!origin_is_exact(&headers, "http://localhost:1234"));
        headers.remove(ORIGIN);
        assert!(!origin_is_exact(&headers, "http://127.0.0.1:1234"));
    }

    #[test]
    fn launch_url_keeps_authority_in_fragment_only() {
        let origin = "http://127.0.0.1:1234";
        let token = "secret";
        let url = format!("{origin}/#token={token}");
        assert!(!url.split('#').next().unwrap().contains(token));
        assert!(url.ends_with("#token=secret"));
        let ready = HostReadyInfo {
            origin: origin.to_owned(),
            launch_url: url,
            runtime_epoch: RuntimeEpochId::from_u128(1),
            test_authority: Some("test-secret".to_owned()),
        };
        let debug = format!("{ready:?}");
        assert!(!debug.contains(token));
        assert!(!debug.contains("test-secret"));
        assert!(debug.contains("redacted"));
    }

    #[test]
    fn mutable_routes_do_not_accept_query_credentials() {
        let request = Request::builder()
            .uri("/api/control?token=secret")
            .body(Body::empty())
            .unwrap();
        assert!(request.headers().get(ORIGIN).is_none());
        assert!(!origin_is_exact(request.headers(), "http://127.0.0.1:1234"));
    }
}
