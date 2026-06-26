// QCue cloud-sync fix (Task 5): the email/password sign-in screen. Minimal,
// content-first, using the QCue design tokens (context.q / QCueText / QSpace).
// On submit it calls [AuthRepository.login]; on success the session JWT is
// persisted (durable token store) and the router redirects to the intended
// route. Errors are precise: a 401 reads "wrong email or password"; a network
// failure reads "can't reach the server — check the Server URL in Settings".
import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import 'auth_provider.dart';
import 'auth_repository.dart';

class LoginScreen extends ConsumerStatefulWidget {
  const LoginScreen({super.key});

  @override
  ConsumerState<LoginScreen> createState() => _LoginScreenState();
}

class _LoginScreenState extends ConsumerState<LoginScreen> {
  final _email = TextEditingController();
  final _password = TextEditingController();
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _email.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_busy) return;
    final email = _email.text.trim();
    final password = _password.text;
    if (email.isEmpty || password.isEmpty) {
      setState(() => _error = 'Enter your email and password.');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final session =
          await ref.read(authRepositoryProvider).login(email, password);
      if (!mounted) return;
      // Flip auth state → the router redirect carries us to the intended route.
      ref.read(authStateProvider.notifier).signedIn(session);
    } on AuthError catch (e) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _error = switch (e.kind) {
          AuthErrorKind.invalidCredentials => 'Wrong email or password.',
          // login never returns emailTaken; map it to the same generic message.
          AuthErrorKind.emailTaken => 'Wrong email or password.',
          AuthErrorKind.network =>
            "Can't reach the server — check the Server URL in Settings.",
          AuthErrorKind.server => 'The server had a problem. Try again.',
        };
      });
    } catch (_) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _error = 'Something went wrong. Try again.';
      });
    }
  }

  /// "Sign in with Google" via the native account picker (Android Credential Manager /
  /// iOS GoogleSignIn) → POST /v1/auth/social. Reuses the SAME signed-in path as
  /// [_submit]: the repository persists the returned JWT pair, then
  /// [authStateProvider.signedIn] flips the app to authed and the router redirect
  /// carries us to the app. A cancel (null session) or failure surfaces a precise
  /// message; on success we leave _busy set, as navigation follows.
  Future<void> _google() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final session = await ref.read(authRepositoryProvider).loginWithGoogle();
      if (!mounted) return;
      if (session == null) {
        setState(() {
          _busy = false;
          _error = 'Google sign-in was cancelled.';
        });
        return;
      }
      // Flip auth state → the router redirect carries us to the intended route.
      ref.read(authStateProvider.notifier).signedIn(session);
    } catch (_) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _error = 'Google sign-in failed. Try again.';
      });
    }
  }

  /// "Sign in with Apple" (SIWA-R1) → a native qcue session via the same signed-in path as
  /// [_google]. Presents the system Apple ID sheet, exchanges the identity token at
  /// POST /v1/auth/social (provider="apple"), then flips auth state. A cancel (null session) or
  /// failure surfaces a precise message.
  Future<void> _apple() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final session = await ref.read(authRepositoryProvider).loginWithApple();
      if (!mounted) return;
      if (session == null) {
        setState(() {
          _busy = false;
          _error = 'Apple sign-in was cancelled.';
        });
        return;
      }
      // Flip auth state → the router redirect carries us to the intended route.
      ref.read(authStateProvider.notifier).signedIn(session);
    } catch (_) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _error = 'Apple sign-in failed. Try again.';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final q = context.q;
    return Scaffold(
      backgroundColor: q.bg,
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(QSpace.lg),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 420),
              child: Column(
                key: const ValueKey('login-screen'),
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text('Sign in',
                      style: QCueText.title.copyWith(color: q.text)),
                  const SizedBox(height: QSpace.sm),
                  Text(
                    'Sign in to sync your captures to your server.',
                    style: QCueText.body.copyWith(color: q.text2),
                  ),
                  const SizedBox(height: QSpace.lg),
                  TextField(
                    key: const ValueKey('login-email'),
                    controller: _email,
                    enabled: !_busy,
                    keyboardType: TextInputType.emailAddress,
                    autocorrect: false,
                    textInputAction: TextInputAction.next,
                    style: QCueText.body.copyWith(color: q.text),
                    decoration: _decoration(context, 'Email'),
                  ),
                  const SizedBox(height: QSpace.md),
                  TextField(
                    key: const ValueKey('login-password'),
                    controller: _password,
                    enabled: !_busy,
                    obscureText: true,
                    textInputAction: TextInputAction.done,
                    onSubmitted: (_) => _submit(),
                    style: QCueText.body.copyWith(color: q.text),
                    decoration: _decoration(context, 'Password'),
                  ),
                  if (_error != null) ...[
                    const SizedBox(height: QSpace.md),
                    Text(
                      _error!,
                      key: const ValueKey('login-error'),
                      style: QCueText.caption.copyWith(color: q.danger),
                    ),
                  ],
                  const SizedBox(height: QSpace.lg),
                  FilledButton(
                    key: const ValueKey('login-submit'),
                    onPressed: _busy ? null : _submit,
                    style: FilledButton.styleFrom(
                      backgroundColor: q.accent,
                      padding:
                          const EdgeInsets.symmetric(vertical: QSpace.md),
                    ),
                    child: _busy
                        ? const SizedBox(
                            height: 18,
                            width: 18,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Text('Sign in'),
                  ),
                  const SizedBox(height: QSpace.md),
                  Row(
                    children: [
                      Expanded(child: Divider(color: q.border)),
                      Padding(
                        padding: const EdgeInsets.symmetric(
                            horizontal: QSpace.sm),
                        child: Text('or',
                            style:
                                QCueText.caption.copyWith(color: q.text3)),
                      ),
                      Expanded(child: Divider(color: q.border)),
                    ],
                  ),
                  const SizedBox(height: QSpace.md),
                  OutlinedButton.icon(
                    key: const ValueKey('login-google'),
                    onPressed: _busy ? null : _google,
                    icon: const Icon(Icons.login, size: 18),
                    label: const Text('Sign in with Google'),
                    style: OutlinedButton.styleFrom(
                      foregroundColor: q.text,
                      side: BorderSide(color: q.border),
                      padding:
                          const EdgeInsets.symmetric(vertical: QSpace.md),
                    ),
                  ),
                  // SIWA-R1: Apple requires "Sign in with Apple" alongside Google on iOS
                  // (Guideline 4.8). iOS-only — Android keeps Google as the sole social option.
                  if (Platform.isIOS) ...[
                    const SizedBox(height: QSpace.md),
                    OutlinedButton.icon(
                      key: const ValueKey('login-apple'),
                      onPressed: _busy ? null : _apple,
                      icon: const Icon(Icons.apple, size: 20),
                      label: const Text('Sign in with Apple'),
                      style: OutlinedButton.styleFrom(
                        foregroundColor: q.text,
                        side: BorderSide(color: q.border),
                        padding:
                            const EdgeInsets.symmetric(vertical: QSpace.md),
                      ),
                    ),
                  ],
                  const SizedBox(height: QSpace.md),
                  TextButton(
                    key: const ValueKey('login-signup-link'),
                    onPressed: _busy ? null : () => context.go('/signup'),
                    child: Text(
                      "Don't have an account? Create account",
                      style: QCueText.body.copyWith(color: q.accent),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  InputDecoration _decoration(BuildContext context, String label) {
    final q = context.q;
    return InputDecoration(
      labelText: label,
      labelStyle: QCueText.body.copyWith(color: q.text3),
      filled: true,
      fillColor: q.surface,
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(QRadius.input),
        borderSide: BorderSide(color: q.border),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(QRadius.input),
        borderSide: BorderSide(color: q.accent),
      ),
    );
  }
}
