import Flutter
import Foundation
import LocalAuthentication
import Security

/// QCue S5-R24/R25/R26/R27 (D9) — the thin iOS secure key store bridge over
/// `qcue/secure`.
///
/// Stores the WRAPPED (AES-GCM ciphertext) BYOK blob in the Keychain as a
/// `kSecClassGenericPassword` item, protected
/// `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` (no iCloud Keychain sync — S5-R27).
/// The plaintext key NEVER crosses this channel — only the already-wrapped blob
/// (S5-R24); decryption happens in the Rust `secrets` crate. Values are NEVER
/// logged (S5-R28).
///
/// SECURITY GAP (S5-R26, tracked): the biometric gate is enforced ONLY in app code
/// (`read(requireBiometric:)` runs `LAContext.evaluatePolicy` before fetching). The
/// item itself carries NO `kSecAttrAccessControl`, so the OS does not require auth on
/// a direct `SecItemCopyMatching` — a code-path bug, or forensic/jailbreak access to
/// the keychain, can read the wrapped blob without the promised Face ID/passcode
/// prompt. NOTE: this same handler also backs the JWT session store
/// (`SecureTokenStore`, keys `qcue.session.*`), which is read with
/// `requireBiometric:false` on every launch — so an OS-level ACL must be applied
/// PER-ITEM (BYOK only), NOT blanket, or session restore would force a Face ID
/// prompt at every launch. Fix = thread a `biometric` flag through `write` and add
/// `SecAccessControlCreateWithFlags(..., .biometryCurrentSet/.userPresence, ...)`
/// as `kSecAttrAccessControl` for vault items. Requires an on-device build to verify.
///
/// NOTE (native-unverified-here): code-complete; NOT compiled/unit-run on the Linux
/// CI host (no Xcode). An XCTest file accompanies it.
final class SecureHandler {

    private let service = "app.qcue.byok"

