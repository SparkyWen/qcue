// QCue S4-R56: the offline banner. Renders only while offline; reassures that
// captures are queued and will sync, with a manual retry. Hairline, flat, on
// the muted `surface2` token so it reads as a status strip, not an alert.
import 'package:flutter/material.dart';
import '../core/theme/qcue_space.dart';
import '../core/theme/qcue_text.dart';
import '../core/theme/qcue_theme.dart';

class OfflineBanner extends StatelessWidget {
  const OfflineBanner({
    super.key,
    required this.offline,
    this.onReconnect,
  });

  final bool offline;
  final VoidCallback? onReconnect;

  @override
  Widget build(BuildContext context) {
    if (!offline) return const SizedBox.shrink();
    return Semantics(
      liveRegion: true,
      label: 'Offline. Captures are queued and will sync when reconnected.',
      child: Container(
        width: double.infinity,
        color: context.q.surface2,
        padding: const EdgeInsets.symmetric(
            horizontal: QSpace.md, vertical: QSpace.sm),
        child: Row(
          children: [
            Icon(Icons.cloud_off_outlined, size: 16, color: context.q.text2),
            const SizedBox(width: QSpace.sm),
            Expanded(
              child: Text(
                'Offline — captures are queued and will sync',
                style: QCueText.caption.copyWith(color: context.q.text2),
              ),
            ),
            if (onReconnect != null)
              TextButton(
                onPressed: onReconnect,
                child: Text(
                  'Retry',
                  style: QCueText.caption.copyWith(color: context.q.linkText),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
