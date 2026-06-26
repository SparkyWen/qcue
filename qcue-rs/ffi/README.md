# `ffi` — Flutter surface for the QCue harness

This crate is the FFI egress (S1-R85…R88): it forwards the harness's normalized
`StreamEvent` stream to Flutter as a sequence of forward-compatible
`protocol::RuntimeEventEnvelope`s, exposes the STT entry, and threads a
`tokio_util::sync::CancellationToken` so the Dart UI's stop button crosses the boundary.

## Layout

- `src/lib.rs` — the **pure, unit-tested core**: `envelope_for`, `forward_events`,
  `is_known_event_v1`, the `EnvelopeSink` seam, and the `Vec`-backed `SinkRecorder`.
- `src/bridge.rs` — the **flutter_rust_bridge-facing wrapper**. It wires the harness to the
  real `flutter_rust_bridge` runtime (`for_generated::StreamSinkBase<RuntimeEventEnvelope, SseCodec>`)
  via `FrbStreamSink`, and exposes `run_turn_into` / `ffi_run_turn` / `transcribe`.

## S4 codegen step (deferred — Flutter/Dart is not installed here)

The `flutter_rust_bridge` **runtime** crate compiles standalone (this crate builds and is
tested without Flutter). What is deferred is the codegen layer: the `StreamSink<T>` type alias
and the `#[frb]` glue are emitted by `flutter_rust_bridge_codegen` into `frb_generated.rs`, and
the Dart bindings are produced alongside.

When Flutter is installed in S4:

```sh
flutter_rust_bridge_codegen generate
```

This expands `frb_generated_stream_sink!` into `StreamSink<RuntimeEventEnvelope>` (which wraps the
same `StreamSinkBase` this crate already uses) and emits the Dart bindings + glue. The only change
in this crate at S4 is to re-express the `pub async fn`s in `bridge.rs` with `#[frb]` taking the
generated `StreamSink<RuntimeEventEnvelope>` and to add a per-turn cancel-handle registry; the
forwarding/STT logic is final.

## FRB version

Pinned to `flutter_rust_bridge = "2.12"` (latest stable at authoring time; the `2.13.0-beta.1`
line was avoided to stay off pre-release).
