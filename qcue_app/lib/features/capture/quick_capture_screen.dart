// QCue S4-R51/R8: the in-app quick-capture compose screen (the widget/notification
// `qcue://capture/compose` deep-link target, and a focused full-screen compose).
// ONE capture path: it commits through the SAME captureFeedProvider sink as the
// bottom field, share, and widget (origin='compose'), so every capture path is
// unified + offline-safe (S4-R51). S4-R8: closing with unsaved text asks to
// confirm the discard (PopScope guards the system back gesture too).
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import '../../core/theme/theme_provider.dart';
import 'capture_provider.dart';

class QuickCaptureScreen extends ConsumerStatefulWidget {
  const QuickCaptureScreen({super.key});

  @override
  ConsumerState<QuickCaptureScreen> createState() => _QuickCaptureScreenState();
}

class _QuickCaptureScreenState extends ConsumerState<QuickCaptureScreen> {
  final _controller = TextEditingController();
  bool _submitting = false;

  @override
  void initState() {
    super.initState();
    _controller.addListener(_onChanged);
  }

  void _onChanged() => setState(() {});

  @override
  void dispose() {
    _controller.removeListener(_onChanged);
    _controller.dispose();
    super.dispose();
  }

  bool get _hasText => _controller.text.trim().isNotEmpty;

  Future<void> _capture() async {
    final body = _controller.text.trim();
    if (body.isEmpty || _submitting) return;
    setState(() => _submitting = true);
    // Same sink as the bottom field / share / widget — offline-safe (S4-R51).
    await ref
        .read(captureFeedProvider.notifier)
        .commit(body: body, origin: 'compose');
    ref.read(hapticsProvider).captureCommitted();
    _controller.clear();
    if (mounted) Navigator.of(context).maybePop();
  }

  /// S4-R8: confirm before discarding unsaved text. Returns true to proceed.
  Future<bool> _confirmDiscard() async {
    if (!_hasText) return true;
    final discard = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Discard capture?'),
        content: const Text('Your unsaved note will be lost.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: const Text('Keep editing'),
          ),
          TextButton(
            key: const ValueKey('compose-discard-confirm'),
            onPressed: () => Navigator.of(ctx).pop(true),
            child: const Text('Discard'),
          ),
        ],
      ),
    );
    return discard ?? false;
  }

  @override
  Widget build(BuildContext context) {
    return PopScope(
      canPop: !_hasText,
      onPopInvokedWithResult: (didPop, _) async {
        if (didPop) return;
        if (await _confirmDiscard() && context.mounted) {
          Navigator.of(context).maybePop();
        }
      },
      child: Scaffold(
        appBar: AppBar(
          title: const Text('Quick capture'),
          leading: IconButton(
            key: const ValueKey('compose-close'),
            icon: const Icon(Icons.close),
            tooltip: 'Close',
            onPressed: () async {
              if (await _confirmDiscard() && context.mounted) {
                Navigator.of(context).maybePop();
              }
            },
          ),
          actions: [
            TextButton(
              key: const ValueKey('compose-submit'),
              onPressed: _hasText && !_submitting ? _capture : null,
              child: const Text('Capture'),
            ),
          ],
        ),
        body: Padding(
          padding: const EdgeInsets.all(QSpace.md),
          child: TextField(
            key: const ValueKey('compose-input'),
            controller: _controller,
            autofocus: true,
            minLines: 4,
            maxLines: null,
            keyboardType: TextInputType.multiline,
            textCapitalization: TextCapitalization.sentences,
            style: QCueText.body.copyWith(color: context.q.text),
            decoration: InputDecoration(
              hintText: 'Capture a thought…',
              hintStyle: QCueText.body.copyWith(color: context.q.text3),
              border: InputBorder.none,
            ),
          ),
        ),
      ),
    );
  }
}
