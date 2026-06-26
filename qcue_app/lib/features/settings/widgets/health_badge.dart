// QCue S4-R47: per-provider health from the 3-state credential machine. DEAD
// (key invalid) is DISTINCT from EXHAUSTED (cooling down) — RKM §9 #3. Never
// color-only: an icon glyph + a text label accompany the token color.
import 'package:flutter/material.dart';
import '../../../core/models/protocol_models.dart';
import '../../../core/theme/qcue_space.dart';
import '../../../core/theme/qcue_text.dart';
import '../../../core/theme/qcue_theme.dart';
import '../../../core/theme/qcue_tokens.dart';

/// Defense-in-depth for the "stuck cooling down" bug: a cooldown is hard-capped at 5 min
/// server-side, and the server now clears elapsed cooldowns on read — but if the cooldown
/// expires while the Settings screen is already open (before the next refetch), the cached
/// status is still `exhausted`. Treat an exhausted credential whose `cooldownUntil` has
/// already passed as healthy. A null cooldown trusts the server's status as-is (matches the
/// server, which only heals non-null elapsed cooldowns).
CredStatus effectiveCredStatus(CredStatus status, DateTime? cooldownUntil) {
  if (status == CredStatus.exhausted &&
      cooldownUntil != null &&
      cooldownUntil.isBefore(DateTime.now())) {
    return CredStatus.ok;
  }
  return status;
}

({IconData icon, String label, QToken token}) credStatusGlyph(
        CredStatus status) =>
    switch (status) {
      CredStatus.ok =>
        (icon: Icons.check_circle_outline, label: 'ok', token: QToken.success),
      CredStatus.exhausted => (
          icon: Icons.hourglass_bottom,
          label: 'cooling down',
          token: QToken.pending
        ),
      CredStatus.dead =>
        (icon: Icons.block, label: 'key invalid', token: QToken.danger),
    };

class HealthBadge extends StatelessWidget {
  const HealthBadge({super.key, required this.status, this.cooldownUntil});
  final CredStatus status;

  /// When the status is `exhausted`, this lets the badge render as healthy once the
  /// cooldown window has elapsed (see [effectiveCredStatus]).
  final DateTime? cooldownUntil;

  @override
  Widget build(BuildContext context) {
    final m = credStatusGlyph(effectiveCredStatus(status, cooldownUntil));
    return Semantics(
      label: 'health ${m.label}',
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(m.icon, size: 14, color: context.q.color(m.token)),
          const SizedBox(width: QSpace.xs),
          Text(m.label,
              style:
                  QCueText.caption.copyWith(color: context.q.color(m.token))),
        ],
      ),
    );
  }
}
