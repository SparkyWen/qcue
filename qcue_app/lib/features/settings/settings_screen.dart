// QCue S4: the Settings tab body — content-first, section headers + hairline
// dividers. Sections:
//   • Appearance — the live theme switcher (kept).
//   • Provider keys (BYOK) — the vault: each provider's masked key_hint + health
//     badge, an Add/Update key flow (obscured field; only key_hint ever shown,
//     NEVER the secret), and Delete.
//   • Models — the active model per provider (dropdown from the stubbed list).
//   • Usage & cost — the cost ledger table (tabular figures).
//   • Privacy — the D9 server-Dream posture toggle + a sign-out row.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../core/ingest/digest_provider.dart';
import '../../core/models/protocol_models.dart';
import '../../core/secure/secure_storage_provider.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import '../../core/session/auth_state.dart'; // app-global auth state (logout) — core, not the auth feature
import '../../core/update/software_update_section.dart';
import 'settings_provider.dart';
import 'widgets/add_key_sheet.dart';
import 'widgets/cost_ledger_table.dart';
import 'widgets/health_badge.dart';
import 'widgets/server_url_field.dart';
import 'widgets/theme_switcher.dart';

class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final async = ref.watch(settingsProvider);
    return ListView(
      key: const ValueKey('settings-screen'),
      padding: const EdgeInsets.only(bottom: QSpace.xl),
      children: [
        const _SectionHeader('Appearance'),
        const ThemeSwitcher(),
        const _Hairline(),
        const _SectionHeader('Connection'),
        const ServerUrlField(),
        const _Hairline(),
        // v0.2.2: Activity moved off the bottom bar to here — an openable row
        // that pushes the full Activity view (review/dream/jobs/cost).
        const _SectionHeader('Activity'),
        const _ActivityRow(),
        const _Hairline(),
        ...switch (async) {
          AsyncLoading() => const [
              Padding(
                padding: EdgeInsets.all(QSpace.xl),
                child: Center(child: CircularProgressIndicator()),
              ),
            ],
          AsyncError(:final error) => [
              Padding(
                padding: const EdgeInsets.all(QSpace.md),
                child: Text("Couldn't load settings · $error",
                    style: QCueText.body.copyWith(color: context.q.danger)),
              ),
            ],
          AsyncData(:final value) => _sections(context, ref, value),
          _ => const <Widget>[],
        },
      ],
    );
  }

  List<Widget> _sections(
      BuildContext context, WidgetRef ref, SettingsSnapshot s) {
    return [
      const _SectionHeader('Provider keys'),
      for (final c in s.credentials) _KeyRow(cred: c),
      _AddKeyRow(),
      const _Hairline(),
      const _SectionHeader('Models'),
      for (final c in s.credentials)
        _ModelPickerRow(
          provider: c.provider,
          models: s.models[c.provider] ?? const [],
          active: s.activeModels[c.provider],
        ),
      const _Hairline(),
      const _SectionHeader('Digest'),
      const _DigestRow(),
      const _Hairline(),
      const _SectionHeader('Usage & cost'),
      Padding(
        padding: const EdgeInsets.symmetric(vertical: QSpace.sm),
        child: CostLedgerTable(
          rows: s.costLedger,
          monthTotalMicros: s.monthTotalMicros,
          capMicros: s.capMicros,
        ),
      ),
      const _Hairline(),
      const _SectionHeader('Software update'),
      const SoftwareUpdateSection(),
      const _Hairline(),
      const _SectionHeader('Privacy'),
      _PrivacyRow(serverDream: s.serverDream),
      _LocationRow(enabled: s.captureLocationEnabled),
      const _SignOutRow(),
      const _DeleteAccountRow(),
    ];
  }
}

class _SectionHeader extends StatelessWidget {
  const _SectionHeader(this.label);
  final String label;
  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.fromLTRB(
            QSpace.md, QSpace.md, QSpace.md, QSpace.sm),
        child: Semantics(
          header: true,
          child: Text(label,
              style: QCueText.label.copyWith(color: context.q.text2)),
        ),
      );
}

