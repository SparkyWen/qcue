import XCTest

@testable import Runner

/// QCue S5-R3/R37 — the iOS background-flush bridge guards the schema version and
/// accepts schedule/cancel without throwing. (native-unverified-here: BGTaskScheduler
/// requires a device/simulator + the background-modes capability; not run on Linux CI.)
final class BackgroundHandlerTests: XCTestCase {

    func testUnknownSchemaVersionRejected() {
        let handler = BackgroundHandler()
        let done = expectation(description: "reject")
        let call = FlutterMethodCall(methodName: "schedulePeriodic", arguments: ["schemaVersion": 999])
        handler.testHandle(call) { result in
            XCTAssertEqual((result as? FlutterError)?.code, "versionMismatch")
            done.fulfill()
        }
        wait(for: [done], timeout: 2)
    }

    func testScheduleAndCancelSucceed() {
        let handler = BackgroundHandler()
        let scheduled = expectation(description: "schedule")
        handler.testHandle(FlutterMethodCall(
            methodName: "schedulePeriodic",
            arguments: ["schemaVersion": QcueChannels.schemaVersion, "requiresNetwork": true]
        )) { _ in scheduled.fulfill() }
        let cancelled = expectation(description: "cancel")
        handler.testHandle(FlutterMethodCall(
            methodName: "cancel",
            arguments: ["schemaVersion": QcueChannels.schemaVersion]
        )) { _ in cancelled.fulfill() }
        wait(for: [scheduled, cancelled], timeout: 2)
    }

    func testTaskIdentifierMatchesInfoPlist() {
        // Mirrors Android's FlushWorker.UNIQUE_WORK + Runner/Info.plist
        // BGTaskSchedulerPermittedIdentifiers.
        XCTAssertEqual(BackgroundHandler.taskIdentifier, "qcue.flush.periodic")
    }
}
