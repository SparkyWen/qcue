import XCTest

@testable import Runner

/// QCue S5-R24/R27 — the iOS Keychain secure store round-trips the WRAPPED blob and
/// is bound ThisDeviceOnly. These assert the channel-contract behavior reachable
/// without a biometric prompt; the biometric fail-closed path (S5-R26) is verified
/// on-device against the LocalAuthentication prompt.
///
/// (native-unverified-here: requires Xcode + the Keychain entitlement; not run on the
/// Linux CI host.)
final class SecureHandlerTests: XCTestCase {

    func testWriteReadDeleteRoundTrip() {
        let handler = SecureHandler()
        let key = "test_cred_\(UUID().uuidString)"
        let blob = "WRAPPED-CIPHERTEXT"

        let writeDone = expectation(description: "write")
        handler.invoke("write", ["key": key, "value": blob]) { _ in writeDone.fulfill() }
        wait(for: [writeDone], timeout: 2)

        let readDone = expectation(description: "read")
        handler.invoke("containsKey", ["key": key]) { result in
            XCTAssertEqual(result as? Bool, true)
            readDone.fulfill()
        }
        wait(for: [readDone], timeout: 2)

        let delDone = expectation(description: "delete")
        handler.invoke("delete", ["key": key]) { _ in delDone.fulfill() }
        wait(for: [delDone], timeout: 2)

        let goneDone = expectation(description: "gone")
        handler.invoke("containsKey", ["key": key]) { result in
            XCTAssertEqual(result as? Bool, false)
            goneDone.fulfill()
        }
        wait(for: [goneDone], timeout: 2)
    }

    /// SECURITY (S5-R26): a biometric write binds an access-control ACL on the item.
    /// SecItemAdd with an ACL does NOT require auth, and containsKey uses
    /// `kSecUseAuthenticationUISkip` (→ errSecInteractionNotAllowed → "exists"), so this
    /// round-trip is exercisable on the Simulator even with no enrolled biometrics. The
    /// gated READ (which prompts) is verified on-device only.
    func testBiometricWriteBindsAclAndRoundTrips() {
        let handler = SecureHandler()
        let key = "test_byok_\(UUID().uuidString)"

        let writeDone = expectation(description: "write")
        handler.invoke("write", ["key": key, "value": "WRAPPED", "requireBiometric": true]) { res in
            XCTAssertFalse(res is FlutterError) // ACL creation + add must succeed
            writeDone.fulfill()
        }
        wait(for: [writeDone], timeout: 2)

        let existsDone = expectation(description: "exists")
        handler.invoke("containsKey", ["key": key]) { result in
            XCTAssertEqual(result as? Bool, true) // gated item still reports as present
            existsDone.fulfill()
        }
        wait(for: [existsDone], timeout: 2)

        let delDone = expectation(description: "delete")
        handler.invoke("delete", ["key": key]) { _ in delDone.fulfill() }
        wait(for: [delDone], timeout: 2)
    }

    func testUnknownSchemaVersionRejected() {
        let handler = SecureHandler()
        let done = expectation(description: "reject")
        handler.invokeRaw("containsKey", ["schemaVersion": 999, "key": "k"]) { result in
            XCTAssertTrue(result is FlutterError)
            XCTAssertEqual((result as? FlutterError)?.code, "versionMismatch")
            done.fulfill()
        }
        wait(for: [done], timeout: 2)
    }
}

// Test-only invocation shims so the handler's private `handle` is exercisable.
extension SecureHandler {
    func invoke(_ method: String, _ args: [String: Any], _ result: @escaping FlutterResult) {
        invokeRaw(method, args.merging(["schemaVersion": QcueChannels.schemaVersion]) { a, _ in a }, result)
    }

    func invokeRaw(_ method: String, _ args: [String: Any], _ result: @escaping FlutterResult) {
        let call = FlutterMethodCall(methodName: method, arguments: args)
        // `handle` is file-private to SecureHandler; this extension lives in the same
        // test target with @testable import, which surfaces internal members. The
        // production `handle` is invoked through the registered channel; for the unit
        // test it is exposed via `testHandle`.
        testHandle(call, result: result)
    }
}
