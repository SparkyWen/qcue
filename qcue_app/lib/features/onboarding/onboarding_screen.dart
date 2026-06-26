// QCue S4-R52: the first-run onboarding flow — Intro → SignIn → AddKey → Theme,
// skippable to a usable KEYLESS app (a user can continue without an account and
// without a key and still reach /capture). A plain step switch (no PageView
// animation) so it is reduced-motion- and widget-test-safe. The router wires
// [onDone] to navigate to /capture once complete.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import '../../core/theme/qcue_tokens.dart';
import '../../core/theme/theme_provider.dart';
import 'onboarding_flow.dart';

class OnboardingScreen extends ConsumerWidget {
  const OnboardingScreen({super.key, this.onDone});

  /// Called once onboarding completes — the router navigates to /capture.
  final VoidCallback? onDone;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final step = ref.watch(onboardingControllerProvider);
    final ctrl = ref.read(onboardingControllerProvider.notifier);
    return Scaffold(
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(QSpace.xl),
          child: switch (step) {
            OnboardingStep.intro => _Step(
                key: const ValueKey('onboarding-intro'),
                icon: Icons.bolt_outlined,
                title: 'Capture first, organize later',
                body: 'QCue catches fleeting ideas — typed or spoken — and '
                    'weaves them into a living wiki.',
                primaryLabel: 'Get started',
                onPrimary: ctrl.next,
              ),
            OnboardingStep.signIn => _Step(
                key: const ValueKey('onboarding-signin'),
                icon: Icons.cloud_outlined,
                title: 'Sync across your devices',
                body: 'Sign in to sync your captures to your account — or start '
                    'now without one; you can sign in any time from Settings.',
                primaryLabel: 'Continue without an account',
                onPrimary: ctrl.continueKeyless,
              ),
            OnboardingStep.addKey => _Step(
                key: const ValueKey('onboarding-addkey'),
                icon: Icons.vpn_key_outlined,
                title: 'Bring your own AI key',
                body: 'Distillation and recall use your own provider key (BYOK). '
                    'Add one in Settings whenever you’re ready.',
                primaryLabel: 'Skip for now',
                onPrimary: ctrl.skipKey,
              ),
            OnboardingStep.theme => _ThemeStep(
                key: const ValueKey('onboarding-theme'),
                onFinish: () async {
                  await ctrl.complete();
                  onDone?.call();
                },
              ),
            OnboardingStep.done => const SizedBox.shrink(),
          },
        ),
      ),
    );
  }
}

class _Step extends StatelessWidget {
  const _Step({
    super.key,
    required this.icon,
    required this.title,
    required this.body,
    required this.primaryLabel,
    required this.onPrimary,
  });
  final IconData icon;
  final String title;
  final String body;
  final String primaryLabel;
  final VoidCallback onPrimary;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Spacer(),
        Icon(icon, size: 48, color: context.q.accent),
        const SizedBox(height: QSpace.lg),
        Text(title, style: QCueText.title.copyWith(color: context.q.text)),
        const SizedBox(height: QSpace.sm),
        Text(body, style: QCueText.body.copyWith(color: context.q.text2)),
        const Spacer(),
        SizedBox(
          width: double.infinity,
          child: FilledButton(
            key: const ValueKey('onboarding-primary'),
            onPressed: onPrimary,
            style: FilledButton.styleFrom(
              minimumSize: const Size.fromHeight(48),
              backgroundColor: context.q.accent,
              foregroundColor: context.q.bg,
            ),
            child: Text(primaryLabel),
          ),
        ),
      ],
    );
  }
}

class _ThemeStep extends ConsumerWidget {
  const _ThemeStep({super.key, required this.onFinish});
  final Future<void> Function() onFinish;

  static const _labels = {
    QThemeId.cleanLight: 'Clean Light',
    QThemeId.anthropicWarm: 'Anthropic Warm',
    QThemeId.night: 'Night',
  };

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final current = ref.watch(themeProvider);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Spacer(),
        Icon(Icons.palette_outlined, size: 48, color: context.q.accent),
        const SizedBox(height: QSpace.lg),
        Text('Pick your look',
            style: QCueText.title.copyWith(color: context.q.text)),
        const SizedBox(height: QSpace.sm),
        Text('Change it any time in Settings.',
            style: QCueText.body.copyWith(color: context.q.text2)),
        const SizedBox(height: QSpace.lg),
        for (final id in QThemeId.values)
          Padding(
            padding: const EdgeInsets.only(bottom: QSpace.sm),
            child: InkWell(
              key: ValueKey('theme-$id'),
              onTap: () => ref.read(themeProvider.notifier).select(id),
              borderRadius: BorderRadius.circular(QRadius.control),
              child: ConstrainedBox(
                constraints: const BoxConstraints(minHeight: 44),
                child: Container(
                  padding: const EdgeInsets.symmetric(
                      horizontal: QSpace.md, vertical: QSpace.sm),
                  decoration: BoxDecoration(
                    border: Border.all(
                      color: id == current ? context.q.accent : context.q.border,
                      width: id == current ? 2 : 1,
                    ),
                    borderRadius: BorderRadius.circular(QRadius.control),
                  ),
                  child: Row(
                    children: [
                      Icon(
                        id == current
                            ? Icons.radio_button_checked
                            : Icons.radio_button_unchecked,
                        size: 18,
                        color: id == current
                            ? context.q.accent
                            : context.q.text3,
                      ),
                      const SizedBox(width: QSpace.sm),
                      Text(_labels[id]!,
                          style: QCueText.body.copyWith(color: context.q.text)),
                    ],
                  ),
                ),
              ),
            ),
          ),
        const Spacer(),
        SizedBox(
          width: double.infinity,
          child: FilledButton(
            key: const ValueKey('onboarding-finish'),
            onPressed: onFinish,
            style: FilledButton.styleFrom(
              minimumSize: const Size.fromHeight(48),
              backgroundColor: context.q.accent,
              foregroundColor: context.q.bg,
            ),
            child: const Text('Finish'),
          ),
        ),
      ],
    );
  }
}
