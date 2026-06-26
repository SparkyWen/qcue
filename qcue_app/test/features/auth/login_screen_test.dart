// QCue cloud-sync fix (Task 5): the router redirect + login screen. Pins:
//   - an UNAUTHENTICATED app redirects to /login (no token → the gate);
//   - signing in (flipping authStateProvider) lands on the app shell;
//   - the login screen renders the email/password form with the design tokens.
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:qcue_app/app.dart';
import 'package:qcue_app/core/net/qcue_config.dart';
import 'package:qcue_app/core/session/session_provider.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/auth/auth_provider.dart';
import 'package:qcue_app/features/auth/auth_repository.dart';
import 'package:qcue_app/features/auth/login_screen.dart';

import '../../fakes/fake_google_signin.dart';

/// A [MockClient] that answers `POST /v1/auth/social` with a token pair (the widget-test binding
/// blocks real sockets, so the HTTP exchange is mocked rather than served).
http.Client _socialClient() => MockClient((req) async => http.Response(
      jsonEncode({'access_jwt': 'g', 'refresh_jwt': 'r', 'expires_at': '2026-06-14T10:30:00Z'}),
      200,
      headers: {'content-type': 'application/json'},
    ));

/// A real [AuthRepository] wired to a [FakeGoogleSignIn] so a Google tap resolves deterministically:
/// a non-null [idToken] exchanges at `/v1/auth/social` via [httpClient]; a null one models a cancel.
AuthRepository _googleRepo({String? idToken, http.Client? httpClient}) => AuthRepository(
      config: QcueConfig(baseUrl: 'https://app.qcue.cn'),
      tokens: InMemoryTokenStore(),
      httpClient: httpClient,
      google: FakeGoogleSignIn(idToken: idToken),
    );

void main() {
  testWidgets('an unauthenticated app redirects to /login', (tester) async {
    await tester.pumpWidget(const ProviderScope(child: QCueApp()));
    await tester.pumpAndSettle();
    // No token → the router redirect carries us to the login screen.
    expect(find.byKey(const ValueKey('login-screen')), findsOneWidget);
    expect(find.byKey(const ValueKey('login-email')), findsOneWidget);
    expect(find.byKey(const ValueKey('login-password')), findsOneWidget);
    expect(find.byKey(const ValueKey('capture-field')), findsNothing);
  });

  testWidgets('a signed-in token bypasses /login → the app shell shows',
      (tester) async {
    await tester.pumpWidget(ProviderScope(
      overrides: [
        tokenStoreProvider
            .overrideWithValue(InMemoryTokenStore(access: 'tok')),
      ],
      child: const QCueApp(),
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('login-screen')), findsNothing);
    expect(find.byKey(const ValueKey('capture-field')), findsOneWidget);
  });

  testWidgets('flipping authState to signedIn navigates off /login',
      (tester) async {
    final container = ProviderContainer();
    addTearDown(container.dispose);
    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: const QCueApp(),
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('login-screen')), findsOneWidget);

    // Simulate a successful login.
    container.read(authStateProvider.notifier).signedIn(
        const Session(jwt: 'tok', email: 'a@b.co', hasKey: false));
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('login-screen')), findsNothing);
    expect(find.byKey(const ValueKey('capture-field')), findsOneWidget);
  });

  testWidgets('the login screen renders the form fields', (tester) async {
    await tester.pumpWidget(ProviderScope(
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const LoginScreen(),
      ),
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('login-submit')), findsOneWidget);
    expect(find.text('Sign in'), findsWidgets);
  });

  testWidgets('the login screen offers a Sign in with Google button',
      (tester) async {
    await tester.pumpWidget(ProviderScope(
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const LoginScreen(),
      ),
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('login-google')), findsOneWidget);
  });

  testWidgets('tapping Sign in with Google signs in and leaves /login',
      (tester) async {
    await tester.pumpWidget(ProviderScope(
      overrides: [
        authRepositoryProvider.overrideWithValue(
          _googleRepo(idToken: 'goog-id', httpClient: _socialClient()),
        ),
      ],
      child: const QCueApp(),
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('login-screen')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('login-google')));
    await tester.pumpAndSettle();

    // The shared signed-in path (authStateProvider.signedIn) carried us into the app.
    expect(find.byKey(const ValueKey('login-screen')), findsNothing);
    expect(find.byKey(const ValueKey('capture-field')), findsOneWidget);
  });

  testWidgets('a cancelled Google sign-in shows an error and stays on /login',
      (tester) async {
    await tester.pumpWidget(ProviderScope(
      overrides: [
        // idToken null = user cancel → loginWithGoogle returns null before any HTTP.
        authRepositoryProvider.overrideWithValue(_googleRepo(idToken: null)),
      ],
      child: const QCueApp(),
    ));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey('login-google')));
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('login-screen')), findsOneWidget);
    expect(find.byKey(const ValueKey('login-error')), findsOneWidget);
  });
}
