import AVFoundation
import Flutter
import Foundation
import Speech

/// QCue S5-R18/R19/R21 — the thin iOS on-device STT bridge.
///
/// Marshals `SFSpeechRecognizer` + `AVAudioEngine` across `qcue/stt` (method) +
/// `qcue/stt/events` (event). Prefers on-device recognition
/// (`requiresOnDeviceRecognition`, iOS 13+) per D4; partials are display-only
/// (S5-R19); the assembled final is the canonical body. NO business logic lives
/// here — escalation policy / persistence are in the shared Rust core (S5-R1).
///
/// NOTE (native-unverified-here): code-complete for the iOS toolchain; NOT compiled
/// or unit-run on the Linux CI host (no Xcode). An XCTest file accompanies it.
final class SttHandler: NSObject, FlutterStreamHandler {

    private let audioEngine = AVAudioEngine()
    private var recognizer: SFSpeechRecognizer?
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private var sink: FlutterEventSink?
    private var captureId: String = ""

    static func register(messenger: FlutterBinaryMessenger) -> SttHandler {
        let handler = SttHandler()
        let method = FlutterMethodChannel(name: QcueChannels.stt, binaryMessenger: messenger)
        let events = FlutterEventChannel(name: QcueChannels.sttEvents, binaryMessenger: messenger)
        method.setMethodCallHandler { [weak handler] call, result in
            handler?.handle(call, result: result)
        }
        events.setStreamHandler(handler)
        return handler
    }

    // MARK: - MethodChannel

