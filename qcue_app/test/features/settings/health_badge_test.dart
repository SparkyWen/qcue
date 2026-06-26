// QCue S4-R47: per-provider health from the 3-state credential machine. DEAD is
// distinct from EXHAUSTED (RKM §9 #3); never color-only (icon + label).
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/settings/widgets/health_badge.dart';

void main() {
  testWidgets('S4-R47: dead is distinct from exhausted; never color-only',
      (tester) async {
    final labels = <CredStatus, String>{};
    for (final s in CredStatus.values) {
      await tester.pumpWidget(MaterialApp(
        theme: QCueTheme.build(QThemeId.cleanLight),
        home: Scaffold(body: HealthBadge(status: s)),
      ));
      final sem = tester.widget<Semantics>(find
          .descendant(
              of: find.byType(HealthBadge), matching: find.byType(Semantics))
          .first);
      labels[s] = sem.properties.label!;
      // an icon glyph accompanies the color (never color-only)
      expect(
          find.descendant(
              of: find.byType(HealthBadge), matching: find.byType(Icon)),
          findsOneWidget);
    }
    expect(labels[CredStatus.ok], contains('ok'));
    expect(labels[CredStatus.exhausted], contains('cooling'));
    expect(labels[CredStatus.dead], contains('invalid'));
    expect(labels[CredStatus.dead], isNot(equals(labels[CredStatus.exhausted])));
  });

  // Defense-in-depth for the stuck "cooling down" bug: even before the next refetch, an
  // exhausted credential whose cooldown has already elapsed must render as healthy. A still-
  // future cooldown keeps showing "cooling down". A null cooldown trusts the server status.
  testWidgets('elapsed cooldown renders healthy; future cooldown still cools',
      (tester) async {
    String labelFor(DateTime? cooldownUntil) {
      return credStatusGlyph(
        effectiveCredStatus(CredStatus.exhausted, cooldownUntil),
      ).label;
    }

    final past = DateTime.now().subtract(const Duration(minutes: 2));
    final future = DateTime.now().add(const Duration(minutes: 2));
    expect(labelFor(past), 'ok', reason: 'elapsed cooldown heals to ok');
    expect(labelFor(future), 'cooling down', reason: 'future cooldown still cools');
    expect(labelFor(null), 'cooling down', reason: 'null cooldown trusts server status');

    // and the widget honours it end-to-end
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(
        body: HealthBadge(status: CredStatus.exhausted, cooldownUntil: past),
      ),
    ));
    final sem = tester.widget<Semantics>(find
        .descendant(
            of: find.byType(HealthBadge), matching: find.byType(Semantics))
        .first);
    expect(sem.properties.label, contains('ok'));
  });
}
