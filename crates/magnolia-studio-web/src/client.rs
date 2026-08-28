use futures::channel::oneshot;
use magnolia_client::{ApplicationClient, ClientError, ClientFuture};
use magnolia_domain::{ClientId, EntityId, ProjectionRevision, RequestId};
use magnolia_protocol::{
    decode_synthetic_payload, decode_telemetry_postcard, CommandEnvelope, CommandReceipt,
    ConnectRequest, ConnectResponse, ControlClientMessage, ControlServerMessage, ProtocolVersion,
    ReconnectCursor, RuntimeProjection, SessionCredential, SyntheticTelemetryPayload,
    TelemetryClientMessage, TelemetryEnvelope, TelemetryLease, TelemetryServerMessage,
    TelemetrySubscription, TranscriptPage, PROTOCOL_VERSION,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    rc::{Rc, Weak},
    str::FromStr,
    sync::Arc,
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{CloseEvent, ErrorEvent, Event, MessageEvent, WebSocket};

const SESSION_KEY: &str = "magnolia.session.v1";
const CLIENT_KEY: &str = "magnolia.client.v1";
const PROJECTION_KEY: &str = "magnolia.projection.v1";
const TRANSCRIPT_KEY: &str = "magnolia.transcript.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionPhase {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Rejected,
}

#[derive(Clone)]
pub struct WebSocketApplicationClient {
    state: Rc<RefCell<ClientState>>,
}

struct ClientState {
    origin: String,
    launch_token: Option<String>,
    session_id: Option<String>,
    connect_request: Option<ConnectRequest>,
    phase: ConnectionPhase,
    negotiated_version: Option<ProtocolVersion>,
    snapshot: Option<Arc<RuntimeProjection>>,
    transcript_cursor: u64,
    connect_waiters: Vec<oneshot::Sender<Result<ConnectResponse, ClientError>>>,
    snapshot_waiters: Vec<oneshot::Sender<Result<Arc<RuntimeProjection>, ClientError>>>,
    projection_waiters: Vec<ProjectionWaiter>,
    receipt_waiters: HashMap<RequestId, oneshot::Sender<Result<CommandReceipt, ClientError>>>,
    lease_waiters: HashMap<RequestId, PendingLease>,
    release_waiters: HashMap<RequestId, oneshot::Sender<Result<(), ClientError>>>,
    transcript_waiters: HashMap<RequestId, oneshot::Sender<Result<TranscriptPage, ClientError>>>,
    desired_subscriptions: BTreeMap<EntityId, TelemetrySubscription>,
    connection_observers: Vec<Rc<dyn Fn(ConnectionPhase)>>,
    telemetry_observers: BTreeMap<EntityId, Vec<TelemetryObserver>>,
    next_observer_id: u64,
    control: Option<SocketHandle>,
    telemetry: Option<SocketHandle>,
    reconnect_generation: u64,
}

struct PendingLease {
    subscription: TelemetrySubscription,
    sender: Option<oneshot::Sender<Result<TelemetryLease, ClientError>>>,
}

struct ProjectionWaiter {
    id: RequestId,
    after: ProjectionRevision,
    sender: oneshot::Sender<Result<Arc<RuntimeProjection>, ClientError>>,
}

struct ProjectionWaiterGuard {
    state: Weak<RefCell<ClientState>>,
    id: RequestId,
}

impl Drop for ProjectionWaiterGuard {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            state
                .borrow_mut()
                .projection_waiters
                .retain(|waiter| waiter.id != self.id);
        }
    }
}

struct TelemetryObserver {
    id: u64,
    callback: Rc<dyn Fn(TelemetryEnvelope, SyntheticTelemetryPayload)>,
}

struct SocketHandle {
    socket: WebSocket,
    _on_open: Closure<dyn FnMut(Event)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
}

