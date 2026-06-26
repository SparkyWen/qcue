import XCTest

@testable import Runner

/// QCue S5-R3/R33 — the iOS notif bridge guards the schema version and accepts the
/// three closed kinds + the push-token stub. (native-unverified-here.)
final class NotifHandlerTests: XCTestCase {

    func testUnknownSchemaVersionRejected() {
        let handler = NotifHandler()
        let done = expectation(description: "reject")
        let call = FlutterMethodCall(methodName: "show", arguments: ["schemaVersion": 999])
        handler.testHandle(call) { result in
            XCTAssertEqual((result as? FlutterError)?.code, "versionMismatch")
            done.fulfill()
        }
        wait(for: [done], timeout: 2)
    }

    func testShowDreamCompleteSucceeds() {
        let handler = NotifHandler()
        let done = expectation(description: "show")
        let call = FlutterMethodCall(methodName: "show", arguments: [
            "schemaVersion": QcueChannels.schemaVersion,
            "kind": "dreamComplete",
            "title": "QCue improved 3 pages",
            "body": "tap",
            "route": ["id": "job-1"],
        ])
        handler.testHandle(call) { _ in done.fulfill() }
        wait(for: [done], timeout: 2)
    }

    func testRegisterPushTokenIsStub() {
        let handler = NotifHandler()
        let done = expectation(description: "stub")
        let call = FlutterMethodCall(
            methodName: "registerPushToken",
            arguments: ["schemaVersion": QcueChannels.schemaVersion]
        )
        handler.testHandle(call) { result in
            XCTAssertNil(result)
            done.fulfill()
        }
        wait(for: [done], timeout: 2)
    }
}
