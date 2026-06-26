import 'dart:convert';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/net/jwt_claims.dart';

String _jwt(Map<String, dynamic> payload) {
  final p = base64Url.encode(utf8.encode(jsonEncode(payload)));
  return 'header.$p.sig';
}

void main() {
  test('subjectOf reads the sub claim', () {
    expect(subjectOf(_jwt({'sub': 'user-123', 'tid': 't'})), 'user-123');
  });
  test('subjectOf returns null on garbage / missing sub', () {
    expect(subjectOf('not-a-jwt'), isNull);
    expect(subjectOf(_jwt({'tid': 't'})), isNull);
    expect(subjectOf(''), isNull);
  });
}