impl Drop for SocketHandle {
    fn drop(&mut self) {
        self.socket.set_onopen(None);
        self.socket.set_onmessage(None);
        self.socket.set_onerror(None);
        self.socket.set_onclose(None);
        let _ = self.socket.close();
    }
}

impl WebSocketApplicationClient {
    pub fn from_window() -> Result<Self, ClientError> {
        let window =
            web_sys::window().ok_or_else(|| transport_error("browser window is absent"))?;
        let location = window.location();
        let origin = location
            .origin()
            .map_err(|_| transport_error("browser origin is unavailable"))?;
        let launch_token = extract_launch_token(
            &location
                .hash()
                .map_err(|_| transport_error("URL fragment is unavailable"))?,
        );
        if launch_token.is_some() {
            let clean_url = format!(
                "{}{}",
                location.pathname().unwrap_or_else(|_| "/".to_owned()),
                location.search().unwrap_or_default()
            );
            window
                .history()
                .and_then(|history| {
                    history.replace_state_with_url(&JsValue::NULL, "", Some(&clean_url))
                })
                .map_err(|_| {
                    transport_error("could not remove launch token from browser history")
                })?;
        }
        let storage = window
            .session_storage()
            .map_err(|_| transport_error("sessionStorage is unavailable"))?
            .ok_or_else(|| transport_error("sessionStorage is disabled"))?;
        let session_id = storage
            .get_item(SESSION_KEY)
            .map_err(|_| transport_error("could not read sessionStorage"))?;
        let transcript_cursor = storage
            .get_item(TRANSCRIPT_KEY)
            .ok()
            .flatten()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        Ok(Self {
            state: Rc::new(RefCell::new(ClientState {
                origin,
                launch_token,
                session_id,
                connect_request: None,
                phase: ConnectionPhase::Disconnected,
                negotiated_version: None,
                snapshot: None,
                transcript_cursor,
                connect_waiters: Vec::new(),
                snapshot_waiters: Vec::new(),
                projection_waiters: Vec::new(),
                receipt_waiters: HashMap::new(),
                lease_waiters: HashMap::new(),
                release_waiters: HashMap::new(),
                transcript_waiters: HashMap::new(),
                desired_subscriptions: BTreeMap::new(),
                connection_observers: Vec::new(),
                telemetry_observers: BTreeMap::new(),
                next_observer_id: 0,
                control: None,
                telemetry: None,
                reconnect_generation: 0,
            })),
        })
    }

    pub fn client_id_from_window() -> Result<ClientId, ClientError> {
        let window =
            web_sys::window().ok_or_else(|| transport_error("browser window is absent"))?;
        let storage = window
            .session_storage()
            .map_err(|_| transport_error("sessionStorage is unavailable"))?
            .ok_or_else(|| transport_error("sessionStorage is disabled"))?;
        if let Some(value) = storage
            .get_item(CLIENT_KEY)
            .map_err(|_| transport_error("could not read client ID"))?
        {
            return ClientId::from_str(&value)
                .map_err(|_| transport_error("stored client ID is malformed"));
        }
        let client_id = ClientId::new();
        storage
            .set_item(CLIENT_KEY, &client_id.to_string())
            .map_err(|_| transport_error("could not persist client ID"))?;
        Ok(client_id)
    }

    #[must_use]
    pub fn phase(&self) -> ConnectionPhase {
        self.state.borrow().phase
    }

    pub fn observe_connection(&self, callback: Rc<dyn Fn(ConnectionPhase)>) {
        callback(self.phase());
        self.state.borrow_mut().connection_observers.push(callback);
    }

    pub fn observe_telemetry(
        &self,
        stream_id: EntityId,
        callback: Rc<dyn Fn(TelemetryEnvelope, SyntheticTelemetryPayload)>,
    ) -> TelemetryObserverHandle {
        let id = {
            let mut state = self.state.borrow_mut();
            state.next_observer_id = state.next_observer_id.saturating_add(1);
            let id = state.next_observer_id;
            state
                .telemetry_observers
                .entry(stream_id)
                .or_default()
                .push(TelemetryObserver { id, callback });
            id
        };
        TelemetryObserverHandle {
            state: Rc::downgrade(&self.state),
            stream_id,
            id,
        }
    }