class _Hairline extends StatelessWidget {
  const _Hairline();
  @override
  Widget build(BuildContext context) =>
      Divider(height: 1, color: context.q.border);
}

/// v0.2.2: the entry point to the Activity view, relocated from the bottom nav.
/// Pushes `/settings/activity` (the unchanged ActivityScreen) within the
/// Settings branch, so Back returns here and the bottom bar stays put.
class _ActivityRow extends StatelessWidget {
  const _ActivityRow();
  @override
  Widget build(BuildContext context) => ListTile(
        key: const ValueKey('settings-activity-row'),
        minVerticalPadding: 12,
        leading: Icon(Icons.list_alt_outlined, color: context.q.accent),
        title: Text('Review & jobs',
            style: QCueText.body.copyWith(color: context.q.text)),
        subtitle: Text("Approvals, dreams, job history & today's cost",
            style: QCueText.caption.copyWith(color: context.q.text3)),
        trailing: Icon(Icons.chevron_right, color: context.q.text3),
        onTap: () => context.go('/settings/activity'),
      );
}

/// One vault row: provider, masked key_hint (mono), health badge, Delete.
class _KeyRow extends ConsumerWidget {
  const _KeyRow({required this.cred});
  final ProviderCredential cred;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Padding(
      padding:
          const EdgeInsets.symmetric(horizontal: QSpace.md, vertical: QSpace.sm),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(cred.provider,
                    style: QCueText.body.copyWith(color: context.q.text)),
                const SizedBox(height: 2),
                Row(
                  children: [
                    // The ONLY thing the vault ever displays (never the secret).
                    Text(cred.keyHint,
                        style: QCueText.mono.copyWith(
                            color: context.q.text3, fontSize: 13)),
                    const SizedBox(width: QSpace.sm),
                    HealthBadge(
                        status: cred.status,
                        cooldownUntil: cred.cooldownUntil),
                  ],
                ),
              ],
            ),
          ),
          IconButton(
            tooltip: 'Delete ${cred.provider} key',
            icon: Icon(Icons.delete_outline, color: context.q.text3, size: 20),
            onPressed: () => _confirmDelete(context, ref),
          ),
        ],
      ),
    );
  }

  Future<void> _confirmDelete(BuildContext context, WidgetRef ref) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Remove ${cred.provider} key?'),
        content: Text(
          'Recall and Dream that use ${cred.provider} will stop until you add a '
          'new key.',
          style: QCueText.body.copyWith(color: ctx.q.text2),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: Text('Cancel', style: TextStyle(color: ctx.q.text2)),
          ),
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(true),
            child: Text('Remove', style: TextStyle(color: ctx.q.danger)),
          ),
        ],
      ),
    );
    if (ok == true) {
      await ref.read(settingsProvider.notifier).deleteKey(cred.provider);
    }
  }
}

/// DIG-R6: the one-click incremental Digest action (moved here from the Wiki screen). Runs the digest,
/// disables + shows a spinner while in flight, and surfaces the enqueued count via a SnackBar. The
/// digested pages appear on the Wiki via the read-sync → cache-revision refresh (no app relaunch).
class _DigestRow extends ConsumerWidget {
  const _DigestRow();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(digestProvider);
    final running = state is DigestRunning;

    // Surface the outcome once, when a run completes.
    ref.listen<DigestState>(digestProvider, (prev, next) {
      if (next is DigestDone) {
        final n = next.enqueued;
        final msg = n == 0
            ? 'Nothing new to digest.'
            : 'Digesting $n capture${n == 1 ? '' : 's'}…';
        ScaffoldMessenger.of(context)
          ..hideCurrentSnackBar()
          ..showSnackBar(SnackBar(content: Text(msg)));
      } else if (next is DigestFailed) {
        ScaffoldMessenger.of(context)
          ..hideCurrentSnackBar()
          ..showSnackBar(const SnackBar(content: Text("Couldn't start the digest.")));
      }
    });

