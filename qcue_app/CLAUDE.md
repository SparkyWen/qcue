# QCue App — Flutter Architecture Guide

**Status**: Foundation Milestone (S4) — production-ready capture/wiki/recall/activity/settings UI; real network client + offline-first local cache; 3 themes; native S5 capabilities staging.

**Model**: Riverpod 2.x state management; go_router 14.x navigation; semantic design tokens (Clean Light / Anthropic Warm / Night); platform-channel method/event channels to Kotlin/Swift natives.

---

## Core Architecture

### 1. **Network & API Client Abstraction**

**Files**: `core/net/` (seams + implementations)

The app speaks ONE data interface: `QcueApiClient` (sealed interface in `qcue_api_client.dart`). Two implementations exist:

| Implementation | Use | Transport |
|---|---|---|
| **StubApiClient** | Demo/fixtures (no backend needed) | In-memory seeded data |
| **HttpApiClient** | Production | REST/RPC over `http` + SSE (EventSource w/ `?token=` auth) |

**Deployment modes** (build-time):
- `flutter run --dart-define=QCUE_STUB=true` → demo mode (keyless, inert, seeded fixtures)
- `flutter run --dart-define=QCUE_BASE_URL=https://api.qcue.app` → live backend (default: local 127.0.0.1:3000)

**Key surface methods**:
- `capture()` — persist a new idea (pending state; idempotency-keyed)
- `captures()` — reverse-chronological feed
- `wikiIndex()` / `wikiPage(slug)` — wiki browse
- `recallStream(question)` — SSE-streamed answer (sessionStarted/messageDelta/citation/reasoning/done)
- `approvals()` / `respondApproval()` — ingest review gate (D13)
- `jobs()` / `dreamEvents()` / `cancelJob()` — activity tracking
- `credentials()` / `putKey()` / `deleteKey()` / `setActiveModel()` — BYOK vault (D9)
- `costLedger()` — per-day spend in micros