    fn open_control(&self, reconnecting: bool) -> Result<(), ClientError> {
        {
            let mut state = self.state.borrow_mut();
            if state.control.is_some() {
                return Ok(());
            }
            set_phase(
                &mut state,
                if reconnecting {
                    ConnectionPhase::Reconnecting
                } else {
                    ConnectionPhase::Connecting
                },
            );
        }
        let url = websocket_url(&self.state.borrow().origin, "/api/control")?;
        let socket = WebSocket::new(&url)
            .map_err(|_| transport_error("could not open the control WebSocket"))?;
        let weak = Rc::downgrade(&self.state);
        let open_socket = socket.clone();
        let on_open = Closure::wrap(Box::new(move |_event: Event| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let authentication = {
                let current = state.borrow();
                let Some(connect) = current.connect_request.clone() else {
                    return;
                };
                let credential = if let Some(session_id) = current.session_id.clone() {
                    SessionCredential::SessionId(session_id)
                } else if let Some(token) = current.launch_token.clone() {
                    SessionCredential::LaunchToken(token)
                } else {
                    drop(current);
                    fail_connection(
                        &state,
                        transport_error("no launch token or resumable session is available"),
                    );
                    return;
                };
                let projection_revision = current
                    .snapshot
                    .as_ref()
                    .map_or(ProjectionRevision::ZERO, |projection| projection.revision);
                let runtime_epoch = current
                    .snapshot
                    .as_ref()
                    .map(|projection| projection.runtime_epoch);
                ControlClientMessage::Authenticate {
                    credential,
                    connect,
                    cursor: ReconnectCursor {
                        runtime_epoch,
                        projection_revision,
                        transcript_after: current.transcript_cursor,
                    },
                }
            };
            if send_json(&open_socket, &authentication).is_err() {
                fail_connection(
                    &state,
                    transport_error("could not send control authentication"),
                );
            }
        }) as Box<dyn FnMut(Event)>);

        let weak = Rc::downgrade(&self.state);
        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Some(text) = event.data().as_string() else {
                fail_connection(
                    &state,
                    transport_error("control connection received a non-text frame"),
                );
                return;
            };
            match serde_json::from_str::<ControlServerMessage>(&text) {
                Ok(message) => handle_control_server_message(&state, message),
                Err(_) => fail_connection(&state, transport_error("control response is malformed")),
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        let weak = Rc::downgrade(&self.state);
        let on_error = Closure::wrap(Box::new(move |_event: ErrorEvent| {
            if let Some(state) = weak.upgrade() {
                set_phase(&mut state.borrow_mut(), ConnectionPhase::Reconnecting);
            }
        }) as Box<dyn FnMut(ErrorEvent)>);

        let weak = Rc::downgrade(&self.state);
        let on_close = Closure::wrap(Box::new(move |_event: CloseEvent| {
            schedule_control_closed(weak.clone());
        }) as Box<dyn FnMut(CloseEvent)>);

        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        self.state.borrow_mut().control = Some(SocketHandle {
            socket,
            _on_open: on_open,
            _on_message: on_message,
            _on_error: on_error,
            _on_close: on_close,
        });
        Ok(())
    }

