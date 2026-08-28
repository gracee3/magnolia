use crate::{client::TelemetryObserverHandle, model::StudioState};
use leptos::{html, prelude::*};
use magnolia_client::ApplicationClient;
use magnolia_domain::{DeliveryPolicy, EntityId};
use magnolia_protocol::{SyntheticTelemetryPayload, TelemetrySubscription};
use send_wrapper::SendWrapper;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenseKind {
    Meter,
    Waveform,
    Spectrum,
}

impl DenseKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Meter => "Level meter",
            Self::Waveform => "Waveform",
            Self::Spectrum => "Spectrum",
        }
    }

    pub const fn test_id(self) -> &'static str {
        match self {
            Self::Meter => "meter-canvas",
            Self::Waveform => "waveform-canvas",
            Self::Spectrum => "spectrum-canvas",
        }
    }

    pub const fn delivery(self) -> DeliveryPolicy {
        match self {
            Self::Meter => DeliveryPolicy::Latest,
            Self::Waveform | Self::Spectrum => DeliveryPolicy::DropOldest,
        }
    }
}

#[derive(Default)]
struct DenseBuffer {
    payload: Option<SyntheticTelemetryPayload>,
    frames: u64,
    dropped: u64,
    discontinuities: u64,
}

#[component]
pub fn DenseCanvas(
    state: StudioState,
    stream_id: EntityId,
    kind: DenseKind,
    visible: Signal<bool>,
) -> impl IntoView {
    let canvas_ref = NodeRef::<html::Canvas>::new();
    let buffer = Rc::new(RefCell::new(DenseBuffer::default()));
    let observer = Rc::new(RefCell::new(None::<TelemetryObserverHandle>));
    let leased = Rc::new(Cell::new(false));
    let generation = Rc::new(Cell::new(0_u64));
    let desired_visible = Rc::new(Cell::new(false));
    let alive = Rc::new(Cell::new(true));
    let frame_count = RwSignal::new(0_u64);
    let dropped = RwSignal::new(0_u64);
    let discontinuities = RwSignal::new(0_u64);
    let lease_status = RwSignal::new("idle".to_owned());

    let effect_state = state.clone();
    let effect_observer = Rc::clone(&observer);
    let effect_leased = Rc::clone(&leased);
    let effect_generation = Rc::clone(&generation);
    let effect_desired = Rc::clone(&desired_visible);
    let effect_alive = Rc::clone(&alive);
    let effect_buffer = Rc::clone(&buffer);
    Effect::new(move || {
        let should_run = visible.get();
        effect_desired.set(should_run);
        let next_generation = effect_generation.get().saturating_add(1);
        effect_generation.set(next_generation);
        if should_run && !effect_leased.replace(true) {
            lease_status.set("subscribing".to_owned());
            let client = effect_state.client.clone();
            let callback_buffer = Rc::clone(&effect_buffer);
            let callback = Rc::new(
                move |envelope: magnolia_protocol::TelemetryEnvelope,
                      payload: SyntheticTelemetryPayload| {
                    let mut buffer = callback_buffer.borrow_mut();
                    buffer.payload = Some(payload);
                    buffer.frames = buffer.frames.saturating_add(1);
                    buffer.dropped = envelope.cumulative_dropped;
                    if envelope.discontinuity {
                        buffer.discontinuities = buffer.discontinuities.saturating_add(1);
                    }
                    frame_count.set(buffer.frames);
                    dropped.set(buffer.dropped);
                    discontinuities.set(buffer.discontinuities);
                },
            );
            let handle = client.observe_telemetry(stream_id, callback);
            *effect_observer.borrow_mut() = Some(handle);
            let observer_for_async = Rc::clone(&effect_observer);
            let leased_for_async = Rc::clone(&effect_leased);
            let generation_for_async = Rc::clone(&effect_generation);
            let desired_for_async = Rc::clone(&effect_desired);
            let alive_for_async = Rc::clone(&effect_alive);
            leptos::task::spawn_local(async move {
                let subscription = TelemetrySubscription {
                    stream_id,
                    requested_rate_hz: 30,
                    capacity: 8,
                    delivery: kind.delivery(),
                };
                match client.subscribe_telemetry(subscription).await {
                    Ok(_)
                        if generation_for_async.get() == next_generation
                            && desired_for_async.get() =>
                    {
                        if alive_for_async.get() {
                            lease_status.set("streaming".to_owned());
                        }
                    }
                    Ok(_) => {
                        let _ = client.release_telemetry(stream_id).await;
                        leased_for_async.set(false);
                        observer_for_async.borrow_mut().take();
                        if alive_for_async.get() {
                            lease_status.set("released".to_owned());
                        }
                    }
                    Err(error) => {
                        leased_for_async.set(false);
                        observer_for_async.borrow_mut().take();
                        if alive_for_async.get() {
                            lease_status.set(format!("error: {error}"));
                        }
                    }
                }
            });
        } else if !should_run && effect_leased.replace(false) {
            effect_observer.borrow_mut().take();
            lease_status.set("releasing".to_owned());
            let client = effect_state.client.clone();
            let alive_for_async = Rc::clone(&effect_alive);
            leptos::task::spawn_local(async move {
                let _ = client.release_telemetry(stream_id).await;
                if alive_for_async.get() {
                    lease_status.set("released".to_owned());
                }
            });
        }
    });

    let animation = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let frame_id = Rc::new(Cell::new(None::<i32>));
    let last_draw = Rc::new(Cell::new(0_f64));
    let animation_self = Rc::clone(&animation);
    let animation_frame_id = Rc::clone(&frame_id);
    let animation_last_draw = Rc::clone(&last_draw);
    let animation_buffer = Rc::clone(&buffer);
    let animation_canvas = canvas_ref;
    *animation.borrow_mut() = Some(Closure::wrap(Box::new(move |timestamp: f64| {
        if timestamp - animation_last_draw.get() >= 32.0 {
            animation_last_draw.set(timestamp);
            if let Some(canvas) = animation_canvas.get() {
                draw(&canvas, kind, &animation_buffer.borrow());
            }
        }
        if let Some(window) = web_sys::window() {
            if let Some(callback) = animation_self.borrow().as_ref() {
                if let Ok(id) = window.request_animation_frame(callback.as_ref().unchecked_ref()) {
                    animation_frame_id.set(Some(id));
                }
            }
        }
    }) as Box<dyn FnMut(f64)>));
    if let (Some(window), Some(callback)) = (web_sys::window(), animation.borrow().as_ref()) {
        if let Ok(id) = window.request_animation_frame(callback.as_ref().unchecked_ref()) {
            frame_id.set(Some(id));
        }
    }

    let cleanup_animation = Rc::clone(&animation);
    let cleanup_frame_id = Rc::clone(&frame_id);
    let cleanup_observer = Rc::clone(&observer);
    let cleanup_leased = Rc::clone(&leased);
    let cleanup_generation = Rc::clone(&generation);
    let cleanup_desired = Rc::clone(&desired_visible);
    let cleanup_alive = Rc::clone(&alive);
    let cleanup_client = state.client.clone();
    let cleanup = SendWrapper::new(move || {
        cleanup_alive.set(false);
        cleanup_desired.set(false);
        cleanup_generation.set(cleanup_generation.get().saturating_add(1));
        if let (Some(window), Some(id)) = (web_sys::window(), cleanup_frame_id.get()) {
            let _ = window.cancel_animation_frame(id);
        }
        cleanup_animation.borrow_mut().take();
        cleanup_observer.borrow_mut().take();
        if cleanup_leased.replace(false) {
            let client = cleanup_client.clone();
            leptos::task::spawn_local(async move {
                let _ = client.release_telemetry(stream_id).await;
            });
        }
    });
    on_cleanup(move || cleanup.take()());

    view! {
        <div class="dense-visual" data-stream=stream_id.to_string()>
            <canvas
                node_ref=canvas_ref
                data-testid=kind.test_id()
                aria-label=kind.label()
                width="720"
                height="260"
            ></canvas>
            <div class="telemetry-readout" aria-live="off">
                <span data-testid=format!("{}-frames", kind.test_id())>
                    {move || format!("{} frames", frame_count.get())}
                </span>
                <span data-testid=format!("{}-drops", kind.test_id())>
                    {move || format!("{} dropped", dropped.get())}
                </span>
                <span>{move || format!("{} gaps", discontinuities.get())}</span>
                <span data-testid=format!("{}-lease", kind.test_id())>
                    {move || lease_status.get()}
                </span>
            </div>
        </div>
    }
}

