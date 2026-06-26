import XCTest

@testable import Runner

/// QCue S5-R3/R42 — the iOS share bridge guards the schema version and drains the
/// App Group container exactly once. (native-unverified-here: requires Xcode + the
/// App Group entitlement; not run on the Linux CI host.)
final class ShareHandlerTests: XCTestCase {

    func testUnknownSchemaVersionRejected() {
        let handler = ShareHandler()
        let done = expectation(description: "reject")
        let call = FlutterMethodCall(methodName: "drainPending", arguments: ["schemaVersion": 999])
        handler.testHandle(call) { result in
            XCTAssertEqual((result as? FlutterError)?.code, "versionMismatch")
            done.fulfill()
        }
        wait(for: [done], timeout: 2)
    }

    func testDrainReturnsStagedItemsThenClears() {
        SharedContainer.stageSharedItem(["text": "hello", "sourceApp": "x"])
        let handler = ShareHandler()
        let done = expectation(description: "drain")
        let call = FlutterMethodCall(
            methodName: "drainPending",
            arguments: ["schemaVersion": QcueChannels.schemaVersion]
        )
        handler.testHandle(call) { result in
            let items = result as? [StagedItem]
            XCTAssertEqual(items?.count, 1)
            XCTAssertEqual(items?.first?["text"] as? String, "hello")
            done.fulfill()
        }
        wait(for: [done], timeout: 2)
        // a second drain is empty (drained exactly once)
        XCTAssertTrue(SharedContainer.drainSharedItems().isEmpty)
    }
}
