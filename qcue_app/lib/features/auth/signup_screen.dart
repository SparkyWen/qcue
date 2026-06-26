// QCue v0.1.1 (WS-A2): the email/password sign-up screen. Mirrors [LoginScreen]
// (same QCue design tokens: context.q / QCueText / QSpace), but POSTs to
// [AuthRepository.signup]. On success the session JWT is persisted (durable
// token store) and the router redirects to the app shell — same as login.
// Errors are precise: a duplicate email reads "That email is already
// registered — sign in instead."; a network failure reads "can't reach the
// server — check the Server URL in Settings".
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import 'auth_provider.dart';
import 'auth_repository.dart';

class SignupScreen extends ConsumerStatefulWidget {
  const SignupScreen({super.key});

  @override
  ConsumerState<SignupScreen> createState() => _SignupScreenState();
}

class _SignupScreenState extends ConsumerState<SignupScreen> {
  final _email = TextEditingController();
  final _password = TextEditingController();
  final _confirm = TextEditingController();
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _email.dispose();
    _password.dispose();
    _confirm.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_busy) return;
    final email = _email.text.trim();
    final password = _password.text;
    final confirm = _confirm.text;
    if (email.isEmpty || password.isEmpty) {
      setState(() => _error = 'Enter your email and password.');
      return;
    }
    if (password != confirm) {
      setState(() => _error = "Passwords don't match.");
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final session =
          await ref.read(authRepositoryProvider).signup(email, password);
      if (!mounted) return;
      // Flip auth state → the router redirect carries us to the app shell.
      ref.read(authStateProvider.notifier).signedIn(session);
    } on AuthError catch (e) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _error = switch (e.kind) {
          AuthErrorKind.emailTaken =>
            'That email is already registered — sign in instead.',
          AuthErrorKind.invalidCredentials => 'Enter your email and password.',
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
                key: const ValueKey('signup-screen'),
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text('Create account',
                      style: QCueText.title.copyWith(color: q.text)),
                  const SizedBox(height: QSpace.sm),
                  Text(
                    'Create an account to sync your captures to your server.',
                    style: QCueText.body.copyWith(color: q.text2),
                  ),
                  const SizedBox(height: QSpace.lg),
                  TextField(
                    key: const ValueKey('signup-email'),
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
                    key: const ValueKey('signup-password'),
                    controller: _password,
                    enabled: !_busy,
                    obscureText: true,
                    textInputAction: TextInputAction.next,
                    style: QCueText.body.copyWith(color: q.text),
                    decoration: _decoration(context, 'Password'),
                  ),
                  const SizedBox(height: QSpace.md),
                  TextField(
                    key: const ValueKey('signup-confirm'),
                    controller: _confirm,
                    enabled: !_busy,
                    obscureText: true,
                    textInputAction: TextInputAction.done,
                    onSubmitted: (_) => _submit(),
                    style: QCueText.body.copyWith(color: q.text),
                    decoration: _decoration(context, 'Confirm password'),
                  ),
                  if (_error != null) ...[
                    const SizedBox(height: QSpace.md),
                    Text(
                      _error!,
                      key: const ValueKey('signup-error'),
                      style: QCueText.caption.copyWith(color: q.danger),
                    ),
                  ],
                  const SizedBox(height: QSpace.lg),
                  FilledButton(
                    key: const ValueKey('signup-submit'),
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
                        : const Text('Create account'),
                  ),
                  const SizedBox(height: QSpace.md),
                  TextButton(
                    key: const ValueKey('signup-login-link'),
                    onPressed: _busy ? null : () => context.go('/login'),
                    child: Text(
                      'Already have an account? Sign in',
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
