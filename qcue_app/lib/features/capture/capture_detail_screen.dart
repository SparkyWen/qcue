// QCue CAP-R1/R2/R3: the capture detail view. Tap a feed row to inspect one
// capture's body, its captured-at time (absolute, local), its location (or "No
// location"), and its ingest state — then Edit (in-place body edit) or Delete
// (confirmed). Mirrors WikiPageScreen's ScreenState 4-state switch; semantic
// tokens only, no raw hex. Edit/Delete route through CaptureFeedNotifier so the
// feed + this detail stay in sync.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../core/models/protocol_models.dart';
import '../../core/models/screen_state.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import 'capture_provider.dart';

class CaptureDetailScreen extends ConsumerWidget {
  const CaptureDetailScreen({super.key, required this.id});
  final String id;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final async = ref.watch(captureDetailProvider(id));
    return Scaffold(
      backgroundColor: context.q.bg,
      appBar: AppBar(title: const Text('Capture')),
      body: async.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (e, _) => _DetailError(e.toString()),
        data: (state) => switch (state) {
          Empty() => const Center(
            key: ValueKey('capture-gone'),
            child: Text('This capture is gone.'),
          ),
          Loading() => const Center(child: CircularProgressIndicator()),
          ErrorState(:final message) => _DetailError(message),
          Data(:final value) => _Body(idea: value, id: id),
        },
      ),
    );
  }
}

class _Body extends ConsumerWidget {
  const _Body({required this.idea, required this.id});
  final Idea idea;
  final String id;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return ListView(
      padding: const EdgeInsets.all(QSpace.md),
      children: [
        Text(idea.body, style: QCueText.body.copyWith(color: context.q.text)),
        const SizedBox(height: QSpace.md),
        Divider(height: 1, color: context.q.border),
        const SizedBox(height: QSpace.md),
        _MetaRow(
          icon: Icons.schedule,
          text: idea.capturedAt.toLocal().toString(),
        ),
        _MetaRow(icon: Icons.place_outlined, text: _location(idea)),
        _MetaRow(
          icon: Icons.auto_awesome,
          text: idea.queued ? 'queued' : idea.ingestState.name,
        ),
        const SizedBox(height: QSpace.lg),
        Row(
          children: [
            OutlinedButton.icon(
              key: const ValueKey('capture-edit'),
              onPressed: () => _edit(context, ref),
              icon: const Icon(Icons.edit_outlined, size: 18),
              label: const Text('Edit'),
            ),
            const SizedBox(width: QSpace.sm),
            OutlinedButton.icon(
              key: const ValueKey('capture-delete'),
              onPressed: () => _confirmDelete(context, ref),
              icon: const Icon(Icons.delete_outline, size: 18),
              label: const Text('Delete'),
            ),
          ],
        ),
      ],
    );
  }

  /// "lat, lng (±Nm)" — or "No location" when this capture has no GPS fix.
  static String _location(Idea idea) {
    if (idea.lat == null || idea.lng == null) return 'No location';
    final acc = idea.locAccuracyM != null
        ? ' (±${idea.locAccuracyM!.round()}m)'
        : '';
    return '${idea.lat!.toStringAsFixed(5)}, '
        '${idea.lng!.toStringAsFixed(5)}$acc';
  }

  Future<void> _edit(BuildContext context, WidgetRef ref) async {
    final controller = TextEditingController(text: idea.body);
    final newBody = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Edit capture'),
        content: TextField(
          key: const ValueKey('capture-edit-field'),
          controller: controller,
          maxLines: null,
          autofocus: true,
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Cancel'),
          ),
          TextButton(
            key: const ValueKey('capture-edit-save'),
            onPressed: () => Navigator.pop(ctx, controller.text.trim()),
            child: const Text('Save'),
          ),
        ],
      ),
    );
    if (newBody != null && newBody.isNotEmpty && newBody != idea.body) {
      await ref
          .read(captureFeedProvider.notifier)
          .editCapture(id, body: newBody);
    }
  }

  Future<void> _confirmDelete(BuildContext context, WidgetRef ref) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete this capture?'),
        content: const Text('Its distilled note will be removed too.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Keep'),
          ),
          TextButton(
            key: const ValueKey('capture-delete-confirm'),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (ok == true) {
      await ref.read(captureFeedProvider.notifier).removeCapture(id);
      if (context.mounted) context.pop();
    }
  }
}

class _MetaRow extends StatelessWidget {
  const _MetaRow({required this.icon, required this.text});
  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(vertical: QSpace.xs),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.only(top: 2),
          child: Icon(icon, size: 16, color: context.q.text3),
        ),
        const SizedBox(width: QSpace.sm),
        Expanded(
          child: Text(
            text,
            style: QCueText.caption.copyWith(color: context.q.text2),
          ),
        ),
      ],
    ),
  );
}

class _DetailError extends StatelessWidget {
  const _DetailError(this.message);
  final String message;
  @override
  Widget build(BuildContext context) => Center(
    child: Padding(
      padding: const EdgeInsets.all(QSpace.xl),
      child: Text(
        "Couldn't load this capture · $message",
        textAlign: TextAlign.center,
        style: QCueText.body.copyWith(color: context.q.danger),
      ),
    ),
  );
}
