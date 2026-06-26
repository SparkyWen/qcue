// QCue cloud-sync fix (Task 4): the Settings "Server URL" field. It writes the
// runtime override (ServerUrlStore) and re-probes `/readyz` so the user can
// point the app at a deployed server without a rebuild. Shows the current
// effective URL + the live connectivity state. An invalid URL is rejected
// in-line (the override is only persisted when it parses as http(s)).
//
// The new URL takes full effect on the next app launch (the api client + config
// are bound at bootstrap); the field says so and re-probes immediately so the
// connectivity dot reflects the new host right away.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

// One sanctioned net seam (S4-R1): QcueConfig + qcueConfigProvider + the
// ServerUrlStore (+ provider) all come through the bridge — no direct core/net
// or cross-feature import.
import '../../../core/net/api_client_provider.dart';
import '../../../core/offline/connectivity.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';

class ServerUrlField extends ConsumerStatefulWidget {
  const ServerUrlField({super.key});

  @override
  ConsumerState<ServerUrlField> createState() => _ServerUrlFieldState();
}

class _ServerUrlFieldState extends ConsumerState<ServerUrlField> {
  late final TextEditingController _controller;
  String? _error;
  bool _saved = false;

  @override
  void initState() {
    super.initState();
    // Seed with the current effective base URL so the user edits the live value.
    final config = ref.read(qcueConfigProvider);
    _controller = TextEditingController(text: config.baseUrl);
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final url = _controller.text.trim();
    if (!QcueConfig.isValidBaseUrl(url)) {
      setState(() {
        _error = 'Enter an https:// URL, e.g. https://api.example.com '
            '(http:// is only allowed for localhost).';
        _saved = false;
      });
      return;
    }
    await ref.read(serverUrlStoreProvider).write(url);
    // Re-probe reachability against the (current) config; the new URL fully
    // applies on next launch, but the probe gives immediate feedback.
    await ref.read(connectivityProvider.notifier).probe();
    if (!mounted) return;
    setState(() {
      _error = null;
      _saved = true;
    });
  }

  @override
  Widget build(BuildContext context) {
    final q = context.q;
    final config = ref.watch(qcueConfigProvider);
    final online = ref.watch(connectivityProvider) == Connectivity.online;

    return Padding(
      key: const ValueKey('server-url-field'),
      padding:
          const EdgeInsets.symmetric(horizontal: QSpace.md, vertical: QSpace.sm),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          TextField(
            key: const ValueKey('server-url-input'),
            controller: _controller,
            keyboardType: TextInputType.url,
            autocorrect: false,
            style: QCueText.mono.copyWith(color: q.text, fontSize: 14),
            decoration: InputDecoration(
              labelText: 'Server URL',
              labelStyle: QCueText.body.copyWith(color: q.text3),
              isDense: true,
              filled: true,
              fillColor: q.surface,
              suffixIcon: TextButton(
                key: const ValueKey('server-url-save'),
                onPressed: _save,
                child: Text('Save',
                    style: QCueText.label.copyWith(color: q.accent)),
              ),
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(QRadius.input),
                borderSide: BorderSide(color: q.border),
              ),
              enabledBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(QRadius.input),
                borderSide: BorderSide(color: q.border),
              ),
            ),
          ),
          const SizedBox(height: QSpace.xs),
          Row(
            children: [
              Icon(online ? Icons.cloud_done_outlined : Icons.cloud_off_outlined,
                  size: 14, color: online ? q.success : q.text3),
              const SizedBox(width: QSpace.xs),
              Expanded(
                child: Text(
                  online
                      ? 'Connected · ${config.baseUrl}'
                      : 'Not reachable · ${config.baseUrl}',
                  style: QCueText.caption.copyWith(color: q.text2),
                ),
              ),
            ],
          ),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.only(top: QSpace.xs),
              child: Text(_error!,
                  key: const ValueKey('server-url-error'),
                  style: QCueText.caption.copyWith(color: q.danger)),
            ),
          if (_saved)
            Padding(
              padding: const EdgeInsets.only(top: QSpace.xs),
              child: Text(
                'Saved — restart the app to use the new server.',
                key: const ValueKey('server-url-saved'),
                style: QCueText.caption.copyWith(color: q.text2),
              ),
            ),
        ],
      ),
    );
  }
}
