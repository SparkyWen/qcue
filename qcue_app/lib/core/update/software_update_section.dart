// AU-R18 — the Settings "Software update" section: current version + patch, a status line, a manual
// "Check for updates" action, the auto-check toggle, and the changelog. All colors via context.q tokens.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../models/app_release_manifest.dart';
import '../native/codepush/code_push_facade.dart';
import '../theme/qcue_space.dart';
import '../theme/qcue_text.dart';
import '../theme/qcue_theme.dart';
import 'apk_installer.dart';
import 'update_service.dart';
import 'update_state.dart';

class SoftwareUpdateSection extends ConsumerWidget {
  const SoftwareUpdateSection({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final async = ref.watch(updateProvider);
    final currentBuild = ref.watch(currentBuildProvider);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: QSpace.md, vertical: QSpace.xs),
          child: Text('Version (build $currentBuild)',
              style: QCueText.caption.copyWith(color: context.q.text3)),
        ),
        ListTile(
          key: const ValueKey('update-status-row'),
          minVerticalPadding: 12,
          leading: Icon(Icons.system_update_outlined, color: context.q.accent),
          title: Text(_statusTitle(async), style: QCueText.body.copyWith(color: context.q.text)),
          subtitle: _subtitle(context, async),
          trailing: async.isLoading
              ? SizedBox(
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(strokeWidth: 2, color: context.q.accent),
                )
              : TextButton(
                  key: const ValueKey('check-for-updates'),
                  onPressed: () => ref.read(updateProvider.notifier).checkNow(),
                  child: Text('Check', style: TextStyle(color: context.q.accent)),
                ),
        ),
        // The nudge action (Android download+install / iOS App Store), only when a full update exists.
        if (async.value is FullUpdateAvailable)
          _UpdateCtaRow(manifest: (async.value! as FullUpdateAvailable).manifest),
        SwitchListTile(
          key: const ValueKey('auto-update-switch'),
          value: ref.watch(autoCheckProvider),
          activeThumbColor: context.q.accent,
          title: Text('Automatic update check',
              style: QCueText.body.copyWith(color: context.q.text)),
          subtitle: Text(
            'Check for new versions on launch. Seamless code-push updates always apply automatically.',
            style: QCueText.caption.copyWith(color: context.q.text2),
          ),
          onChanged: (on) => ref.read(autoCheckProvider.notifier).set(on),
        ),
      ],
    );
  }

  String _statusTitle(AsyncValue<UpdateState> a) => switch (a) {
        AsyncLoading() => 'Checking for updates…',
        AsyncError() => "Couldn't check for updates",
        AsyncData(:final value) => switch (value) {
            UpToDate() => 'Up to date',
            PatchReady() => 'Update ready — applies next launch',
            FullUpdateAvailable() => 'New version available',
            UpdateRequired() => 'Update required',
          },
        _ => 'Up to date',
      };

  Widget? _subtitle(BuildContext context, AsyncValue<UpdateState> a) {
    final v = a.value;
    if (v is FullUpdateAvailable) {
      return Text(
        v.manifest.changelog.isEmpty ? v.manifest.latestVersion : v.manifest.changelog,
        style: QCueText.caption.copyWith(color: context.q.text3),
      );
    }
    if (v is PatchReady && v.patchNumber != null) {
      return Text('Patch ${v.patchNumber}',
          style: QCueText.caption.copyWith(color: context.q.text3));
    }
    return null;
  }
}

/// The "download & install" (Android) / "open App Store" (iOS) CTA, shown when a full update exists.
class _UpdateCtaRow extends ConsumerWidget {
  const _UpdateCtaRow({required this.manifest});
  final AppReleaseManifest manifest;
  @override
  Widget build(BuildContext context, WidgetRef ref) => ListTile(
        key: const ValueKey('update-cta'),
        minVerticalPadding: 12,
        leading: Icon(Icons.download_outlined, color: context.q.accent),
        title: Text('Download & install',
            style: QCueText.body.copyWith(color: context.q.accent)),
        onTap: () => applyFullUpdate(ref, manifest, context),
      );
}
