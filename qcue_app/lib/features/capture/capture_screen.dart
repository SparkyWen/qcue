// QCue S4-R30/R31/R32: the Capture feed — the default/home tab. A reverse-
// chronological outline of captures sits above an always-ready field pinned at
// the bottom (multiline text + push-to-talk mic). Submitting calls the single
// QcueApiClient seam (appends a `pending` row) and fires a light haptic. An
// offline banner shows while disconnected; pull-to-refresh; empty state.
// Content-first, generous whitespace, hairline dividers, virtualized feed.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../core/models/protocol_models.dart';
import '../../core/models/screen_state.dart';
import '../../core/net/api_client_provider.dart';
import '../../core/offline/connectivity.dart';
import '../../core/theme/qcue_space.dart';
import '../../core/theme/qcue_text.dart';
import '../../core/theme/qcue_theme.dart';
import '../../core/theme/theme_provider.dart';
import '../../widgets/empty_state.dart';
import '../../widgets/offline_banner.dart';
import 'capture_date_provider.dart';
import 'capture_provider.dart';
import 'widgets/capture_field.dart';
import 'widgets/feed_list.dart';
import 'widgets/voice_capture_controller.dart';

class CaptureScreen extends ConsumerWidget {
  const CaptureScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final offline = ref.watch(offlineProvider);
    final selectedDay = ref.watch(selectedCaptureDateProvider);

    Future<void> commit(String body, String origin) async {
      await ref
          .read(captureFeedProvider.notifier)
          .commit(body: body, origin: origin);
      ref.read(hapticsProvider).captureCommitted();
      // If a day view is active, refresh it so a capture made while browsing (a "now" capture belongs
      // to today's day query) surfaces immediately instead of looking dropped.
      final day = ref.read(selectedCaptureDateProvider);
      if (day != null) ref.invalidate(dayCapturesProvider(day));
    }

    // Render an AsyncValue<ScreenState<List<Idea>>> (shared by the live feed and the day view).
    Widget renderFeed(
      AsyncValue<ScreenState<List<Idea>>> async, {
      required Widget empty,
      required Future<void> Function() onRefresh,
    }) {
      return switch (async) {
        AsyncLoading() => const Center(child: CircularProgressIndicator()),
        AsyncError(:final error) => _ErrorCapture(error.toString()),
        AsyncData(:final value) => switch (value) {
            Empty() => empty,
            Loading() => const Center(child: CircularProgressIndicator()),
            ErrorState(:final message) => _ErrorCapture(message),
            Data(:final value) => RefreshIndicator(
                onRefresh: onRefresh,
                child: FeedList(
                  ideas: value,
                  // S4-R33: re-submit a failed capture's body as a fresh attempt.
                  onRetry: (idea) => commit(idea.body, idea.origin),
                  // CAP-R1: tap a row to open its detail (time/location + edit/delete).
                  onOpen: (idea) => context.go('/capture/${idea.id}'),
                ),
              ),
          },
        _ => const SizedBox.shrink(),
      };
    }

    final Widget feed = selectedDay == null
        ? renderFeed(
            ref.watch(captureFeedProvider),
            empty: const EmptyState(
              key: ValueKey('capture-empty'),
              icon: Icons.edit_outlined,
              title: 'Capture your first idea',
              hint: 'Type below, or hold the mic to speak.',
            ),
            onRefresh: () => ref.read(captureFeedProvider.notifier).refresh(),
          )
        : renderFeed(
            ref.watch(dayCapturesProvider(selectedDay)),
            empty: const EmptyState(
              key: ValueKey('capture-day-empty'),
              icon: Icons.event_busy_outlined,
              title: 'No captures on this day',
              hint: 'Pick another day, or clear to see your latest.',
            ),
            onRefresh: () async => ref.invalidate(dayCapturesProvider(selectedDay)),
          );

    return Column(
      children: [
        const _CaptureDateBar(),
        OfflineBanner(
          offline: offline,
          onReconnect: () {
            // Re-probe reachability (the bootstrap host flushes the outbound
            // queue on the online transition) and refresh the feed.
            ref.read(connectivityProvider.notifier).probe();
            ref.read(captureFeedProvider.notifier).refresh();
          },
        ),
        Expanded(child: feed),
        Divider(height: 1, color: context.q.border),
        CaptureField(
          onCommit: commit,
          voice: ref.watch(voiceCaptureProvider),
        ),
      ],
    );
  }
}

/// A slim toolbar above the feed: a calendar button to browse captures by any day, and — when a day is
/// chosen — a chip showing that day with a clear (X) back to the live newest-first feed.
class _CaptureDateBar extends ConsumerWidget {
  const _CaptureDateBar();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selected = ref.watch(selectedCaptureDateProvider);
    return Padding(
      padding: const EdgeInsets.fromLTRB(QSpace.md, QSpace.sm, QSpace.sm, 0),
      child: Row(
        children: [
          if (selected != null)
            Expanded(
              child: Align(
                alignment: Alignment.centerLeft,
                child: InputChip(
                  key: const ValueKey('capture-date-chip'),
                  label: Text(_dayLabel(selected),
                      style: QCueText.label.copyWith(color: context.q.text)),
                  onDeleted: () =>
                      ref.read(selectedCaptureDateProvider.notifier).clear(),
                  deleteIconColor: context.q.text2,
                ),
              ),
            )
          else
            const Spacer(),
          IconButton(
            key: const ValueKey('capture-calendar'),
            tooltip: 'Browse by day',
            icon: Icon(Icons.calendar_today_outlined,
                size: 20, color: context.q.text2),
            onPressed: () => _pickDay(context, ref, selected),
          ),
        ],
      ),
    );
  }

  Future<void> _pickDay(
      BuildContext context, WidgetRef ref, DateTime? current) async {
    final now = DateTime.now();
    final picked = await showDatePicker(
      context: context,
      initialDate: current ?? now,
      firstDate: DateTime(2020),
      lastDate: now,
    );
    if (picked != null) {
      ref.read(selectedCaptureDateProvider.notifier).select(picked);
    }
  }
}

/// A friendly label for the selected day: Today / Yesterday / YYYY-MM-DD (matches the feed grouping).
String _dayLabel(DateTime day) {
  final now = DateTime.now();
  final today = DateTime(now.year, now.month, now.day);
  final d = DateTime(day.year, day.month, day.day);
  final diff = today.difference(d).inDays;
  if (diff == 0) return 'Today';
  if (diff == 1) return 'Yesterday';
  String two(int n) => n.toString().padLeft(2, '0');
  return '${d.year}-${two(d.month)}-${two(d.day)}';
}

class _ErrorCapture extends StatelessWidget {
  const _ErrorCapture(this.message);
  final String message;
  @override
  Widget build(BuildContext context) => Center(
        child: Padding(
          padding: const EdgeInsets.all(QSpace.xl),
          child: Text("Couldn't load your feed · $message",
              textAlign: TextAlign.center,
              style: QCueText.body.copyWith(color: context.q.danger)),
        ),
      );
}
