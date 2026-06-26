// QCue S4-R5: the replay-on-reconnect wire envelope + the Item taxonomy,
// mirroring the Rust `app-server-protocol`/`protocol` crates EXACTLY.
//
//   RuntimeEventEnvelope { schema_version: u32, thread_id: Uuid,
//     turn_id: Option<Uuid>, seq: u64, event: String, payload: Value }
//
// The envelope is forward-compatible: `event` is a free String (unknown/future
// kinds round-trip, never throw) and missing fields take serde defaults. The
// `Item` enum is serde `tag:"type"`, `rename_all:"camelCase"` — its variant tags
// are camelCase on the wire ("ideaCaptured", "voiceTranscript", ...).
import 'sse_event.dart' show Citation;

/// The canonical known `event` kinds (the camelCase tokens carried as
/// `event: String`). Unknown wire strings are NOT in this enum — they survive
/// as the raw String on [RuntimeEventEnvelope.event] (forward-compat).
enum RuntimeEventKind {
  threadStarted,
  turnStarted,
  itemStarted,
  itemDelta,
  itemCompleted,
  turnCompleted,
  usage,
  error,
}

const _runtimeEventWire = {
  RuntimeEventKind.threadStarted: 'threadStarted',
  RuntimeEventKind.turnStarted: 'turnStarted',
  RuntimeEventKind.itemStarted: 'itemStarted',
  RuntimeEventKind.itemDelta: 'itemDelta',
  RuntimeEventKind.itemCompleted: 'itemCompleted',
  RuntimeEventKind.turnCompleted: 'turnCompleted',
  RuntimeEventKind.usage: 'usage',
  RuntimeEventKind.error: 'error',
};

/// The known kind for [wire], or null if it is an unknown/future event.
RuntimeEventKind? runtimeEventKindFromWire(String wire) {
  for (final e in _runtimeEventWire.entries) {
    if (e.value == wire) return e.key;
  }
  return null;
}

String runtimeEventKindToWire(RuntimeEventKind k) => _runtimeEventWire[k]!;

/// Forward-compatible event envelope crossing FFI/SSE/WSS (master §8).
class RuntimeEventEnvelope {
  const RuntimeEventEnvelope({
    required this.schemaVersion,
    required this.event,
    this.threadId = '',
    this.turnId,
    this.seq = 0,
    this.payload = const {},
  });

  final int schemaVersion;
  final String threadId; // Uuid string; serde-default is the nil UUID
  final String? turnId; // Option<Uuid>
  final int seq;

  /// Event kind string; unknown kinds are preserved verbatim (forward-compat).
  final String event;

  /// Opaque JSON payload (serde `Value`); an omitted payload decodes to `{}`.
  final Map<String, dynamic> payload;

  /// The known kind, or null when [event] is an unknown/future string.
  RuntimeEventKind? get knownKind => runtimeEventKindFromWire(event);

  factory RuntimeEventEnvelope.fromJson(Map<String, dynamic> j) =>
      RuntimeEventEnvelope(
        schemaVersion: (j['schema_version'] as num?)?.toInt() ?? 0,
        threadId: j['thread_id'] as String? ?? '',
        turnId: j['turn_id'] as String?,
        seq: (j['seq'] as num?)?.toInt() ?? 0,
        event: j['event'] as String,
        payload:
            (j['payload'] as Map?)?.cast<String, dynamic>() ?? const {},
      );

  Map<String, dynamic> toJson() => {
        'schema_version': schemaVersion,
        'thread_id': threadId,
        'turn_id': turnId,
        'seq': seq,
        'event': event,
        'payload': payload,
      };

  @override
  bool operator ==(Object other) =>
      other is RuntimeEventEnvelope &&
      other.schemaVersion == schemaVersion &&
      other.threadId == threadId &&
      other.turnId == turnId &&
      other.seq == seq &&
      other.event == event;

  @override
  int get hashCode => Object.hash(schemaVersion, threadId, turnId, seq, event);
}

// ── WikiEditOp (protocol::WikiEditOp, serde tag="type") ──
sealed class WikiEditOp {
  const WikiEditOp();

