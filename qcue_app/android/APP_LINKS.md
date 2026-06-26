# Android App Links runbook (security: deep-link / OAuth-redirect hardening)

**Why:** `cn.qcue.app` registers the custom scheme `qcue://` (`AndroidManifest.xml`,
`VIEW`/`BROWSABLE`, no host, no verification). That same scheme is the OAuth/OIDC redirect
(`qcue://oauth2redirect`, via `appAuthRedirectScheme=qcue` in `app/build.gradle.kts`, used by
`lib/core/auth/qcue_oidc.dart`). Any installed app can also register `qcue://`, so the OAuth
callback (and the internal widget/notif links) is **hijackable**.

PKCE already blocks *token theft* (an intercepted `code` is useless without the in-app
`code_verifier` — RFC 8252), so today's residual risk is callback-stealing / phishing UX, not
account takeover. App Links (verified `https://` links the OS binds exclusively to this signed
app) closes it for good.

> This is a **coordinated** change: app code + a server-hosted file + a Google-console redirect
> URI. Do it as one unit and verify sign-in on a device — a half-applied migration breaks Google
> sign-in. That is why it is a runbook here, not a committed code change.

---

## 1. Get the signing SHA-256 fingerprint(s)

App Links matches the **installed APK's signing cert**, so the fingerprint depends on the install
source:

- **Sideloaded GitHub-Release APK** (signed locally with the `upload` key):
  ```sh
  # run where a JDK exists (your Android build machine)
  keytool -list -v \
    -keystore qcue_app/android/upload-keystore.p12 \
    -storepass "$(grep '^storePassword=' qcue_app/android/key.properties | cut -d= -f2)" \
    -alias upload | grep 'SHA256:'
  ```
- **Google Play install** (Play re-signs with the Play App Signing key — the upload key's
  fingerprint will NOT match): Google Play Console → your app → **Setup → App integrity →
  App signing key certificate → SHA-256 certificate fingerprint**.

Include **both** fingerprints so links verify regardless of how the user installed the app.

## 2. Create / host `assetlinks.json`

Serve this (unauthenticated, `Content-Type: application/json`, HTTP 200, **no redirect**) at
exactly `https://app.qcue.cn/.well-known/assetlinks.json`. Add a route in the backend
(`qcue-rs/app-server`, alongside `/healthz` `/readyz` `/version`):

```json
[
  {
    "relation": ["delegate_permission/common.handle_all_urls"],
    "target": {
      "namespace": "android_app",
      "package_name": "cn.qcue.app",
      "sha256_cert_fingerprints": [
        "PASTE_UPLOAD_OR_LOCAL_RELEASE_KEY_SHA256_HERE",
        "PASTE_PLAY_APP_SIGNING_SHA256_HERE"
      ]
    }
  }
]
```

(While here, the iOS Universal Links equivalent is `apple-app-site-association` at the same
`/.well-known/` path — out of scope for this Android task, but the same hosting story.)

## 3. Add the verified intent-filter (manifest)

