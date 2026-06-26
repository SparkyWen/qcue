// QCue ISO-R1: decode the `sub` claim from a JWT for use as a LOCAL cache-owner tag. This does NOT
// verify the signature — it only labels which account a local cache belongs to so a switch can wipe it.
import 'dart:convert';

/// The `sub` (subject / user id) claim of [jwt], or null if it can't be read.
String? subjectOf(String jwt) {
  final parts = jwt.split('.');
  if (parts.length < 2) return null;
  try {
    final payload =
        utf8.decode(base64Url.decode(base64Url.normalize(parts[1])));
    final map = jsonDecode(payload) as Map<String, dynamic>;
    final sub = map['sub'];
    return (sub is String && sub.isNotEmpty) ? sub : null;
  } catch (_) {
    return null;
  }
}