    fn open_telemetry(&self) -> Result<(), ClientError> {
        let (origin, session_id, epoch) = {
            let state = self.state.borrow();
            if state.telemetry.is_some() || state.phase != ConnectionPhase::Connected {
                return Ok(());
            }
            (
                state.origin.clone(),
                state
                    .session_id
                    .clone()
                    .ok_or_else(|| transport_error("control session is absent"))?,
                state
                    .snapshot
                    .as_ref()
                    .map(|projection| projection.runtime_epoch),
            )
        };
        let url = websocket_url(&origin, "/api/telemetry")?;
        let socket = WebSocket::new(&url)
            .map_err(|_| transport_error("could not open the telemetry WebSocket"))?;
        socket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let open_socket = socket.clone();
        let on_open = Closure::wrap(Box::new(move |_event: Event| {
            let message = TelemetryClientMessage::Authenticate {
                session_id: session_id.clone(),
                protocol_version: PROTOCOL_VERSION,
                runtime_epoch: epoch,
            };
            let _ = send_json(&open_socket, &message);
        }) as Box<dyn FnMut(Event)>);

        let weak = Rc::downgrade(&self.state);
        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            if let Some(text) = event.data().as_string() {
                if let Ok(TelemetryServerMessage::Ready { .. }) =
                    serde_json::from_str::<TelemetryServerMessage>(&text)
                {
                    recreate_telemetry_leases(&state);
                }
                return;
            }
            let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() else {
                return;
            };
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            let Ok(envelope) = decode_telemetry_postcard(&bytes) else {
                return;
            };
            let Ok(payload) = decode_synthetic_payload(&envelope) else {
                return;
            };
            let callbacks: Vec<_> = state
                .borrow()
                .telemetry_observers
                .get(&envelope.stream_id)
                .map(|observers| {
                    observers
                        .iter()
                        .map(|observer| Rc::clone(&observer.callback))
                        .collect()
                })
                .unwrap_or_default();
            for callback in callbacks {
                callback(envelope.clone(), payload.clone());
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        let on_error =
            Closure::wrap(Box::new(move |_event: ErrorEvent| {}) as Box<dyn FnMut(ErrorEvent)>);
        let weak = Rc::downgrade(&self.state);
        let on_close = Closure::wrap(Box::new(move |_event: CloseEvent| {
            schedule_telemetry_closed(weak.clone());
        }) as Box<dyn FnMut(CloseEvent)>);

        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        self.state.borrow_mut().telemetry = Some(SocketHandle {
            socket,
            _on_open: on_open,
            _on_message: on_message,
            _on_error: on_error,
            _on_close: on_close,
        });
        Ok(())
    }

    fn send_control(&self, message: &ControlClientMessage) -> Result<(), ClientError> {
        let state = self.state.borrow();
        if state.phase != ConnectionPhase::Connected {
            return Err(transport_error("control connection is not connected"));
        }
        let handle = state
            .control
            .as_ref()
            .ok_or_else(|| transport_error("control WebSocket is absent"))?;
        send_json(&handle.socket, message)
    }
}

impl ApplicationClient for WebSocketApplicationClient {
    fn connect(&self, request: ConnectRequest) -> ClientFuture<'_, ConnectResponse> {
        Box::pin(async move {
            if let Some(snapshot) = self.state.borrow().snapshot.clone() {
                if self.state.borrow().phase == ConnectionPhase::Connected {
                    return Ok(ConnectResponse::Accepted {
                        negotiated_version: self
                            .state
                            .borrow()
                            .negotiated_version
                            .unwrap_or(PROTOCOL_VERSION),
                        snapshot: Box::new((*snapshot).clone()),
                    });
                }
            }
            let (sender, receiver) = oneshot::channel();
            {
                let mut state = self.state.borrow_mut();
                state.connect_request = Some(request);
                state.connect_waiters.push(sender);
            }
            self.open_control(false)?;
            receiver
                .await
                .map_err(|_| transport_error("connect was cancelled"))?
        })
    }

