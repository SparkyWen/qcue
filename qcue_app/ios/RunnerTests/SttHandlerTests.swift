import XCTest

@testable import Runner

/// QCue S5-R3/R18 — the iOS STT bridge guards the schema version and answers
/// isAvailable without starting the mic. The full recognition path (partials →
/// final, on-device flag) is verified on-device against `SFSpeechRecognizer`.
///
/// (native-unverified-here: requires Xcode + the Speech entitlement; not run on the
/// Linux CI host.)
final class SttHandlerTests: XCTestCase {

    func testUnknownSchemaVersionRejected() {
        let handler = SttHandler()
        let done = expectation(description: "reject")
        let call = FlutterMethodCall(methodName: "isAvailable", arguments: ["schemaVersion": 999])
        handler.testHandle(call) { result in
            XCTAssertEqual((result as? FlutterError)?.code, "versionMismatch")
            done.fulfill()
        }
        wait(for: [done], timeout: 2)
    }

    func testIsAvailableReturnsBool() {
        let handler = SttHandler()
        let done = expectation(description: "avail")
        let call = FlutterMethodCall(
            methodName: "isAvailable",
            arguments: ["schemaVersion": QcueChannels.schemaVersion, "localeTag": "en-US"]
        )
        handler.testHandle(call) { result in
            XCTAssertTrue(result is Bool)
            done.fulfill()
        }
        wait(for: [done], timeout: 2)
    }
}