**Wire contract**:
- JWT bearer (`Authorization: Bearer <access>`) on every REST/RPC call
- JSON-RPC-lite: `{error:{code,message}}` on non-2xx; `-32001` is retried with jitter
- `Idempotency-Key` header on capture POSTs (deduped server-side)
- SSE auth via `?token=` query param (not Authorization header — pitfall #15)

---

### 2. **Local-First Offline-Capable Architecture (D5/D6)**

**Files**: `core/offline/` (cache/queue/sync-status)

**Design Pattern**: Decorator over HttpApiClient (`OfflineAwareApiClient`) — UNCHANGED `QcueApiClient` seam.

**Canonical Guarantees**:
1. **Durability**: A capture is persisted to SQLITE3 (feed + outbound queue) BEFORE any network attempt — an idea is never lost, even fully offline.
2. **Idempotency**: Each queued capture carries a stable uuidv7-shaped client ID; the idempotency key guards against double-insertion on retry.
3. **Graceful Reads**: `captures()` / `wikiPage()` try the network; on failure serve from the cache.
4. **LRU Eviction**: Read-cache trims old rows under a cap (`feedCap=200`, `wikiCap=48`), but NEVER evicts unflushed queued captures.

**IdeaCache API**:
- `enqueueCapture()` — write locally, return a provisional `Idea` with `queued=true`
- `putFeed()` — refresh cache with server data, preserving unflushed rows
- `feed()` — return merged [server data + queued rows]
- `flushOutbox()` — POST unflushed captures in order, idempotent, reconcile
- `reconcileQueued()` — flip a queued row to the server's authoritative idea (real ID; ingestion state)

**Backing store seam** (`CacheStore` interface):
- Production: **SqliteCacheStore** (FFI bindings; Android loads the `sqlite3_flutter_libs` bundled lib, iOS/macOS open the OS system `libsqlite3.dylib` — the bundled lib segfaults under the iOS SPM build; see `sqlite_cache_store.dart`. System SQLite has no FTS5/rtree.)
- Tests: **InMemoryCacheStore** (no disk, fast)

**Sync status tracking** (`syncStatusProvider`):
- Captures upload failures classified as: `unauthorized` (401), `network` (unreachable), `other` (server error)
- UI reads this to show the offline banner with actionable messages (e.g., "Not signed in — sign in to upload")

**Connectivity probe** (`connectivityProvider`):
- A lightweight `/readyz` HTTP GET (not `connectivity_plus` plugin — headless `flutter test` incompatible)
- Runs on bootstrap and every 10 seconds; on reconnect + app resume, the outbound queue is flushed
- Seam: `ConnectivitySource` (prod: `PingConnectivitySource`, tests: `FakeConnectivitySource`)

---

### 3. **State Management: Riverpod 2.x**

**Files**: `lib/features/*/` (feature providers); `core/net/api_client_provider.dart` (data seam)

**Pattern**:
- **Data seam**: `apiClientProvider` (bound at bootstrap; single point of DI)
- **Feature providers**: `AsyncNotifier` (fetch), `Notifier` (mutation), `FutureProvider`/`Provider` (computed)
- **Screen state**: `ScreenState<T>` sealed class (Loading/Empty/Error/Data); renders one state at a time

**Key providers**:
- `apiClientProvider` — the single `QcueApiClient` (Stub or Http or Offline-decorated)
- `captureFeedProvider` — `AsyncNotifier<ScreenState<List<Idea>>>`, optimistic refresh on commit
- `wikiIndexProvider` — `FutureProvider<ScreenState<List<WikiPage>>>`
- `wikiPageProvider.family` — `FutureProvider<ScreenState<WikiPage>, slug>`
- `connectivityProvider` — `NotifierProvider<ConnectivityNotifier, Connectivity>`
- `syncStatusProvider` — last upload outcome (error reason + timestamp)
- `authStateProvider` — `NotifierProvider<AuthStateNotifier, AuthStatus>` (authed/authing/unauthed)
- `themeProvider` — `NotifierProvider<QThemeId>` (Clean Light / Anthropic Warm / Night)

**Conventions**:
- Screens read providers via `ref.watch()` (reactive); mutations use `ref.read(provider.notifier)`
- AsyncValue.guard() wraps safe error handling; explicit error classification (not silent catch)
- No setState — Riverpod owns the state graph

---

### 4. **Theming: 3 Shipped Themes + Design Tokens**

**Files**: `core/theme/` (qcue_tokens.dart = colors; qcue_text.dart = typography; qcue_theme.dart = builder)

**Pattern**: Semantic design tokens (QToken enum) + Material 3 ThemeExtension.

**Themes**:
1. **Clean Light** (default) — white bg, slate text, bright blue accent
2. **Anthropic Warm** — cream bg, warm terracotta accent, clay link text
3. **Night** — dark gray bg, light text, muted blue

**Key invariants**:
- Link text must clear 4.5:1 AA WCAG contrast (not just underline; see `linkText` token)
- Hex literals ONLY in `qcue_tokens.dart`; everywhere else uses `context.q.color(QToken.x)`
- `@immutable` all ThemeExtension classes; const constructors enforced via linting

**Accessibility**:
- `core/contrast/wcag.dart` — contrast ratio calculator (used in design-token audit)
- All states (pending/success/danger) meet 3:1 minimum; readability states clear 4.5:1

---

## Native Integration (S5)

### 5. **Platform Channels: Dart ↔ Kotlin/Swift**

**Files**: `core/native/channels.dart` (Dart contract); `android/app/src/main/kotlin/cn/qcue/app/` (Kotlin); `ios/Runner/` (Swift)

**Contract**:
- **MethodChannel** (request-response): `qcue/stt`, `qcue/secure`, `qcue/share`, `qcue/widget`, `qcue/notif`, `qcue/background`
- **EventChannel** (streaming): `qcue/stt/events`, `qcue/share/events`, `qcue/widget/events`, `qcue/notif/events`
- Every payload carries `schemaVersion: 1` (native rejects unknown major with `versionMismatch`)
- Typed error set: `kind ∈ {permissionDenied, unavailable, cancelled, osError, versionMismatch, rateLimited}`
- Raw OS exceptions NEVER leak; `nativeErrorFrom()` wraps to closed `NativeError` set

**Android Native Structure**:
```
android/app/src/main/kotlin/cn/qcue/app/
├── MainActivity.kt          ← registers all plugins on configureFlutterEngine
├── SttPlugin.kt             ← Android SpeechRecognizer + partials/finals
├── SecurePlugin.kt          ← EncryptedSharedPreferences / Keystore biometric read
├── SharePlugin.kt           ← ACTION_SEND intent + App Group storage
├── WidgetPlugin.kt          ← AppWidget broadcast receiver + quick-capture
├── NotifPlugin.kt           ← NotificationManager + tap routing
├── BackgroundPlugin.kt      ← WorkManager periodic flush schedule
├── QcueChannels.kt          ← channel names + error mapping
└── [test/kotlin/...]        ← unit tests
```

**iOS Native Structure**:
```
ios/Runner/
├── AppDelegate.swift        ← registers handlers on didInitializeImplicitFlutterEngine
├── SceneDelegate.swift      ← routes qcue:// deep links + Share Extension drain
├── Stt/SttHandler.swift     ← SFSpeechRecognizer + event marshaling
├── Secure/SecureHandler.swift ← Keychain + biometric LocalAuthentication
├── Share/ShareHandler.swift ← Share Extension + App Group UbiquitousKeyValueStore
├── Widget/WidgetHandler.swift ← WidgetKit AppIntentExecutor
├── Notif/NotifHandler.swift ← UNUserNotificationCenter + tap routing
├── Background/BackgroundHandler.swift ← BGTaskScheduler task registration
├── Native/QcueChannels.swift ← channel names + error mapping
├── ShareExtension/ShareViewController.swift ← Share Extension UI
└── QcueWidget/QcueWidget.swift ← WidgetKit static + dynamic config
```

---

### 6. **Speech-to-Text (S5-R18/R19/R21)**

**Dart Facade**: `core/native/stt/native_stt.dart`

**Flow**:
1. **Permission**: `requestPermission()` → grants mic + speech (Android: runtime; iOS: Info.plist)
2. **Availability**: `isAvailable(locale?)` → checks OS recognizer + supported locale
3. **Capture**: `start(captureId, locale)` → native records + streams partials (display-only) + final
4. **Fallback**: On-device unavailable → cloud transcribe via `api.transcribe(audio, locale)`

**Event Taxonomy** (via `SttEvent` sealed class):
- `SttPartial(captureId, text)` — live display (NOT stored)
- `SttFinal(result)` — canonical transcript (stored in body)
- `SttError(captureId, kind, message)` — typed error (unavailable/unsupportedLocale/network/permission/etc)
- `SttAvail(onDeviceAvailable, supportedLocales)` — capability update

---

### 7. **Secure Storage (S5-R46 / D9 Vault)**

**Dart Facade**: `core/secure/secure_storage.dart`

**Principle**: Plaintext keys NEVER held in Dart; only opaque handles + masked hints (`sk-…AB12`).

**Android**: EncryptedSharedPreferences (biometric gating via `LocalAuthentication`)
**iOS**: Keychain (`kSecAttrAccessibleWhenUnlockedThisDeviceOnly`, per S5-R25/R27; biometric gate applied on read when `requireBiometric`)

**Binding**:
```dart
secureStorageProvider.overrideWithValue(
  NativeSecureStorage(requireBiometric: true),  // BYOK vault
)
```

---

### 8. **Share Sheet (S5-R2, S5-R42)**

**Dart Facade**: `core/native/share/share_channel.dart`

**Android**: Intent.ACTION_SEND (text + URL) → dequeued in MainActivity
**iOS**: Share Extension (UNNotificationRequest + App Group storage) → AppDelegate drains on resume

**Binding**: Every native capture enqueues via the SAME idempotent `api.capture(body, origin)`, so all paths (manual text, voice, share, widget, notification tap) are unified offline-safe.

---

### 9. **Home-Screen Widgets (S5-R34)**

**Android**: AppWidget (QuickCaptureWidget) + deep-link intent
**iOS**: WidgetKit (QcueWidget.swift) + AppIntent executor

**Capture**: Tap → deep-link `qcue://capture/compose` → MainActivity/SceneDelegate routes to Dart GoRouter → Capture screen shows

**Count Refresh**: After first feed load, refresh widget with today's idea count (best-effort, ~20s latency)

---

### 10. **Notifications (S5-R45)**

**Android**: NotificationManager + NotificationCompat (large icon, actions)
**iOS**: UNUserNotificationCenter (foreground + background)

**Routing**: Notification tap → deep-link (kind=ingest/dream/alert) → routes via GoRouter

---

### 11. **Background Flush Scheduler (S5-R37)**

**Android**: WorkManager (periodic 15m — the WorkManager minimum; respects Doze/battery-saver)
**iOS**: BGTaskScheduler (identifier: `qcue.flush.periodic` — mirrors Android `FlushWorker.UNIQUE_WORK`; ~15m nominal)

**Job**: POST all unflushed captures in the outbound queue, idempotent by client ID

---

## The Five Screens

**Architecture**: Each feature is a `<Feature>Screen` (ConsumerWidget) + `<Feature>Provider` + optional `<Feature>Repository` (API calls).

### Capture (`features/capture/`)
- **State**: `captureFeedProvider` (AsyncNotifier)
- **Flow**: Reverse-chronological ideas (newest first); pull-to-refresh; empty state ("Capture your first idea")
- **Input**: Always-ready text field (multiline) + mic button (voice capture controller)
- **Commit**: `ref.read(captureFeedProvider.notifier).commit(body, origin)` → optimistic feed refresh + haptic
- **Offline**: Queued captures show distinct dot; auto-flush on reconnect

### Wiki (`features/wiki/`)
- **State**: `wikiIndexProvider` (index) + `wikiPageProvider.family` (detail by slug)
- **Flow**: Grouped index (by WikiPageType) → tap page → markdown body + backlinks
- **Search**: Slug-based (no full-text search in S4; future D8)
- **Links**: [[wikilink]] inline links render via `WikiLink` + `WikiLinkText` widgets

### Recall (`features/recall/`)
- **State**: `recallStreamProvider` (AsyncNotifierProvider that streams SSE)
- **Flow**: Text input (question) → SSE stream (sessionStarted/messageDelta/citation/reasoning/done) → streamed response
- **Citations**: Inline [citation_chip] that links to source (line range)
- **Reasoning**: Collapsible reasoning disclosure (gated behind reasoning tokens)

### Activity (`features/activity/`)
- **State**: `activityProvider` (approvals/jobs/cost) + `dreamProvider` (streaming dream progress)
- **Approvals**: Destructive edits (wiki merge/delete) awaiting confirm/reject
- **Jobs**: Ingest/lint/transcribe/dream with progress + error states
- **Cost Ledger**: Per-day spend (token kinds + cost in micros)
- **Dream Detail**: Real-time progress (Orient→Gather→Consolidate→Prune) with touched-page list

### Settings (`features/settings/`)
- **State**: `settingsProvider` (BYOK credentials/models/theme/server-url)
- **Sections**:
  - Theme picker (Clean Light / Anthropic Warm / Night)
  - Server URL override (Settings > Settings; read via `serverUrlStore`)
  - BYOK vault (add/delete/masked hint display; no plaintext ever shown)
  - Model picker per provider (fetch_models / activeModel / setActiveModel)
  - Cost summary (today's spend)
  - Server-readable toggle (D9 privacy)

---

## Navigation

**Router**: `core/router/qcue_router.dart` (go_router 14.x)

**Structure**:
- 5 StatefulShellRoutes (tab-preserving): `/capture`, `/wiki`, `/recall`, `/activity`, `/settings`
- Auth gate: unauthenticated redirects to `/login` (unless at `/login` or `/signup`)
- Deep-links: `qcue://capture/compose` (widget) routed through GoRouter
- Not found: Typed `NotFoundScreen`

**Root routes**:
```
/login → LoginScreen
/signup → SignupScreen
/capture → CaptureScreen (+ subroutes)
/wiki → WikiScreen
  ├ /:slug → WikiPageScreen
/recall → RecallScreen
/activity → ActivityScreen
  ├ /dream/:jobId → DreamDetailScreen
/settings → SettingsScreen
```

**Auth flow**:
1. Unauthenticated → `/login`
2. Sign in → `authStateProvider.notifier.markAuthed()` → router redirects to `/capture`
3. Sign out → `authStateProvider.notifier.markUnauthed()` → router redirects to `/login`

---

## Testing

### Test Commands

```bash
# Run all tests
flutter test

# Run a single test file
flutter test test/widgets/status_dot_test.dart

# Run with coverage
flutter test --coverage

# Code analysis
flutter analyze

# Format (and check via pre-commit)
dart format lib test
```

### Test Fixtures & Fakes

- **`StubApiClient.seeded()`** — demo mode (5 ideas, 6 wiki pages, approvals, jobs, cost ledger)
- **`InMemoryCacheStore`** — fast offline cache for unit tests
- **`FakeConnectivitySource`** — deterministic reachability (no real socket)
- **`InMemoryTokenStore`** — auth token storage for integration tests

### Architecture Tests

- **`test/architecture/layering_test.dart`** — enforces no cycles, features don't cross-import
- **`test/architecture/no_raw_hex_test.dart`** — grep fails if any hex outside `qcue_tokens.dart`
- **`test/app_shell_smoke_test.dart`** — 5-tab IA, navigation, live theme switch

---

## Conventions & Gotchas

### Code Style
- **Linting**: `prefer_const_constructors`, `avoid_print`, `require_trailing_commas`
- **Comments**: Tie to master plan tasks (`S4-R30`, `D5`, `Task 7`)
- **Immutability**: `@immutable` on data models; `const` constructors

### Key Patterns

| Pattern | Example | Why |
|---|---|---|
| **Seams (interfaces)** | `QcueApiClient` / `CacheStore` / `SttFacade` | Test-mockable; plug Stub/Http/Offline |
| **Decorator** | `OfflineAwareApiClient` wraps `HttpApiClient` | Local-first without re-exporting API surface |
| **Design tokens** | `context.q.color(QToken.accent)` | Single source of truth; live theme switch |
| **Sealed states** | `ScreenState<T> { Loading/Empty/Error/Data }` | Exhaustive match in UI; no null data |
| **Provider + .family** | `wikiPageProvider.family<String>` | Parameterized caching (slug → WikiPage) |
| **AsyncNotifier** | `CaptureFeedNotifier extends AsyncNotifier<ScreenState<...>>` | Async state + mutations in one notifier |

### Non-Obvious Behaviors

1. **Idempotency Keys**: Every capture enqueued locally gets a uuidv7-shaped key. On immediate POST (online) and later flush (offline), the SAME key is sent, so the server dedups a retry.

2. **Optimistic Updates**: Capture feed commits refresh the feed optimistically BEFORE the server acks (display-first UX), but the queued row is already in the cache with `queued=true` dot.

3. **SSE Replay**: On WSS reconnect, the last event sequence number is sent; the server replays missed events. Forward-compat: unknown event types are silently skipped.

4. **LRU is Selective**: Read-cache LRU eviction NEVER touches unflushed queued rows — a lost idea is the worst failure mode.

5. **Platform Channel Versioning**: Every method call payload carries `schemaVersion: 1`. Native rejects unknown major versions rather than mis-parsing; a version bump is breaking.

6. **Android Emulator Host**: When testing on Android emulator, use `10.0.2.2` to reach the host (not `127.0.0.1`). Controlled via `QCUE_BASE_URL` build-time override.

7. **SharedPreferences for Theme**: Theme choice (QThemeId) is persisted via SharedPreferences (not Keychain/Keystore — doesn't need biometrics). Read at bootstrap before runApp.

8. **No tenant_id on Wire**: The client NEVER sends `tenant_id`; the server's RLS (Row-Level Security) owns isolation (JWT claims contain it).

---

## Build & Deployment

### Android
```bash
# Build APK (debug)
flutter build apk --debug

# Run on emulator (with server URL override)
flutter run --dart-define=QCUE_BASE_URL=http://10.0.2.2:3000
```

**Release builds — use the deploy script, not a raw `flutter build`.** `pwsh
scripts/deploy-android.ps1` (Windows) builds BOTH channels: split-per-ABI APKs
(arm64-v8a + armeabi-v7a — the x86_64 emulator slice is dropped) for GitHub Releases,
and a `.aab` for Google Play. Never ship a plain `flutter build apk --release`: it
emits ONE universal APK that packs all three ABIs (~63MB; a phone downloads ~40MB it
never runs). Split-per-ABI brings the arm64 download to ~20MB — matching the iOS build.

Release **signing** reads `android/key.properties` (gitignored; see
`android/key.properties.example` for the one-time `keytool` setup) and falls back to
the debug key when absent — but Google Play **rejects** a debug-signed AAB, so the
script warns when the fallback is in effect.

### iOS
```bash
# Build for device (release)
flutter build ios --release

# Build for simulator
flutter build ios --simulator

# Archive + notarize
open ios/Runner.xcworkspace
# → Product > Archive (Xcode)
```

### Versioning
- Semantic versioning in `pubspec.yaml` (e.g., `0.1.2+3`, build-number for iOS)
- Pinned exact versions on key deps (`riverpod`, `go_router`, `sqlite3`)

---

## Rapid Onboarding Checklist

- [ ] Read `pubspec.yaml` (dependencies + versioning strategy)
- [ ] Understand the **data seam** (`apiClientProvider`) — all screens read via it
- [ ] Trace one capture from field → commit → feed
- [ ] Understand **OfflineAwareApiClient** decorator (local-first, LRU eviction, idempotent flush)
- [ ] Read a feature provider (e.g., `captureFeedProvider`) — AsyncNotifier pattern
- [ ] Check the **theme system**: token enum → ThemeExtension → `context.q.color(QToken.x)`
- [ ] Understand **native channels**: method/event channel contracts + typed errors
- [ ] Run `flutter test` → watch the smoke test + architecture tests pass
- [ ] Run the app in stub mode (`QCUE_STUB=true`) to see seeded data

---

## Future Milestones

**S5**: Real native capability plugins (STT/Keychain/Widget/Share/Notifications/Background flush). iOS is wired + verified on the simulator (Share/Widget extension targets, App Group, 17/17 XCTests, app boots); Android remains code-complete/unverified-here. Device/TestFlight needs the App IDs + App Group registered in the Apple portal.

**D4**: Cloud STT fallback (transcribe endpoint + escalation to on-device on error).

**D8**: Full-text wiki search (grep-style retrieval, not embeddings).

**D9**: BYOK vault (per-provider API key storage, biometric gating, cost tracking per provider).

**D13**: Ingest review gate (destructive wiki edits awaiting human approval).

---

## Key File Reference

| Path | Purpose |
|---|---|
| `lib/main.dart` | Bootstrap (providers, token store, offline decorator binding, native channels startup) |
| `lib/app.dart` | Root widget (theme + router setup) |
| `core/net/qcue_api_client.dart` | API seam interface + StubApiClient |
| `core/net/http_api_client.dart` | Real network client (REST/RPC + SSE) |
| `core/offline/offline_api_client.dart` | Local-first decorator (cache + queue) |
| `core/offline/idea_cache.dart` | Read-cache + outbound queue logic |
| `core/theme/qcue_tokens.dart` | Design-system token map (3 themes) |
| `core/models/protocol_models.dart` | Dart wire models (Idea/WikiPage/Job/Approval/etc) |
| `core/router/qcue_router.dart` | go_router config (5 tabs, auth gate, deep-links) |
| `features/{capture,wiki,recall,activity,settings}/` | Feature screens + providers |
| `android/app/src/main/kotlin/cn/qcue/app/MainActivity.kt` | Android plugin registration |
| `ios/Runner/AppDelegate.swift` | iOS handler registration |
