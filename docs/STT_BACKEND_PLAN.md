# Magnolia STT Backend Plan

Status: accepted direction; implementation pending  
Last reviewed: 2026-07-21  
Target machine: Lenovo ThinkPad T14, Intel Core i5-1145G7, Iris Xe, 16 GB RAM, no NVIDIA CUDA GPU

## Decision summary

Replace the TensorRT/Parakeet-specific speech-to-text implementation with a backend-neutral STT module. Preserve Magnolia's existing event contract so the patch bay, caption UI, and downstream modules do not need to know which recognizer is active.

Implement providers in this order:

1. `LocalSherpa`: Sherpa-ONNX streaming recognizer using a CPU/int8 English Zipformer model.
2. `OpenAiRealtime`: OpenAI Realtime transcription using `gpt-realtime-whisper` over WebSocket.

The initial/default backend for the T14 should be `LocalSherpa`. It is a true streaming recognizer and should provide more responsive partial captions at lower CPU cost than repeatedly decoding overlapping Whisper windows. Accuracy and latency must be measured on this machine before making it the permanent default.

`OpenAiRealtime` is the first fallback when local recognition is unavailable, unhealthy, or explicitly disabled. Automatic fallback must be opt-in because it sends microphone audio off-device and can incur cost. `LocalWhisper`, Google Chirp, and other providers are deferred evaluation candidates, not part of the initial implementation.

## Current repository state

The existing `crates/parakeet_stt` crate is tightly coupled to a sibling TensorRT repository through these path dependencies:

```toml
features = { path = "../../../trt-asr-engine/rust/features" }
parakeet_trt = { path = "../../../trt-asr-engine/rust/parakeet_trt" }
```

This prevents Cargo from loading the Magnolia workspace when the sibling checkout is absent. The root workspace uses `crates/*` and `apps/*`, so even `cargo run -p daemon` must parse the Parakeet manifest.

Parakeet is currently consumed by:

- `apps/asr_test`
- `apps/parakeet_stt_demo`

The daemon does not currently depend on or register `parakeet_stt`. Adding live captions to the daemon is therefore an explicit integration step, not just a dependency replacement.

Before removing the TensorRT code, retain the backend-neutral parts of `crates/parakeet_stt/CONTRACT.md` and its output convention:

- `stt_partial`
- `stt_final`
- `stt_error`
- `stt_metrics`
- related endpoint, reset, acknowledgement, and dropped-audio events where useful

During the transition, event payloads can remain extensible JSON carried in `Signal::Computed`. The Magnolia refoundation plan replaces this with a versioned STT event schema; new code should not deepen the dependency on the global `Signal` enum.

The TensorRT/Parakeet crate, demos, hard-coded sibling paths, CUDA telemetry, and TensorRT-specific configuration should then be removed from Magnolia. The standalone `trt-asr-engine` repository remains available as research history but is not an active Magnolia dependency or fallback.

## Proposed architecture

```text
PipeWire audio input
        |
        v
resample/downmix to backend format
        |
        v
bounded lock-free or non-blocking audio queue
        |
        v
SttProcessor + selected SttBackend
        |
        +-- LocalSherpa (default)
        `-- OpenAiRealtime (explicit fallback)
        |
        v
stt_partial / stt_final / stt_error / stt_metrics
        |
        v
