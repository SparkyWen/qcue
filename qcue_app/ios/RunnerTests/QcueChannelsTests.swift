import XCTest

@testable import Runner

/// QCue S5-R3 — the iOS channel-version guard + name contract.
/// (native-unverified-here: requires Xcode; not run on the Linux CI host.)
final class QcueChannelsTests: XCTestCase {

    func testSchemaVersionMatchesContract() {
        XCTAssertEqual(QcueChannels.schemaVersion, 1)
        XCTAssertEqual(QcueChannels.stt, "qcue/stt")
        XCTAssertEqual(QcueChannels.sttEvents, "qcue/stt/events")
        XCTAssertEqual(QcueChannels.secure, "qcue/secure")
    }

    func testVersionOkRejectsUnknownMajor() {
        XCTAssertTrue(QcueChannels.versionOk(["schemaVersion": 1]))
        XCTAssertFalse(QcueChannels.versionOk(["schemaVersion": 999]))
        XCTAssertFalse(QcueChannels.versionOk(["schemaVersion": "1"]))
        XCTAssertFalse(QcueChannels.versionOk(nil))
        XCTAssertFalse(QcueChannels.versionOk([:]))
    }
}