    return ListTile(
      key: const ValueKey('settings-digest'),
      minVerticalPadding: 12,
      leading: running
          ? SizedBox(
              width: 22,
              height: 22,
              child: CircularProgressIndicator(
                  strokeWidth: 2, color: context.q.accent),
            )
          : Icon(Icons.auto_awesome_outlined, color: context.q.accent),
      title: Text(running ? 'Digesting…' : 'Digest now',
          style: QCueText.body.copyWith(color: context.q.accent)),
      subtitle: Text('Distil your new captures into the wiki',
          style: QCueText.caption.copyWith(color: context.q.text3)),
      onTap: running ? null : () => ref.read(digestProvider.notifier).run(),
    );
  }
}

/// The "Add / update a key" affordance — opens the obscured AddKeySheet.
class _AddKeyRow extends ConsumerWidget {
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return ListTile(
      key: const ValueKey('add-key-row'),
      minVerticalPadding: 12,
      leading: Icon(Icons.add, color: context.q.accent),
      title: Text('Add or update a key',
          style: QCueText.body.copyWith(color: context.q.accent)),
      onTap: () => _openSheet(context, ref),
    );
  }

  Future<void> _openSheet(BuildContext context, WidgetRef ref) async {
    // A minimal provider chooser; launch providers (master D7) without a key yet.
    const providers = ['openai', 'anthropic', 'gemini', 'deepseek', 'openrouter'];
    final provider = await showModalBottomSheet<String>(
      context: context,
      builder: (ctx) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final p in providers)
              ListTile(
                title: Text(p, style: QCueText.body.copyWith(color: ctx.q.text)),
                onTap: () => Navigator.of(ctx).pop(p),
              ),
          ],
        ),
      ),
    );
    if (provider == null || !context.mounted) return;
    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      builder: (ctx) => Padding(
        padding: EdgeInsets.only(
            bottom: MediaQuery.of(ctx).viewInsets.bottom),
        child: AddKeySheet(
          provider: provider,
          // The device-cached BYOK key store (D9): NullSecureStorage under tests,
          // the native Keychain/Keystore store on device (overridden at bootstrap).
          vault: ref.read(secureStorageProvider),
          // The plaintext goes straight to the S3 vault via putKey; only the
          // masked key_hint ever comes back into the UI.
          onSubmitSecret: (key) =>
              ref.read(settingsProvider.notifier).putKey(provider, key),
          onAdded: (_) => Navigator.of(ctx).pop(),
        ),
      ),
    );
  }
}

/// The active-model dropdown for a provider.
class _ModelPickerRow extends ConsumerWidget {
  const _ModelPickerRow({
    required this.provider,
    required this.models,
    required this.active,
  });
  final String provider;
  final List<String> models;
  final String? active;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Padding(
      padding:
          const EdgeInsets.symmetric(horizontal: QSpace.md, vertical: QSpace.xs),
      child: Row(
        children: [
          Expanded(
            child: Text(provider,
                style: QCueText.body.copyWith(color: context.q.text)),
          ),
          if (models.isEmpty)
            Text('—', style: QCueText.body.copyWith(color: context.q.text3))
          else
            DropdownButton<String>(
              key: ValueKey('model-picker-$provider'),
              value: active != null && models.contains(active)
                  ? active
                  : models.first,
              underline: const SizedBox.shrink(),
              style: QCueText.mono.copyWith(color: context.q.text, fontSize: 14),
              dropdownColor: context.q.surface,
              items: [
                for (final m in models)
                  DropdownMenuItem(value: m, child: Text(m)),
              ],
              onChanged: (m) {
                if (m != null) {
                  ref
                      .read(settingsProvider.notifier)
                      .setActiveModel(provider, m);
                }
              },
            ),
        ],
      ),
    );
  }
}

