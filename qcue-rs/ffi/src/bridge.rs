// QCue S1-R85/R86/R87 — the flutter_rust_bridge-facing wrapper.
//
// This module wires the harness to the REAL `flutter_rust_bridge` runtime (the crate compiles
// standalone — no Flutter/Dart toolchain needed). What is DEFERRED to the S4 codegen milestone is
// only the macro layer: `flutter_rust_bridge_codegen generate` expands `frb_generated_stream_sink!`
// into a `StreamSink<T>` type alias + the `#[frb]` glue in `frb_generated.rs`, and emits the Dart
// bindings. None of that is constructible without running codegen (and Flutter isn't installed),
// so here we ride the codegen-INDEPENDENT `for_generated::StreamSinkBase<T, SseCodec>` — the same
// underlying handle the generated `StreamSink<T>` wraps — through the `EnvelopeSink` seam.
//
// At S4 the only change is to re-express these `pub async fn`s with `#[frb]` taking the generated
// `StreamSink<RuntimeEventEnvelope>` (which derefs to this same base) and to add a cancel-handle
// registry; the forwarding/STT logic below is final. See ffi/README.md.

use crate::{EnvelopeSink, forward_events};
use flutter_rust_bridge::for_generated::{Rust2DartAction, SseCodec, StreamSinkBase};
use protocol::{RuntimeEventEnvelope, TranscriptionResult};
use router::stt::SttRouter;
use router::stub::StubProvider;
use std::io::Write;
use tokio_util::sync::CancellationToken;

/// The flutter_rust_bridge `StreamSink` (its base handle) typed to our canonical envelope. The
/// generated `StreamSink<RuntimeEventEnvelope>` wraps exactly this; both forward to Dart's `Stream`.
pub type FrbEnvelopeSink = StreamSinkBase<RuntimeEventEnvelope, SseCodec>;

/// Adapts the real flutter_rust_bridge sink to the codegen-independent `EnvelopeSink` seam.
///
/// Each envelope is serialized to its canonical JSON (the SAME bytes the SSE/WSS egress emits, so
/// the two egresses stay byte-identical, S1-R86) and pushed onto the Dart-facing stream. The exact
/// on-wire framing the generated Dart decoder expects is finalized by codegen in S4; this proves the
/// FRB runtime type rides the public surface and compiles today.
pub struct FrbStreamSink {
    inner: FrbEnvelopeSink,
}

impl FrbStreamSink {
    pub fn new(inner: FrbEnvelopeSink) -> Self {
        Self { inner }
    }
}

impl EnvelopeSink for FrbStreamSink {
    fn add(&self, env: RuntimeEventEnvelope) {
        // Serialize to canonical JSON. An owned envelope of plain serde types cannot fail to
        // serialize; on the impossible error path we simply drop the frame rather than panic.
        let Ok(json) = serde_json::to_vec(&env) else {
            return;
        };
        let message = SseCodec::encode(Rust2DartAction::Success, move |serializer| {
            // `cursor` is the public byte sink of the FRB serializer; writing into an in-memory
            // Vec cursor is infallible, so the result is intentionally ignored.
            let _ = serializer.cursor.write_all(&json);
        });
        // `add_raw` returns Err only once the Dart side has closed the stream; nothing to do here.
        let _ = self.inner.add_raw(message);
    }
}

/// S1-R85 — the FFI turn entry: drive the harness's stream into ANY `EnvelopeSink` (the FRB sink in
/// production, a `SinkRecorder` in tests), cancellable across the boundary (S1-R87).
///
/// At M1 the harness is the keyless/networkless `StubProvider` (the only provider that needs no DB
/// or credentials). S4 swaps the first argument for the real `Harness` + `TurnContext` without
/// touching the forwarding contract below.
pub async fn run_turn_into<S: EnvelopeSink>(
    provider: StubProvider,
    sink: S,
    cancel: CancellationToken,
) {
    forward_events(provider.stream(), sink, cancel).await;
}

/// The production FFI turn entry: forward over the real flutter_rust_bridge sink. Kept `pub` so S4's
/// `#[frb]` wrapper is a one-line delegation; not unit-tested here (no Dart isolate at M1).
pub async fn ffi_run_turn(
    provider: StubProvider,
    sink: FrbEnvelopeSink,
    cancel: CancellationToken,
) {
    run_turn_into(provider, FrbStreamSink::new(sink), cancel).await;
}

/// S1-R79..R82 — the FFI STT entry. The router NEVER raises: any failure (no provider, oversize
/// audio, network error) returns a `TranscriptionResult { success:false, error, provider }`.
pub async fn transcribe(
    router: SttRouter,
    audio: Vec<u8>,
    model: Option<String>,
    language: Option<String>,
) -> TranscriptionResult {
    router.transcribe(&audio, model.as_deref(), language.as_deref()).await
}