  factory WikiEditOp.fromJson(Map<String, dynamic> j) {
    return switch (j['type'] as String?) {
      'Create' => const WikiEditCreate(),
      'Update' => const WikiEditUpdate(),
      'Merge' => WikiEditMerge(j['into_slug'] as String),
      'Delete' => const WikiEditDelete(),
      final t => throw ArgumentError('unknown WikiEditOp type: $t'),
    };
  }

  Map<String, dynamic> toJson();
}

class WikiEditCreate extends WikiEditOp {
  const WikiEditCreate();
  @override
  Map<String, dynamic> toJson() => {'type': 'Create'};
  @override
  bool operator ==(Object other) => other is WikiEditCreate;
  @override
  int get hashCode => 'Create'.hashCode;
}

class WikiEditUpdate extends WikiEditOp {
  const WikiEditUpdate();
  @override
  Map<String, dynamic> toJson() => {'type': 'Update'};
  @override
  bool operator ==(Object other) => other is WikiEditUpdate;
  @override
  int get hashCode => 'Update'.hashCode;
}

class WikiEditMerge extends WikiEditOp {
  const WikiEditMerge(this.intoSlug);
  final String intoSlug;
  @override
  Map<String, dynamic> toJson() => {'type': 'Merge', 'into_slug': intoSlug};
  @override
  bool operator ==(Object other) => other is WikiEditMerge && other.intoSlug == intoSlug;
  @override
  int get hashCode => Object.hash('Merge', intoSlug);
}

class WikiEditDelete extends WikiEditOp {
  const WikiEditDelete();
  @override
  Map<String, dynamic> toJson() => {'type': 'Delete'};
  @override
  bool operator ==(Object other) => other is WikiEditDelete;
  @override
  int get hashCode => 'Delete'.hashCode;
}

// ── DreamPhase (protocol::DreamPhase) ──
enum DreamPhase { orient, gather, consolidate, prune }
const _dreamPhaseWire = {
  DreamPhase.orient: 'Orient',
  DreamPhase.gather: 'Gather',
  DreamPhase.consolidate: 'Consolidate',
  DreamPhase.prune: 'Prune',
};
DreamPhase dreamPhaseFromJson(String s) {
  for (final e in _dreamPhaseWire.entries) {
    if (e.value == s) return e.key;
  }
  throw ArgumentError('unknown DreamPhase: $s');
}

String dreamPhaseToJson(DreamPhase v) => _dreamPhaseWire[v]!;

// ── Item taxonomy (app-server-protocol::Item, serde tag="type" camelCase) ──
sealed class Item {
  const Item();

  factory Item.fromJson(Map<String, dynamic> j) {
    return switch (j['type'] as String?) {
      'ideaCaptured' => ItemIdeaCaptured(
          ideaId: j['idea_id'] as String,
          body: j['body'] as String,
        ),
      'voiceTranscript' => ItemVoiceTranscript(
          ideaId: j['idea_id'] as String,
          text: j['text'] as String,
          provider: j['provider'] as String,
        ),
      'wikiEdit' => ItemWikiEdit(
          pageId: j['page_id'] as String,
          slug: j['slug'] as String,
          op: WikiEditOp.fromJson((j['op'] as Map).cast<String, dynamic>()),
        ),
      'recallResult' => ItemRecallResult(
          answerDelta: j['answer_delta'] as String,
          citations: (j['citations'] as List? ?? const [])
              .map((e) => Citation.fromJson((e as Map).cast<String, dynamic>()))
              .toList(),
        ),
      'agentMessage' => ItemAgentMessage(delta: j['delta'] as String),
      'dreamTurn' => ItemDreamTurn(
          phase: dreamPhaseFromJson(j['phase'] as String),
          pagesTouched: (j['pages_touched'] as List? ?? const [])
              .cast<String>(),
        ),
      'reasoning' => ItemReasoning(delta: j['delta'] as String),
      'error' => ItemError(
          code: (j['code'] as num).toInt(),
          message: j['message'] as String,
        ),
      final t => throw ArgumentError('unknown Item type: $t'),
    };
  }

  Map<String, dynamic> toJson();
}