    fn snapshot(&self) -> ClientFuture<'_, Arc<RuntimeProjection>> {
        Box::pin(async move {
            if let Some(snapshot) = self.state.borrow().snapshot.clone() {
                return Ok(snapshot);
            }
            let (sender, receiver) = oneshot::channel();
            self.state.borrow_mut().snapshot_waiters.push(sender);
            receiver
                .await
                .map_err(|_| transport_error("snapshot wait was cancelled"))?
        })
    }

    fn wait_for_projection(
        &self,
        after: ProjectionRevision,
    ) -> ClientFuture<'_, Arc<RuntimeProjection>> {
        Box::pin(async move {
            if let Some(snapshot) = self.state.borrow().snapshot.clone() {
                if snapshot.revision > after {
                    return Ok(snapshot);
                }
            }
            let (sender, receiver) = oneshot::channel();
            let id = RequestId::new();
            self.state
                .borrow_mut()
                .projection_waiters
                .push(ProjectionWaiter { id, after, sender });
            let _guard = ProjectionWaiterGuard {
                state: Rc::downgrade(&self.state),
                id,
            };
            receiver
                .await
                .map_err(|_| transport_error("projection wait was cancelled"))?
        })
    }

    fn dispatch(&self, command: CommandEnvelope) -> ClientFuture<'_, CommandReceipt> {
        Box::pin(async move {
            let request_id = command.request_id;
            let (sender, receiver) = oneshot::channel();
            self.state
                .borrow_mut()
                .receipt_waiters
                .insert(request_id, sender);
            if let Err(error) = self.send_control(&ControlClientMessage::Command { command }) {
                self.state.borrow_mut().receipt_waiters.remove(&request_id);
                return Err(error);
            }
            receiver
                .await
                .map_err(|_| transport_error("receipt wait was cancelled"))?
        })
    }

    fn subscribe_telemetry(
        &self,
        subscription: TelemetrySubscription,
    ) -> ClientFuture<'_, TelemetryLease> {
        Box::pin(async move {
            let request_id = RequestId::new();
            let (sender, receiver) = oneshot::channel();
            self.state.borrow_mut().lease_waiters.insert(
                request_id,
                PendingLease {
                    subscription: subscription.clone(),
                    sender: Some(sender),
                },
            );
            if let Err(error) = self.send_control(&ControlClientMessage::SubscribeTelemetry {
                request_id,
                subscription,
            }) {
                self.state.borrow_mut().lease_waiters.remove(&request_id);
                return Err(error);
            }
            receiver
                .await
                .map_err(|_| transport_error("telemetry lease wait was cancelled"))?
        })
    }

    fn release_telemetry(&self, stream_id: EntityId) -> ClientFuture<'_, ()> {
        Box::pin(async move {
            let request_id = RequestId::new();
            let (sender, receiver) = oneshot::channel();
            self.state
                .borrow_mut()
                .release_waiters
                .insert(request_id, sender);
            if let Err(error) = self.send_control(&ControlClientMessage::ReleaseTelemetry {
                request_id,
                stream_id,
            }) {
                self.state.borrow_mut().release_waiters.remove(&request_id);
                return Err(error);
            }
            receiver
                .await
                .map_err(|_| transport_error("telemetry release wait was cancelled"))?
        })
    }

    fn transcript_page(&self, after: u64, limit: u32) -> ClientFuture<'_, TranscriptPage> {
        Box::pin(async move {
            let request_id = RequestId::new();
            let (sender, receiver) = oneshot::channel();
            self.state
                .borrow_mut()
                .transcript_waiters
                .insert(request_id, sender);
            if let Err(error) = self.send_control(&ControlClientMessage::TranscriptPage {
                request_id,
                after,
                limit,
            }) {
                self.state
                    .borrow_mut()
                    .transcript_waiters
                    .remove(&request_id);
                return Err(error);
            }
            receiver
                .await
                .map_err(|_| transport_error("transcript wait was cancelled"))?
        })
    }
}

pub struct TelemetryObserverHandle {
    state: Weak<RefCell<ClientState>>,
    stream_id: EntityId,
    id: u64,
}