    static func register(messenger: FlutterBinaryMessenger) -> SecureHandler {
        let handler = SecureHandler()
        let channel = FlutterMethodChannel(name: QcueChannels.secure, binaryMessenger: messenger)
        channel.setMethodCallHandler { call, result in handler.handle(call, result: result) }
        return handler
    }

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
        case "write":
            guard let key = args["key"] as? String, let value = args["value"] as? String else {
                result(typedError("osError", "missing args")); return
            }
            let requireBiometric = args["requireBiometric"] as? Bool ?? false
            write(key: key, value: value, requireBiometric: requireBiometric, result: result)
        case "read":
            guard let key = args["key"] as? String else {
                result(typedError("osError", "missing key")); return
            }
            let requireBiometric = args["requireBiometric"] as? Bool ?? false
            let reason = args["reason"] as? String ?? "Unlock your key"
            read(key: key, requireBiometric: requireBiometric, reason: reason, result: result)
        case "delete":
            guard let key = args["key"] as? String else {
                result(typedError("osError", "missing key")); return
            }
            delete(key: key)
            result(nil)
        case "containsKey":
            guard let key = args["key"] as? String else {
                result(typedError("osError", "missing key")); return
            }
            result(read(key: key) != nil)
        case "biometricAvailable":
            var error: NSError?
            let ok = LAContext().canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error)
            result(ok)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    // MARK: - Keychain

    /// Write does NOT prompt (S5-R26): the wrapped blob is cached when the vault
    /// syncs; the biometric gate is enforced on READ. This mirrors Android
    /// SecurePlugin, which writes into EncryptedSharedPreferences unconditionally and
    /// gates only the read. The item is bound `WhenUnlockedThisDeviceOnly` — never
    /// synced to iCloud Keychain, never in a device backup (S5-R27).
    ///
    /// SECURITY GAP (tracked, see type docstring): no `kSecAttrAccessControl` is set,
    /// so the biometric requirement is app-code-only, not OS-enforced. A per-item
    /// (BYOK-only) access-control ACL is the fix; it must NOT be applied to the JWT
    /// session items this same handler stores, or launch would prompt for Face ID.
    private func write(key: String, value: String, requireBiometric: Bool, result: @escaping FlutterResult) {
        delete(key: key)
        guard let data = value.data(using: .utf8) else {
            result(typedError("osError", "encoding")); return
        }
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecValueData as String: data,
        ]
        if requireBiometric {
            // SECURITY (S5-R26): bind an OS-ENFORCED access-control ACL so the wrapped
            // blob cannot be released by a bare `SecItemCopyMatching` — the OS now
            // requires a fresh user-presence auth (`.userPresence` = biometry OR the
            // device passcode, matching the read path's `.deviceOwnerAuthentication`
            // so a no-biometry/passcode-only device still works). Applied to BYOK vault
            // items ONLY; the JWT session items (requireBiometric:false) keep the plain
            // accessible attribute below so session restore stays prompt-free on launch.
            // NOTE: kSecAttrAccessControl and kSecAttrAccessible are mutually exclusive —
            // the access class is supplied as the ACL's protection parameter here.
            var aclError: Unmanaged<CFError>?
            guard let access = SecAccessControlCreateWithFlags(
                nil,
                kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                .userPresence,
                &aclError
            ) else {
                result(typedError("osError", "acl create failed")); return
            }
            query[kSecAttrAccessControl as String] = access
        } else {
            query[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        }
        let status = SecItemAdd(query as CFDictionary, nil)
        if status == errSecSuccess {
            result(nil)
        } else {
            result(typedError("osError", "keychain add \(status)"))
        }
    }

    /// Read the wrapped blob for [key]. When `requireBiometric` (S5-R26) a
    /// LocalAuthentication evaluation gates the read; a user-cancel / auth failure
    /// FAILS CLOSED (typed cancelled / permissionDenied — the blob is never returned
    /// unauthenticated). When false, the blob is returned without a prompt. Mirrors
    /// Android SecurePlugin's gate-on-read model.
    private func read(key: String, requireBiometric: Bool, reason: String, result: @escaping FlutterResult) {
        guard requireBiometric else {
            result(fetch(key: key, context: nil)); return
        }
        let context = LAContext()
        context.localizedReason = reason
        context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { [weak self] ok, error in
            DispatchQueue.main.async {
                guard let self = self else { return }
                if ok {
                    // Reuse the JUST-authenticated context so the access-control ACL on
                    // the item is satisfied without triggering a SECOND OS prompt.
                    result(self.fetch(key: key, context: context))
                    return
                }
                switch (error as? LAError)?.code {
                case .userCancel?, .appCancel?, .systemCancel?:
                    result(self.typedError("cancelled", "user cancelled"))
                default:
                    result(self.typedError("permissionDenied", "biometric failed"))
                }
            }
        }
    }

    /// Fetch the stored blob as a String, or nil if absent. With a [context] the
    /// fetch satisfies an access-control ACL using the already-authenticated
    /// LAContext (no re-prompt); without one it never surfaces the auth UI.
    private func fetch(key: String, context: LAContext?) -> String? {
        guard let data = read(key: key, context: context) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    /// Read the stored blob. Pass the authenticated [context] to satisfy an
    /// access-control ACL without a second prompt; with no context (existence
    /// check / non-biometric item) the auth UI is suppressed.
    private func read(key: String, context: LAContext? = nil) -> Data? {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        if let context = context {
            query[kSecUseAuthenticationContext as String] = context
        } else {
            // Suppress the auth UI for a mere existence check / a non-ACL item.
            query[kSecUseAuthenticationUI as String] = kSecUseAuthenticationUISkip
        }
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        // errSecInteractionNotAllowed means the item exists but is auth-gated.
        if status == errSecInteractionNotAllowed { return Data() }
        return status == errSecSuccess ? (item as? Data) : nil
    }

    private func delete(key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        SecItemDelete(query as CFDictionary)
    }

    private func typedError(_ kind: String, _ message: String) -> FlutterError {
        FlutterError(code: kind, message: message, details: ["kind": kind, "retryable": false])
    }
}