/// The D9 server-readable / server-Dream posture toggle.
class _PrivacyRow extends ConsumerWidget {
  const _PrivacyRow({required this.serverDream});
  final bool serverDream;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return SwitchListTile(
      key: const ValueKey('server-dream-switch'),
      value: serverDream,
      activeThumbColor: context.q.accent,
      title: Text('Server-side nightly Dream',
          style: QCueText.body.copyWith(color: context.q.text)),
      subtitle: Text(
        'Let the server consolidate your wiki overnight while your phone sleeps. '
        'Turn off to keep consolidation on-device only.',
        style: QCueText.caption.copyWith(color: context.q.text2),
      ),
      onChanged: (on) =>
          ref.read(settingsProvider.notifier).setServerDream(on),
    );
  }
}

/// LOC-R2: the device-local "tag captures with location" toggle. Off by default;
/// when on, the capture funnel fetches a single action-time GPS fix per capture
/// (used only while this is on). Mirrors [_PrivacyRow].
class _LocationRow extends ConsumerWidget {
  const _LocationRow({required this.enabled});
  final bool enabled;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return SwitchListTile(
      key: const ValueKey('capture-location-switch'),
      value: enabled,
      activeThumbColor: context.q.accent,
      title: Text('Tag captures with location',
          style: QCueText.body.copyWith(color: context.q.text)),
      subtitle: Text(
        'Off by default. When on, each new capture records where you made it '
        '(precise GPS). Used only while this is on; turn it off any time.',
        style: QCueText.caption.copyWith(color: context.q.text2),
      ),
      onChanged: (on) =>
          ref.read(settingsProvider.notifier).setCaptureLocation(on),
    );
  }
}

/// The account / sign-out row (sign-out is a stub this milestone).
class _SignOutRow extends ConsumerWidget {
  const _SignOutRow();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return ListTile(
      minVerticalPadding: 12,
      leading: Icon(Icons.logout, color: context.q.danger),
      title: Text('Sign out',
          style: QCueText.body.copyWith(color: context.q.danger)),
      // Clears the durable token pair (best-effort server revoke) + the local
      // session, then the router redirect carries us back to /login (Task 5).
      onTap: () => ref.read(authStateProvider.notifier).signOut(),
    );
  }
}

/// Apple Guideline 5.1.1(v): the in-app account-deletion row. A destructive
/// confirm gate spells out the irreversible consequence; on confirm the account
/// is purged server-side (DELETE /v1/account) and the app signs out + wipes the
/// local cache, so the router redirect lands on /login.
class _DeleteAccountRow extends ConsumerWidget {
  const _DeleteAccountRow();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return ListTile(
      minVerticalPadding: 12,
      leading: Icon(Icons.delete_forever, color: context.q.danger),
      title: Text('Delete account',
          style: QCueText.body.copyWith(color: context.q.danger)),
      onTap: () => _confirm(context, ref),
    );
  }

  Future<void> _confirm(BuildContext context, WidgetRef ref) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete account?'),
        content: Text(
          'This permanently deletes your account and all synced data. '
          "This can't be undone.",
          style: QCueText.body.copyWith(color: ctx.q.text2),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: Text('Cancel', style: TextStyle(color: ctx.q.text2)),
          ),
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(true),
            child: Text('Delete', style: TextStyle(color: ctx.q.danger)),
          ),
        ],
      ),
    );
    if (ok == true) {
      try {
        await ref.read(settingsProvider.notifier).deleteAccount();
        // On success signOut() flips auth → unauthed and the router redirects to
        // /login; this widget is torn down, so there is nothing more to show.
      } catch (_) {
        // Never leave the user believing a failed delete succeeded (privacy
        // footgun): surface it so they can retry. Still signed in / not deleted.
        if (context.mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
              content: Text(
                "Couldn't delete your account — check your connection and try again.",
              ),
            ),
          );
        }
      }
    }
  }
}