impl Drop for TelemetryObserverHandle {
    fn drop(&mut self) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let mut state = state.borrow_mut();
        if let Some(observers) = state.telemetry_observers.get_mut(&self.stream_id) {
            observers.retain(|observer| observer.id != self.id);
            if observers.is_empty() {
                state.telemetry_observers.remove(&self.stream_id);
            }
        }
    }
}

fn handle_control_server_message(state: &Rc<RefCell<ClientState>>, message: ControlServerMessage) {
    match message {
        ControlServerMessage::Connected {
            session_id,
            response,
            transcript,
            ..
        } => {
            if let Some(window) = web_sys::window() {
                if let Ok(Some(storage)) = window.session_storage() {
                    let _ = storage.set_item(SESSION_KEY, &session_id);
                    if let Some(last) = transcript.segments.last() {
                        let _ = storage.set_item(TRANSCRIPT_KEY, &last.sequence.to_string());
                    }
                }
            }
            let accepted = match &response {
                ConnectResponse::Accepted {
                    negotiated_version,
                    snapshot,
                } => Some((*negotiated_version, Arc::new((**snapshot).clone()))),
                ConnectResponse::Rejected { .. } => None,
            };
            let was_accepted = accepted.is_some();
            {
                let mut current = state.borrow_mut();
                current.session_id = Some(session_id);
                current.launch_token = None;
                current.transcript_cursor = transcript
                    .segments
                    .last()
                    .map_or(current.transcript_cursor, |segment| segment.sequence);
                if let Some((version, snapshot)) = accepted {
                    current.negotiated_version = Some(version);
                    publish_snapshot(&mut current, snapshot);
                    set_phase(&mut current, ConnectionPhase::Connected);
                } else {
                    set_phase(&mut current, ConnectionPhase::Rejected);
                }
                for sender in current.connect_waiters.drain(..) {
                    let _ = sender.send(Ok(response.clone()));
                }
            }
            if was_accepted {
                let client = WebSocketApplicationClient {
                    state: Rc::clone(state),
                };
                let _ = client.open_telemetry();
            }
        }
        ControlServerMessage::Projection { projection } => {
            publish_snapshot(&mut state.borrow_mut(), Arc::new(*projection));
        }
        ControlServerMessage::Receipt { receipt } => {
            if let Some(sender) = state
                .borrow_mut()
                .receipt_waiters
                .remove(&receipt.request_id)
            {
                let _ = sender.send(Ok(receipt));
            }
        }
        ControlServerMessage::TelemetryLease { request_id, lease } => {
            let pending = state.borrow_mut().lease_waiters.remove(&request_id);
            if let Some(pending) = pending {
                state
                    .borrow_mut()
                    .desired_subscriptions
                    .insert(lease.stream_id, pending.subscription);
                if let Some(sender) = pending.sender {
                    let _ = sender.send(Ok(lease));
                }
            }
        }
        ControlServerMessage::TelemetryReleased {
            request_id,
            stream_id,
        } => {
            let mut current = state.borrow_mut();
            current.desired_subscriptions.remove(&stream_id);
            if let Some(sender) = current.release_waiters.remove(&request_id) {
                let _ = sender.send(Ok(()));
            }
        }
        ControlServerMessage::TranscriptPage { request_id, page } => {
            if let Some(last) = page.segments.last() {
                state.borrow_mut().transcript_cursor = last.sequence;
            }
            if let Some(sender) = state.borrow_mut().transcript_waiters.remove(&request_id) {
                let _ = sender.send(Ok(page));
            }
        }
        ControlServerMessage::Error {
            request_id,
            message,
            fatal,
            ..
        } => {
            let error = transport_error(message);
            if let Some(request_id) = request_id {
                fail_request(state, request_id, error);
            } else if fatal {
                fail_connection(state, error);
            }
        }
        ControlServerMessage::Pong { .. } => {}
    }
}

