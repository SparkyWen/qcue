// AU-R19 — the blocking "Update required" screen (current build < min_supported_build). The only thing
// shown; the CTA downloads+installs (Android) / opens the App Store (iOS).
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../theme/qcue_space.dart';
import '../theme/qcue_text.dart';
import '../theme/qcue_theme.dart';
import 'apk_installer.dart';
import 'update_service.dart';
import 'update_state.dart';

class UpdateRequiredScreen extends ConsumerWidget {
  const UpdateRequiredScreen({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(updateProvider).value;
    final manifest = state is UpdateRequired ? state.manifest : null;
    return Scaffold(
      backgroundColor: context.q.bg,
      body: Center(
        key: const ValueKey('update-required'),
        child: Padding(
          padding: const EdgeInsets.all(QSpace.xl),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(Icons.system_update, size: 48, color: context.q.accent),
              const SizedBox(height: QSpace.md),
              Text('Update required', style: QCueText.title.copyWith(color: context.q.text)),
              const SizedBox(height: QSpace.sm),
              Text(
                'This version is no longer supported. Update to keep using QCue.',
                textAlign: TextAlign.center,
                style: QCueText.body.copyWith(color: context.q.text2),
              ),
              const SizedBox(height: QSpace.lg),
              if (manifest != null)
                FilledButton(
                  key: const ValueKey('update-required-cta'),
                  onPressed: () => applyFullUpdate(ref, manifest, context),
                  child: const Text('Update now'),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