    #if DEBUG
    /// Test-only entry point so XCTest can exercise `handle` without a live channel.
    func testHandle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        handle(call, result: result)
    }
    #endif

    private func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        guard QcueChannels.versionOk(call.arguments) else {
            result(typedError("versionMismatch", "unsupported schemaVersion"))
            return
        }
        let args = call.arguments as? [String: Any] ?? [:]
        switch call.method {
        case "isAvailable":
            let locale = args["localeTag"] as? String
            result(isAvailable(localeTag: locale))
        case "requestPermission":
            requestPermission { status in result(status) }
        case "start":
            captureId = args["captureId"] as? String ?? ""
            let locale = args["localeTag"] as? String
            let partials = args["partialResults"] as? Bool ?? true
            do {
                try start(localeTag: locale, partials: partials)
                result(nil)
            } catch {
                result(typedError("osError", "\(error)"))
            }
        case "stop":
            stop()
            result(nil)
        case "cancel":
            cancel()
            result(nil)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    private func isAvailable(localeTag: String?) -> Bool {
        let rec: SFSpeechRecognizer?
        if let tag = localeTag {
            rec = SFSpeechRecognizer(locale: Locale(identifier: tag))
        } else {
            rec = SFSpeechRecognizer()
        }
        return rec?.isAvailable ?? false
    }

    /// Requests BOTH speech + microphone authorization (S5-R18) and maps to the
    /// QPermStatus token Dart expects.
    private func requestPermission(_ done: @escaping (String) -> Void) {
        SFSpeechRecognizer.requestAuthorization { speechAuth in
            AVAudioSession.sharedInstance().requestRecordPermission { micGranted in
                DispatchQueue.main.async {
                    if !micGranted { done("denied"); return }
                    switch speechAuth {
                    case .authorized: done("granted")
                    case .denied: done("denied")
                    case .restricted: done("restricted")
                    case .notDetermined: done("notDetermined")
                    @unknown default: done("denied")
                    }
                }
            }
        }
    }

    private func start(localeTag: String?, partials: Bool) throws {
        cancel() // never double-open the mic (S5-R20)

        let locale = localeTag.map { Locale(identifier: $0) } ?? Locale.current
        guard let rec = SFSpeechRecognizer(locale: locale), rec.isAvailable else {
            // Parity with Android SttPlugin: no device recognizer at all →
            // "unavailable"; a working device recognizer but not for this locale →
            // "unsupportedLocale". (Android only checks the global probe and emits
            // "unavailable"; this keeps that for the no-recognizer case.)
            let deviceHasStt = SFSpeechRecognizer()?.isAvailable ?? false
            emitError(
                kind: deviceHasStt ? "unsupportedLocale" : "unavailable",
                message: deviceHasStt
                    ? "no recognizer for \(locale.identifier)"
                    : "no recognizer on device"
            )
            return
        }
        recognizer = rec

        let audioSession = AVAudioSession.sharedInstance()
        try audioSession.setCategory(.record, mode: .measurement, options: .duckOthers)
        try audioSession.setActive(true, options: .notifyOthersOnDeactivation)

        let req = SFSpeechAudioBufferRecognitionRequest()
        req.shouldReportPartialResults = partials
        // Prefer fully offline recognition where the device supports it (D4).
        if #available(iOS 13.0, *), rec.supportsOnDeviceRecognition {
            req.requiresOnDeviceRecognition = true
        }
        request = req

        let inputNode = audioEngine.inputNode
        let format = inputNode.outputFormat(forBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            self?.request?.append(buffer)
        }
        audioEngine.prepare()
        try audioEngine.start()

        let onDevice = req.requiresOnDeviceRecognition
        let cid = captureId
        task = rec.recognitionTask(with: req) { [weak self] result, error in
            guard let self = self else { return }
            if let result = result {
                let text = result.bestTranscription.formattedString
                if result.isFinal {
                    self.emit([
                        "event": "final",
                        "captureId": cid,
                        "transcript": text,
                        "onDevice": onDevice,
                        "confidence": self.confidence(result),
                        "localeTag": locale.identifier,
                        // Parity with Android SttPlugin.onResults, which emits audioMillis=0;
                        // the S5-R21 LongAudio escalation gate is measured by the recorder
                        // path, not derived from the recognizer transcript here.
                        "audioMillis": 0,
                        "reason": "completed",
                    ])
                    self.teardown()
                } else {
                    // S5-R19: partials are display-only.
                    self.emit(["event": "partial", "captureId": cid, "text": text])
                }
            }
            if let error = error {
                self.emitError(kind: "osError", message: "\(error)")
                self.teardown()
            }
        }
    }

    private func confidence(_ result: SFSpeechRecognitionResult) -> Double? {
        let segs = result.bestTranscription.segments
        guard !segs.isEmpty else { return nil }
        let avg = segs.map { Double($0.confidence) }.reduce(0, +) / Double(segs.count)
        return avg
    }

    private func stop() {
        // Graceful stop → the recognizer finalizes and emits SttFinal.
        audioEngine.inputNode.removeTap(onBus: 0)
        audioEngine.stop()
        request?.endAudio()
    }

    private func cancel() {
        task?.cancel()
        teardown()
    }

    private func teardown() {
        if audioEngine.isRunning {
            audioEngine.inputNode.removeTap(onBus: 0)
            audioEngine.stop()
        }
        request = nil
        task = nil
        // Release the audio session we activated in start() (.record/.measurement). Leaving it active
        // would keep ducking other audio and — if the record-package voice path runs afterwards —
        // leave a stale .measurement session/route owned by us. Best-effort: a failed deactivation
        // (e.g. session already inactive) is harmless.
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    // MARK: - EventChannel

    func onListen(withArguments _: Any?, eventSink events: @escaping FlutterEventSink) -> FlutterError? {
        sink = events
        emit([
            "event": "avail",
            "onDeviceAvailable": SFSpeechRecognizer()?.supportsOnDeviceRecognition ?? false,
            "supportedLocales": SFSpeechRecognizer.supportedLocales().map { $0.identifier },
        ])
        return nil
    }

    func onCancel(withArguments _: Any?) -> FlutterError? {
        sink = nil
        return nil
    }

    private func emit(_ map: [String: Any?]) {
        DispatchQueue.main.async { [weak self] in
            self?.sink?(map.compactMapValues { $0 })
        }
    }

    private func emitError(kind: String, message: String) {
        emit(["event": "error", "captureId": captureId, "kind": kind, "message": message])
    }

    private func typedError(_ kind: String, _ message: String) -> FlutterError {
        FlutterError(code: kind, message: message, details: ["kind": kind, "retryable": false])
    }
}