fn draw(canvas: &HtmlCanvasElement, kind: DenseKind, buffer: &DenseBuffer) {
    let width = u32::try_from(canvas.client_width().max(320)).unwrap_or(720);
    let height = u32::try_from(canvas.client_height().max(180)).unwrap_or(260);
    if canvas.width() != width {
        canvas.set_width(width);
    }
    if canvas.height() != height {
        canvas.set_height(height);
    }
    let Ok(Some(context)) = canvas.get_context("2d") else {
        return;
    };
    let Ok(context) = context.dyn_into::<CanvasRenderingContext2d>() else {
        return;
    };
    let width = f64::from(width);
    let height = f64::from(height);
    context.set_fill_style_str("#0a1118");
    context.fill_rect(0.0, 0.0, width, height);
    context.set_stroke_style_str("#1f3340");
    context.set_line_width(1.0);
    for line in 1..4 {
        let y = height * f64::from(line) / 4.0;
        context.begin_path();
        context.move_to(0.0, y);
        context.line_to(width, y);
        context.stroke();
    }
    let Some(payload) = &buffer.payload else {
        return;
    };
    match (kind, payload) {
        (
            DenseKind::Meter,
            SyntheticTelemetryPayload::Meter {
                level_milli,
                peak_milli,
            },
        ) => {
            let level = f64::from(*level_milli) / 1_000.0;
            let peak = f64::from(*peak_milli) / 1_000.0;
            let gradient = context.create_linear_gradient(0.0, height, width, height);
            let _ = gradient.add_color_stop(0.0, "#4cc9a7");
            let _ = gradient.add_color_stop(0.75, "#d8bd65");
            let _ = gradient.add_color_stop(1.0, "#e56f6f");
            context.set_fill_style_canvas_gradient(&gradient);
            context.fill_rect(22.0, height * (1.0 - level), width - 44.0, height * level);
            context.set_stroke_style_str("#f5f7fa");
            context.begin_path();
            context.move_to(12.0, height * (1.0 - peak));
            context.line_to(width - 12.0, height * (1.0 - peak));
            context.stroke();
        }
        (DenseKind::Waveform, SyntheticTelemetryPayload::Waveform { samples }) => {
            if samples.len() < 2 {
                return;
            }
            context.set_stroke_style_str("#6be1d2");
            context.set_line_width(2.0);
            context.begin_path();
            for (index, sample) in samples.iter().enumerate() {
                let x = width * index as f64 / (samples.len() - 1) as f64;
                let y = height / 2.0 - (f64::from(*sample) / 32_768.0) * height * 0.45;
                if index == 0 {
                    context.move_to(x, y);
                } else {
                    context.line_to(x, y);
                }
            }
            context.stroke();
        }
        (DenseKind::Spectrum, SyntheticTelemetryPayload::Spectrum { bins }) => {
            if bins.is_empty() {
                return;
            }
            let bar_width = width / bins.len() as f64;
            for (index, value) in bins.iter().enumerate() {
                let normalized = f64::from(*value) / 1_000.0;
                let hue = 168.0 + 55.0 * index as f64 / bins.len() as f64;
                context.set_fill_style_str(&format!("hsl({hue:.0} 62% 58%)"));
                context.fill_rect(
                    index as f64 * bar_width,
                    height * (1.0 - normalized),
                    (bar_width - 1.0).max(1.0),
                    height * normalized,
                );
            }
        }
        _ => {}
    }
}
