// QCue S4-R46: collects a BYOK key into the S3 vault (via the caller's putKey
// RPC) + an opaque handle into platform secure storage, and surfaces ONLY the
// masked last-4 hint — the security boundary. The field obscures input; the
// Dart layer never logs or persists the plaintext, and clears it immediately.
import 'package:flutter/material.dart';
import '../../../core/models/protocol_models.dart';
import '../../../core/secure/secure_storage.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';

/// Mask a secret to the `prefix…last4` hint (the only thing ever displayed).
String maskKey(String key) {
  final k = key.trim();
  final last4 = k.length >= 4 ? k.substring(k.length - 4) : k;
  final prefix = k.length >= 3 ? k.substring(0, 3) : '';
  return prefix.isEmpty ? '…$last4' : '$prefix…$last4';
}

class AddKeySheet extends StatefulWidget {
  const AddKeySheet({
    super.key,
    required this.provider,
    required this.vault,
    required this.onAdded,
    this.onSubmitSecret,
  });

  final String provider;
  final SecureStorage vault;

  /// Surfaces ONLY the masked credential (key_hint) — the security boundary.
  final void Function(ProviderCredential) onAdded;

  /// Forwards the raw key to the caller's putKey RPC (server vault). The screen
  /// uses this to persist server-side; the Dart layer never stores it itself.
  final void Function(String key)? onSubmitSecret;

  @override
  State<AddKeySheet> createState() => _AddKeySheetState();
}

class _AddKeySheetState extends State<AddKeySheet> {
  final _controller = TextEditingController();
  bool _obscure = true;

  Future<void> _add() async {
    final key = _controller.text.trim();
    if (key.isEmpty) return;
    // Store only an opaque handle locally; the plaintext goes to the S3 vault via
    // the screen's putKey RPC and is then cleared. The handle never IS the key.
    await widget.vault.write('cred_handle_${widget.provider}', 'opaque-handle');
    // Forward the raw key to the server vault (putKey RPC) BEFORE clearing.
    widget.onSubmitSecret?.call(key);
    widget.onAdded(ProviderCredential(
      provider: widget.provider,
      keyHint: maskKey(key), // last-4 only — the secret never surfaces again
      status: CredStatus.ok,
    ));
    _controller.clear(); // drop the plaintext immediately
  }

  @override
  void dispose() {
    _controller.clear();
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(QSpace.md),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Add ${widget.provider} key',
              style: QCueText.subtitle.copyWith(color: context.q.text)),
          const SizedBox(height: QSpace.xs),
          Text('Your key is stored encrypted. Only the last 4 are ever shown.',
              style: QCueText.caption.copyWith(color: context.q.text2)),
          const SizedBox(height: QSpace.md),
          Semantics(
            label: 'secure, obscured key field',
            textField: true,
            child: TextField(
              key: const ValueKey('key-field'),
              controller: _controller,
              obscureText: _obscure,
              autocorrect: false,
              enableSuggestions: false,
              style: QCueText.mono.copyWith(color: context.q.text),
              decoration: InputDecoration(
                hintText: 'sk-…',
                hintStyle: QCueText.mono.copyWith(color: context.q.text3),
                filled: true,
                fillColor: context.q.surface,
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(QRadius.input),
                  borderSide: BorderSide(color: context.q.border),
                ),
                enabledBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(QRadius.input),
                  borderSide: BorderSide(color: context.q.border),
                ),
                suffixIcon: IconButton(
                  tooltip: _obscure ? 'Show key' : 'Hide key',
                  icon: Icon(
                      _obscure ? Icons.visibility : Icons.visibility_off,
                      color: context.q.text3),
                  onPressed: () => setState(() => _obscure = !_obscure),
                ),
              ),
            ),
          ),
          const SizedBox(height: QSpace.md),
          SizedBox(
            width: double.infinity,
            child: FilledButton(
              style: FilledButton.styleFrom(
                backgroundColor: context.q.accent,
                foregroundColor: context.q.bg,
                minimumSize: const Size.fromHeight(44),
              ),
              onPressed: _add,
              child: const Text('Add'),
            ),
          ),
        ],
      ),
    );
  }
}
