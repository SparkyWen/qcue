import XCTest

@testable import Runner

/// QCue S5-R3/R46 — the iOS widget bridge guards the schema version and persists
/// only the non-sensitive count to the App Group container. (native-unverified-here.)
final class WidgetHandlerTests: XCTestCase {

    func testUnknownSchemaVersionRejected() {
        let handler = WidgetHandler()
        let done = expectation(description: "reject")
        let call = FlutterMethodCall(methodName: "setCount", arguments: ["schemaVersion": 999])
        handler.testHandle(call) { result in
            XCTAssertEqual((result as? FlutterError)?.code, "versionMismatch")
            done.fulfill()
        }
        wait(for: [done], timeout: 2)
    }

    func testSetCountPersistsOnlyTheCount() {
        let handler = WidgetHandler()
        let done = expectation(description: "set")
        let call = FlutterMethodCall(methodName: "setCount", arguments: [
            "schemaVersion": QcueChannels.schemaVersion,
            "count": 7,
        ])
        handler.testHandle(call) { _ in done.fulfill() }
        wait(for: [done], timeout: 2)
        XCTAssertEqual(SharedContainer.widgetCount(), 7)
    }
}