fn publish_snapshot(state: &mut ClientState, snapshot: Arc<RuntimeProjection>) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.session_storage() {
            let _ = storage.set_item(PROJECTION_KEY, &snapshot.revision.to_string());
        }
    }
    state.snapshot = Some(Arc::clone(&snapshot));
    for sender in state.snapshot_waiters.drain(..) {
        let _ = sender.send(Ok(Arc::clone(&snapshot)));
    }
    let mut remaining = Vec::new();
    for waiter in state.projection_waiters.drain(..) {
        if snapshot.revision > waiter.after {
            let _ = waiter.sender.send(Ok(Arc::clone(&snapshot)));
        } else if !waiter.sender.is_canceled() {
            remaining.push(waiter);
        }
    }
    state.projection_waiters = remaining;
}

fn fail_request(state: &Rc<RefCell<ClientState>>, request_id: RequestId, error: ClientError) {
    let mut state = state.borrow_mut();
    if let Some(sender) = state.receipt_waiters.remove(&request_id) {
        let _ = sender.send(Err(error));
        return;
    }
    if let Some(pending) = state.lease_waiters.remove(&request_id) {
        if let Some(sender) = pending.sender {
            let _ = sender.send(Err(error));
        }
        return;
    }
    if let Some(sender) = state.release_waiters.remove(&request_id) {
        let _ = sender.send(Err(error));
        return;
    }
    if let Some(sender) = state.transcript_waiters.remove(&request_id) {
        let _ = sender.send(Err(error));
    }
}

fn fail_connection(state: &Rc<RefCell<ClientState>>, error: ClientError) {
    let mut state = state.borrow_mut();
    set_phase(&mut state, ConnectionPhase::Rejected);
    for sender in state.connect_waiters.drain(..) {
        let _ = sender.send(Err(error.clone()));
    }
    for sender in state.snapshot_waiters.drain(..) {
        let _ = sender.send(Err(error.clone()));
    }
    for waiter in state.projection_waiters.drain(..) {
        let _ = waiter.sender.send(Err(error.clone()));
    }
    for (_, sender) in state.receipt_waiters.drain() {
        let _ = sender.send(Err(error.clone()));
    }
    for (_, pending) in state.lease_waiters.drain() {
        if let Some(sender) = pending.sender {
            let _ = sender.send(Err(error.clone()));
        }
    }
    for (_, sender) in state.release_waiters.drain() {
        let _ = sender.send(Err(error.clone()));
    }
    for (_, sender) in state.transcript_waiters.drain() {
        let _ = sender.send(Err(error.clone()));
    }
}

fn schedule_control_closed(weak: Weak<RefCell<ClientState>>) {
    wasm_bindgen_futures::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        let Some(state) = weak.upgrade() else {
            return;
        };
        control_closed(&state);
    });
}

fn control_closed(state: &Rc<RefCell<ClientState>>) {
    let generation = {
        let mut state = state.borrow_mut();
        state.control = None;
        state.telemetry = None;
        if state.phase == ConnectionPhase::Rejected {
            return;
        }
        set_phase(&mut state, ConnectionPhase::Reconnecting);
        let disconnected = transport_error(
            "control connection closed before the in-flight request completed; retry is safe",
        );
        for (_, sender) in state.receipt_waiters.drain() {
            let _ = sender.send(Err(disconnected.clone()));
        }
        for (_, pending) in state.lease_waiters.drain() {
            if let Some(sender) = pending.sender {
                let _ = sender.send(Err(disconnected.clone()));
            }
        }
        for (_, sender) in state.release_waiters.drain() {
            let _ = sender.send(Err(disconnected.clone()));
        }
        for (_, sender) in state.transcript_waiters.drain() {
            let _ = sender.send(Err(disconnected.clone()));
        }
        state.reconnect_generation = state.reconnect_generation.saturating_add(1);
        state.reconnect_generation
    };
    schedule_control_reconnect(state, generation);
}