Keep the existing `qcue://` filter during the transition; **add** a path-scoped App Links filter
(scoped to `/applink` so it does NOT capture the website's normal pages):

```xml
<intent-filter android:autoVerify="true">
    <action android:name="android.intent.action.VIEW"/>
    <category android:name="android.intent.category.DEFAULT"/>
    <category android:name="android.intent.category.BROWSABLE"/>
    <data android:scheme="https" android:host="app.qcue.cn" android:pathPrefix="/applink"/>
</intent-filter>
```

Then teach `MainActivity.routeDeepLink` to accept the https host (mirror the existing `qcue://`
host/path allowlist):

```kotlin
private fun routeDeepLink(uri: Uri?) {
    if (uri == null) return
    val isAppLink = uri.scheme == "https" && uri.host == "app.qcue.cn" &&
        uri.path?.startsWith("/applink/") == true
    if (uri.scheme != "qcue" && !isAppLink) return
    val path = uri.path?.removePrefix("/applink") ?: uri.path
    when (uri.host) {
        "capture" -> if (path == "/compose") widget?.deliverTap("compose")
        "widget"  -> if (path == "/quickCapture") widget?.deliverTap("quickCapture")
    }
    if (isAppLink) when (path) {
        "/capture/compose"     -> widget?.deliverTap("compose")
        "/widget/quickCapture" -> widget?.deliverTap("quickCapture")
        "/oauth2redirect"      -> { /* handled by flutter_appauth */ }
    }
}
```

## 4. Migrate the OAuth redirect (the actual security win)

1. Choose the https redirect: `https://app.qcue.cn/applink/oauth2redirect`.
2. **Google Cloud Console → Credentials → the OAuth client** → add the new https redirect URI
   (keep `qcue://oauth2redirect` until rollout is confirmed, then remove it).
3. `lib/core/auth/qcue_oidc.dart`: change the redirect URL constant from `qcue://oauth2redirect`
   to the https URL. Update/drop the `appAuthRedirectScheme=qcue` placeholder in
   `app/build.gradle.kts` accordingly (flutter_appauth supports https App Links redirects).
4. Deploy `assetlinks.json` (step 2) **before** shipping the app change.

## 5. Verify on device

```sh
adb shell pm get-app-links cn.qcue.app          # expect: app.qcue.cn -> verified
adb shell pm verify-app-links --re-verify cn.qcue.app
# then sign in with Google end-to-end and tap a widget/notification link
```

If `assetlinks.json` is unreachable or the fingerprint is wrong, verification silently fails and
the OS falls back to the chooser/browser — so confirm step 5 before removing the `qcue://` filter.

---

## Implementation notes (as shipped — branch `security/android-app-links`)

Two clarifications/deviations from the steps above; read before deploying:

1. **MainActivity's App Links filter is path-scoped to the deep-link namespaces, NOT a blanket
   `/applink`.** It declares `pathPrefix=/applink/capture` and `pathPrefix=/applink/widget` only. The
   OAuth redirect `/applink/oauth2redirect` is deliberately left out of MainActivity and is owned
   exclusively by flutter_appauth's `net.openid.appauth.RedirectUriReceiverActivity` (declared in
   `AndroidManifest.xml` with its own verified https intent-filter, exact `path`). A blanket `/applink`
   on MainActivity would *also* match the OAuth path → two in-app activities matching one URL → an
   ambiguous in-app chooser could fire mid-sign-in. Non-overlapping paths avoid that.
2. **`appAuthRedirectScheme` stays `"qcue"`** (not dropped). flutter_appauth's bundled manifest still
   references `${appAuthRedirectScheme}` on `RedirectUriReceiverActivity`; dropping the placeholder
   fails the manifest merge. It now serves only as the legacy qcue:// AppAuth receiver during the
   transition — remove it (and the qcue:// MainActivity filter) once step 5 passes.

**Scope/risk finding:** the app's **live** Google/Apple sign-in is the NATIVE path
(`core/auth/google_native_signin.dart` / `apple_native_signin.dart` → a Google/Apple ID token →
backend `/v1/auth/social`), which uses **no redirect URI** and is unaffected by App Links. The
flutter_appauth redirect path (`core/auth/qcue_oidc.dart`, `QcueOidc`) currently has **no callers**, so
migrating its redirect to https hardens a not-yet-wired seam + the manifest-registered `qcue://` surface
and does **not** change today's live sign-in behaviour (no iOS Universal Links / AASA needed for the
native flow). The assetlinks-first ordering remains the correct discipline (and is required if
`QcueOidc` is ever wired to a redirect-based flow).

**Not verifiable on the planning host (no Android SDK):** the manifest *merge* (the additive
`RedirectUriReceiverActivity` intent-filter unioning with the plugin's) and the autoVerify result. Run a
real `flutter build apk` + device step 5 to confirm both.

### Signing fingerprint (upload key) — for `assetlinks.json`

Computed from `android/upload-keystore.p12` (the key that signs the sideloaded GitHub-Release APK):

```
SHA-256: 36:49:16:DF:C7:86:27:50:64:C8:CC:55:D2:6B:46:E9:EC:F3:52:8A:6A:C4:88:83:B7:D7:69:55:65:D4:9A:FF
```

If you also distribute via Google Play, add the **Play App Signing** key's SHA-256 (Play Console →
Setup → App integrity → App signing key certificate) as a second entry in `sha256_cert_fingerprints`.
