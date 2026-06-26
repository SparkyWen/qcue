// QCue S4-R52/R64: the signed-in session. `null` ⇒ onboarding. Single source
// of truth. A keyless session is valid (StubProvider, master §4).
import 'package:flutter_riverpod/flutter_riverpod.dart';

class Session {
  const Session({required this.jwt, required this.email, required this.hasKey});
  final String jwt;
  final String email;
  final bool hasKey;
}

class SessionNotifier extends Notifier<Session?> {
  @override
  Session? build() => null;
  void signIn(Session s) => state = s;
  void signOut() => state = null;
  void markKeyAdded() {
    final s = state;
    if (s != null) {
      state = Session(jwt: s.jwt, email: s.email, hasKey: true);
    }
  }
}

final sessionProvider =
    NotifierProvider<SessionNotifier, Session?>(SessionNotifier.new);
