// QCue: back-compat re-export shim. The auth wiring was split to satisfy the
// layering law: the net/config providers ([qcueConfigProvider]/[tokenStoreProvider])
// live on the `core/net` bridge, and the app-global auth state
// ([authStateProvider]/[authRepositoryProvider]/[AuthStatus]) lives in
// `core/session/auth_state`. This shim re-exports them so the auth SCREENS (same
// feature) and the router keep importing one familiar path. Settings + the
// Settings server-URL field import the core homes directly (no cross-feature hop).
export '../../core/net/api_client_provider.dart' show qcueConfigProvider, tokenStoreProvider;
export '../../core/session/auth_state.dart'
    show AuthStatus, AuthStateNotifier, authStateProvider, authRepositoryProvider;