class ItemIdeaCaptured extends Item {
  const ItemIdeaCaptured({required this.ideaId, required this.body});
  final String ideaId;
  final String body;
  @override
  Map<String, dynamic> toJson() =>
      {'type': 'ideaCaptured', 'idea_id': ideaId, 'body': body};
  @override
  bool operator ==(Object other) =>
      other is ItemIdeaCaptured && other.ideaId == ideaId && other.body == body;
  @override
  int get hashCode => Object.hash('ideaCaptured', ideaId, body);
}

class ItemVoiceTranscript extends Item {
  const ItemVoiceTranscript({
    required this.ideaId,
    required this.text,
    required this.provider,
  });
  final String ideaId;
  final String text;
  final String provider;
  @override
  Map<String, dynamic> toJson() => {
        'type': 'voiceTranscript',
        'idea_id': ideaId,
        'text': text,
        'provider': provider,
      };
  @override
  bool operator ==(Object other) =>
      other is ItemVoiceTranscript &&
      other.ideaId == ideaId &&
      other.text == text &&
      other.provider == provider;
  @override
  int get hashCode => Object.hash('voiceTranscript', ideaId, text, provider);
}

class ItemWikiEdit extends Item {
  const ItemWikiEdit({
    required this.pageId,
    required this.slug,
    required this.op,
  });
  final String pageId;
  final String slug;
  final WikiEditOp op;
  @override
  Map<String, dynamic> toJson() => {
        'type': 'wikiEdit',
        'page_id': pageId,
        'slug': slug,
        'op': op.toJson(),
      };
  @override
  bool operator ==(Object other) =>
      other is ItemWikiEdit &&
      other.pageId == pageId &&
      other.slug == slug &&
      other.op == op;
  @override
  int get hashCode => Object.hash('wikiEdit', pageId, slug, op);
}

class ItemRecallResult extends Item {
  const ItemRecallResult({
    required this.answerDelta,
    required this.citations,
  });
  final String answerDelta;
  final List<Citation> citations;
  @override
  Map<String, dynamic> toJson() => {
        'type': 'recallResult',
        'answer_delta': answerDelta,
        'citations': citations.map((c) => c.toJson()).toList(),
      };
  @override
  bool operator ==(Object other) =>
      other is ItemRecallResult &&
      other.answerDelta == answerDelta &&
      _listEq(other.citations, citations);
  @override
  int get hashCode => Object.hash('recallResult', answerDelta,
      Object.hashAll(citations));
}

class ItemAgentMessage extends Item {
  const ItemAgentMessage({required this.delta});
  final String delta;
  @override
  Map<String, dynamic> toJson() => {'type': 'agentMessage', 'delta': delta};
  @override
  bool operator ==(Object other) => other is ItemAgentMessage && other.delta == delta;
  @override
  int get hashCode => Object.hash('agentMessage', delta);
}

class ItemDreamTurn extends Item {
  const ItemDreamTurn({required this.phase, required this.pagesTouched});
  final DreamPhase phase;
  final List<String> pagesTouched;
  @override
  Map<String, dynamic> toJson() => {
        'type': 'dreamTurn',
        'phase': dreamPhaseToJson(phase),
        'pages_touched': pagesTouched,
      };
  @override
  bool operator ==(Object other) =>
      other is ItemDreamTurn &&
      other.phase == phase &&
      _listEq(other.pagesTouched, pagesTouched);
  @override
  int get hashCode =>
      Object.hash('dreamTurn', phase, Object.hashAll(pagesTouched));
}

class ItemReasoning extends Item {
  const ItemReasoning({required this.delta});
  final String delta;
  @override
  Map<String, dynamic> toJson() => {'type': 'reasoning', 'delta': delta};
  @override
  bool operator ==(Object other) => other is ItemReasoning && other.delta == delta;
  @override
  int get hashCode => Object.hash('reasoning', delta);
}

class ItemError extends Item {
  const ItemError({required this.code, required this.message});
  final int code;
  final String message;
  @override
  Map<String, dynamic> toJson() =>
      {'type': 'error', 'code': code, 'message': message};
  @override
  bool operator ==(Object other) =>
      other is ItemError && other.code == code && other.message == message;
  @override
  int get hashCode => Object.hash('error', code, message);
}

bool _listEq<T>(List<T> a, List<T> b) {
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}
