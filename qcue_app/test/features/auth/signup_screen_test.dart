// QCue v0.1.1 (WS-A2): the signup screen + the /signup route + the redirect.
// Pins (mirroring login_screen_test):
//   - an UNAUTHENTICATED app may navigate to /signup WITHOUT being bounced to
//     /login (both are allowed unauthenticated locations);
//   - a successful signup flips authState → lands on the app shell;
//   - a duplicate-email 400 shows the "email already registered" message;
//   - a network error shows the "can't reach the server" message;
//   - the screen renders the form fields with the design tokens.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/app.dart';
import 'package:qcue_app/core/net/qcue_config.dart';
import 'package:qcue_app/core/session/session_provider.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/auth/auth_provider.dart';
import 'package:qcue_app/features/auth/auth_repository.dart';
import 'package:qcue_app/features/auth/signup_screen.dart';

/// A scriptable [AuthRepository] test double. Records the last signup call and
/// either returns a session, throws an [AuthError], or both — without touching
/// the network. Built on a real config + in-memory store so [signedIn] wiring
/// still flows through the real providers.
class FakeAuthRepository extends AuthRepository {
  FakeAuthRepository({this.throwOnSignup})
      : super(config: QcueConfig(), tokens: InMemoryTokenStore());

  final AuthError? throwOnSignup;
  String? lastEmail;

  @override
  Future<Session> signup(String email, String password) async {
    lastEmail = email;
    final err = throwOnSignup;
    if (err != null) throw err;
    return Session(jwt: 'tok', email: email, hasKey: false);
  }
}

Widget _appWith(AuthRepository repo) => ProviderScope(
      overrides: [authRepositoryProvider.overrideWithValue(repo)],
      child: const QCueApp(),
    );

void main() {
  testWidgets('an unauthenticated app can navigate to /signup (not bounced)',
      (tester) async {
    await tester.pumpWidget(const ProviderScope(child: QCueApp()));
    await tester.pumpAndSettle();
    // Unauthed → starts on /login.
    expect(find.byKey(const ValueKey('login-screen')), findsOneWidget);

    // Tap the "create account" link → /signup, and we are NOT bounced to /login.
    await tester.tap(find.byKey(const ValueKey('login-signup-link')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('signup-screen')), findsOneWidget);
    expect(find.byKey(const ValueKey('signup-email')), findsOneWidget);
    expect(find.byKey(const ValueKey('signup-password')), findsOneWidget);
    expect(find.byKey(const ValueKey('login-screen')), findsNothing);
  });

  testWidgets('a successful signup flips authState → the app shell shows',
      (tester) async {
    final repo = FakeAuthRepository();
    await tester.pumpWidget(_appWith(repo));
    await tester.pumpAndSettle();

    // Go to /signup.
    await tester.tap(find.byKey(const ValueKey('login-signup-link')));
    await tester.pumpAndSettle();

    await tester.enterText(
        find.byKey(const ValueKey('signup-email')), 'new@b.co');
    await tester.enterText(
        find.byKey(const ValueKey('signup-password')), 'pw123456');
    await tester.enterText(
        find.byKey(const ValueKey('signup-confirm')), 'pw123456');
    await tester.tap(find.byKey(const ValueKey('signup-submit')));
    await tester.pumpAndSettle();

    expect(repo.lastEmail, 'new@b.co');
    expect(find.byKey(const ValueKey('signup-screen')), findsNothing);
    expect(find.byKey(const ValueKey('capture-field')), findsOneWidget);
  });

  testWidgets('a duplicate-email 400 shows the "already registered" message',
      (tester) async {
    final repo = FakeAuthRepository(
        throwOnSignup: AuthError.emailTaken('email already registered'));
    await tester.pumpWidget(_appWith(repo));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('login-signup-link')));
    await tester.pumpAndSettle();

    await tester.enterText(
        find.byKey(const ValueKey('signup-email')), 'taken@b.co');
    await tester.enterText(
        find.byKey(const ValueKey('signup-password')), 'pw123456');
    await tester.enterText(
        find.byKey(const ValueKey('signup-confirm')), 'pw123456');
    await tester.tap(find.byKey(const ValueKey('signup-submit')));
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('signup-error')), findsOneWidget);
    expect(
      find.textContaining('already registered', findRichText: true),
      findsOneWidget,
    );
    // Still on the signup screen (not signed in).
    expect(find.byKey(const ValueKey('signup-screen')), findsOneWidget);
  });

  testWidgets('a network error shows the unreachable-server message',
      (tester) async {
    final repo = FakeAuthRepository(throwOnSignup: AuthError.network('refused'));
    await tester.pumpWidget(_appWith(repo));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('login-signup-link')));
    await tester.pumpAndSettle();

    await tester.enterText(
        find.byKey(const ValueKey('signup-email')), 'a@b.co');
    await tester.enterText(
        find.byKey(const ValueKey('signup-password')), 'pw123456');
    await tester.enterText(
        find.byKey(const ValueKey('signup-confirm')), 'pw123456');
    await tester.tap(find.byKey(const ValueKey('signup-submit')));
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('signup-error')), findsOneWidget);
    expect(find.textContaining('reach the server'), findsOneWidget);
  });

  testWidgets('the signup screen renders the form fields', (tester) async {
    await tester.pumpWidget(ProviderScope(
      child: MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: const SignupScreen(),
      ),
    ));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('signup-submit')), findsOneWidget);
    expect(find.text('Create account'), findsWidgets);
  });
}