Magnolia patch bay and caption tile
```

Suggested core interface:

```rust
pub trait SttBackend: Send {
    fn start(&mut self, session: &SttSessionConfig) -> anyhow::Result<()>;
    fn push_audio(&mut self, audio: SttAudioChunk) -> anyhow::Result<()>;
    fn finish_utterance(&mut self) -> anyhow::Result<()>;
    fn reset(&mut self) -> anyhow::Result<()>;
    fn poll_events(&mut self, output: &mut Vec<SttEvent>) -> anyhow::Result<()>;
    fn shutdown(&mut self);
}
```

Keep inference and network work off the PipeWire callback. The callback should only publish audio into a bounded queue. When overloaded, discard or coalesce replaceable partial work while retaining final/error/control events.

## Local backend choices

### Sherpa-ONNX: recommended first backend

Sherpa-ONNX has a Rust API with `OnlineRecognizer`, local CPU execution, endpoint detection, and streaming transducer models. An int8 English Zipformer model is the most promising first benchmark for this T14.

Expected advantages:

- True online decoding rather than periodic full-window retranscription.
- Fast partial updates suitable for visible live captions.
- Runs offline and does not send microphone audio elsewhere.
- CPU/int8 path fits the T14 better than CUDA/TensorRT.
- Native Rust-facing API is available.

Risks:

- Dictation accuracy may be lower than Whisper or cloud models.
- Model packaging and native library distribution need deliberate handling.
- Proper-noun biasing and punctuation need evaluation.

Reference: <https://docs.rs/sherpa-onnx/latest/sherpa_onnx/>

### Deferred: whisper.cpp local quality mode

Whisper.cpp is appropriate when local accuracy matters more than the earliest possible partial result. Start evaluation with quantized `base.en`; test `small.en` only if its real-time factor and battery cost remain acceptable.

Whisper.cpp's microphone example performs repeated/sliding-window transcription. It can produce live-looking captions, but this is different from a stateful streaming transducer and may revise text more aggressively.

Reference: <https://github.com/ggml-org/whisper.cpp/tree/master/examples/stream>

### Vosk and similarly small recognizers

Vosk is lightweight and easy to run on CPU, but it should not be the primary dictation backend unless testing shows acceptable accuracy. It is better suited to constrained commands or very low-resource operation.

If “Vox” refers to Voxtral, it is not the preferred T14-local target. If it refers to Vosk, treat it as the low-resource/low-accuracy option.

## Cloud backends

### Deferred: Google Cloud Speech-to-Text V2 / Chirp 3

Google is a strong quality-oriented cloud backend. Chirp 3 supports `StreamingRecognize`, automatic punctuation, speech adaptation for domain vocabulary, language detection, and endpointing controls. Streaming recognition is exposed through gRPC.

Tradeoffs:

- Network latency and connectivity become part of the audio path.
- Requires Google Cloud project setup, billing, IAM, and credentials.
- Microphone audio leaves the machine.
- A long-running stream needs reconnect/session rollover behavior.

References:

- <https://docs.cloud.google.com/speech-to-text/docs/streaming-recognize>
- <https://docs.cloud.google.com/speech-to-text/docs/models/chirp-3>

### OpenAI Realtime transcription

OpenAI now provides live transcription rather than only file-oriented Whisper requests. `gpt-realtime-whisper` streams transcript deltas as audio arrives. For a native/server-side Rust application, use a Realtime WebSocket transcription session. The documented PCM path uses 24 kHz mono PCM.

Do not use `whisper-1` file uploads as the live-caption path. File-oriented transcription streaming and realtime microphone transcription are distinct APIs.

Tradeoffs:

- WebSocket JSON/audio protocol is straightforward to implement in Rust.
- Requires an API key, billing, reconnect logic, and secret handling.
- Microphone audio leaves the machine.
- Accuracy, partial stability, latency, and cost must be benchmarked against Google.

References:

- <https://developers.openai.com/api/docs/guides/realtime-transcription>
- <https://developers.openai.com/api/docs/guides/speech-to-text>

## Implementation phases

### Phase 1: remove TensorRT and extract the provider-neutral contract

- Create a backend-neutral crate, preferably `crates/speech_to_text`.
- Move or recreate generic `SttEvent`, state, queueing, partial/final priority, session control, metrics, resampling, and VAD-facing behavior there.
- Do not copy Parakeet/TensorRT session code or CUDA telemetry.
- Keep `schema_version: 1` and existing `Signal::Computed` source names where compatible.
- Add mock-backend unit tests for ordering, reset, endpoint, backpressure, and final-event retention.
- Remove `crates/parakeet_stt` and `apps/parakeet_stt_demo` after copying only backend-neutral contract and test concepts.
- Remove all sibling `trt-asr-engine` path dependencies, TensorRT/CUDA settings, and Parakeet-specific daemon documentation from Magnolia.
- Generalize useful `apps/asr_test` behavior under the new contract or remove the app if retaining it would slow the initial implementation.
- Confirm the workspace builds with no TensorRT repository, CUDA toolkit, or NVIDIA device present.

### Phase 2: implement and benchmark LocalSherpa

- Add a `LocalSherpaBackend` behind a Cargo feature if native packaging requires it.
- Use 16 kHz mono float audio at the backend boundary unless the selected model specifies otherwise.
- Start with greedy decoding and one inference thread, then test two threads.
- Emit partials only when text changes; rate-limit UI updates if necessary.
- Use recognizer endpoint events to produce stable finals and reset the stream.
- Package model paths in configuration rather than hard-coded home-directory paths.

### Phase 3: wire live captions into the daemon

- Register the STT processor in the daemon patch bay.
- Connect `audio_input` or the post-DSP stream to `speech_to_text`.
- Add a monitor tile that distinguishes provisional text from committed text.
- Make the backend selectable per tile/module configuration.
- Show offline/connecting/streaming/error state without blocking the render loop.

### Phase 4: add the OpenAI fallback

- Add OpenAI Realtime transcription as the sole initial cloud provider.
- Never commit credentials. Resolve them from environment variables or an OS credential store.
- Normalize provider-specific events into the same Magnolia event schema.
- Preserve the local backend as a functional offline fallback.
- Require explicit user configuration before sending microphone audio to OpenAI; do not silently fail over to cloud.
- Defer Google Chirp, local Whisper, and other providers until benchmarks identify a concrete need.

### Phase 5: replace the ASR harness

- Generalize `apps/asr_test` to accept `--engine local-sherpa` or `openai-realtime`.
- Remove Parakeet-only CLI settings and CUDA telemetry from the common path.
- Retain WER scoring, first-partial time, first-final time, real-time pacing, and backpressure tests.
- Keep provider-specific diagnostics in backend-specific modules.

## Benchmark and acceptance criteria

Do not choose a default from reputation alone. Test each backend against:

- LibriSpeech for a reproducible baseline.
- A small personal dictation corpus recorded using the T14 microphone.
- Magnolia/project vocabulary and proper nouns.
- Quiet-room and moderate-background-noise samples.

Record:

- Word error rate.
- Time to first non-empty partial.
- Time from end of speech to stable final.
- Partial revision/churn rate.
- Real-time factor.
- CPU utilization and peak resident memory.
- Audio queue drops.
- Battery discharge rate during a fixed-duration run.
- Cloud cost per hour and reconnect/error rate for hosted providers.

Initial performance goals, to be validated rather than assumed:

- No audio work or blocking locks in the PipeWire callback.
- No dropped audio during normal dictation.
- Partial captions feel interactive.
- Final text appears shortly after endpoint detection.
- Local operation remains comfortable on battery without sustained maximum CPU frequency.
- The daemon remains responsive at 60 FPS while STT is active.

## Configuration sketch

```toml
[speech_to_text]
backend = "local_sherpa"
language = "en-US"
model_dir = "models/sherpa-onnx-streaming-zipformer-en"
num_threads = 1
emit_partials = true
endpointing = true

[speech_to_text.openai]
model = "gpt-realtime-whisper"
fallback_enabled = false
```

Environment variables or an OS credential store should carry secrets, beginning with `OPENAI_API_KEY`. Configuration and logs must never print secret values.

## First concrete task for the next agent

Implement Phase 1 and a compile-tested `LocalSherpaBackend` skeleton:

1. Create `crates/speech_to_text` with provider-neutral events and a backend trait.
2. Port only the generic queue/backpressure/session behavior from `parakeet_stt`.
3. Add a mock backend and tests before introducing the native Sherpa dependency.
4. Add the Sherpa backend behind a feature and compile it on the T14.
5. Download no model until its license, size, expected memory use, and exact source URL are recorded.
6. Leave the OpenAI provider as an interface/stub until the local path emits real partial and final events.

The intended outcome is a working offline streaming caption path on the T14, with OpenAI Realtime available as an explicitly enabled fallback rather than an architectural dependency.
