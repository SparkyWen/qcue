// QCue S4-R46: the BYOK key-entry sheet. The field obscures input; after entry,
// ONLY the masked last-4 hint is ever surfaced (never the secret); the plaintext
// never lands in Dart prefs/cache. The security boundary: the vault UI displays
// key_hint and nothing else.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/protocol_models.dart';
import 'package:qcue_app/core/secure/secure_storage.dart';
import 'package:qcue_app/core/theme/qcue_theme.dart';
import 'package:qcue_app/core/theme/qcue_tokens.dart';
import 'package:qcue_app/features/settings/widgets/add_key_sheet.dart';

class SpyVault implements SecureStorage {
  final writes = <String, String>{};
  @override
  Future<void> write(String k, String v) async => writes[k] = v;
  @override
  Future<String?> read(String k) async => writes[k];
  @override
  Future<void> delete(String k) async => writes.remove(k);
}

void main() {
  testWidgets(
      'S4-R46: the field obscures input; only the hint is shown; no plaintext persists',
      (tester) async {
    final vault = SpyVault();
    ProviderCredential? added;
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(
        body: AddKeySheet(
          provider: 'openai',
          vault: vault,
          // The screen wires onSubmit to QcueApiClient.putKey, which returns a
          // masked cred. The sheet here simply derives the same last-4 hint.
          onAdded: (cred) => added = cred,
        ),
      ),
    ));
    final field =
        tester.widget<TextField>(find.byKey(const ValueKey('key-field')));
    expect(field.obscureText, isTrue); // obscured entry
    await tester.enterText(
        find.byKey(const ValueKey('key-field')), 'sk-secret-7777');
    await tester.tap(find.text('Add'));
    await tester.pump();
    // the credential exposes only the last-4 hint, never the key
    expect(added, isNotNull);
    expect(added!.keyHint, contains('7777'));
    expect(added!.keyHint, isNot(contains('secret')));
    expect(added!.toJson().toString(), isNot(contains('sk-secret')));
    // the secret never lands in Dart prefs/cache in plaintext
    expect(vault.writes.values.any((v) => v == 'sk-secret-7777'), isFalse);
  });

  testWidgets('S4-R46: an empty key does not produce a credential',
      (tester) async {
    final vault = SpyVault();
    ProviderCredential? added;
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(
        body: AddKeySheet(
            provider: 'openai', vault: vault, onAdded: (c) => added = c),
      ),
    ));
    await tester.tap(find.text('Add'));
    await tester.pump();
    expect(added, isNull);
  });

  testWidgets('S4-R46: the visibility toggle can reveal the field while typing',
      (tester) async {
    final vault = SpyVault();
    await tester.pumpWidget(MaterialApp(
      theme: QCueTheme.build(QThemeId.cleanLight),
      home: Scaffold(
        body: AddKeySheet(provider: 'openai', vault: vault, onAdded: (_) {}),
      ),
    ));
    expect(
        tester
            .widget<TextField>(find.byKey(const ValueKey('key-field')))
            .obscureText,
        isTrue);
    await tester.tap(find.byIcon(Icons.visibility));
    await tester.pump();
    expect(
        tester
            .widget<TextField>(find.byKey(const ValueKey('key-field')))
            .obscureText,
        isFalse);
  });
}