fn schedule_control_reconnect(state: &Rc<RefCell<ClientState>>, generation: u64) {
    let weak = Rc::downgrade(state);
    wasm_bindgen_futures::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(250).await;
        let Some(state) = weak.upgrade() else {
            return;
        };
        if state.borrow().reconnect_generation != generation
            || state.borrow().phase == ConnectionPhase::Rejected
        {
            return;
        }
        let client = WebSocketApplicationClient { state };
        if client.open_control(true).is_err() {
            let next = {
                let mut state = client.state.borrow_mut();
                state.reconnect_generation = state.reconnect_generation.saturating_add(1);
                state.reconnect_generation
            };
            schedule_control_reconnect(&client.state, next);
        }
    });
}

fn schedule_telemetry_closed(weak: Weak<RefCell<ClientState>>) {
    wasm_bindgen_futures::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        let Some(state) = weak.upgrade() else {
            return;
        };
        state.borrow_mut().telemetry = None;
        if state.borrow().phase == ConnectionPhase::Connected {
            schedule_telemetry_reconnect(&state);
        }
    });
}

fn schedule_telemetry_reconnect(state: &Rc<RefCell<ClientState>>) {
    let weak = Rc::downgrade(state);
    wasm_bindgen_futures::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(250).await;
        let Some(state) = weak.upgrade() else {
            return;
        };
        let client = WebSocketApplicationClient { state };
        let _ = client.open_telemetry();
    });
}

fn recreate_telemetry_leases(state: &Rc<RefCell<ClientState>>) {
    let (socket, subscriptions) = {
        let state = state.borrow();
        let Some(control) = state.control.as_ref() else {
            return;
        };
        (
            control.socket.clone(),
            state
                .desired_subscriptions
                .values()
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    for subscription in subscriptions {
        let request_id = RequestId::new();
        state.borrow_mut().lease_waiters.insert(
            request_id,
            PendingLease {
                subscription: subscription.clone(),
                sender: None,
            },
        );
        let message = ControlClientMessage::SubscribeTelemetry {
            request_id,
            subscription,
        };
        let _ = send_json(&socket, &message);
    }
}

fn set_phase(state: &mut ClientState, phase: ConnectionPhase) {
    if state.phase == phase {
        return;
    }
    state.phase = phase;
    let observers = state.connection_observers.clone();
    for observer in observers {
        observer(phase);
    }
}

fn extract_launch_token(fragment: &str) -> Option<String> {
    fragment
        .strip_prefix("#token=")
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn websocket_url(origin: &str, path: &str) -> Result<String, ClientError> {
    if let Some(rest) = origin.strip_prefix("http://") {
        return Ok(format!("ws://{rest}{path}"));
    }
    if let Some(rest) = origin.strip_prefix("https://") {
        return Ok(format!("wss://{rest}{path}"));
    }
    Err(transport_error("browser origin is not HTTP(S)"))
}

fn send_json<T: serde::Serialize>(socket: &WebSocket, value: &T) -> Result<(), ClientError> {
    let json = serde_json::to_string(value)
        .map_err(|error| transport_error(format!("could not encode transport message: {error}")))?;
    socket
        .send_with_str(&json)
        .map_err(|_| transport_error("WebSocket send failed"))
}

fn transport_error(message: impl Into<String>) -> ClientError {
    ClientError::Service(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_parser_accepts_only_the_dedicated_fragment_shape() {
        assert_eq!(extract_launch_token("#token=abc"), Some("abc".to_owned()));
        assert_eq!(extract_launch_token("#other=abc"), None);
        assert_eq!(extract_launch_token("#token="), None);
    }

    #[test]
    fn websocket_urls_preserve_the_exact_http_origin() {
        assert_eq!(
            websocket_url("http://127.0.0.1:1234", "/api/control").unwrap(),
            "ws://127.0.0.1:1234/api/control"
        );
        assert!(websocket_url("file://local", "/api/control").is_err());
    }
}
