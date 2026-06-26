import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qcue_app/core/models/screen_state.dart';
import 'package:qcue_app/core/models/recall_conversation.dart';
import 'package:qcue_app/core/net/api_client_provider.dart';
import 'package:qcue_app/core/net/qcue_api_client.dart';
import 'package:qcue_app/features/recall/conversations_provider.dart';

void main() {
  test('conversationsProvider yields Data for a non-empty seed', () async {
    final container = ProviderContainer(
      overrides: [apiClientProvider.overrideWithValue(StubApiClient.seeded())],
    );
    addTearDown(container.dispose);
    final state = await container.read(conversationsProvider.future);
    expect(state, isA<Data<List<ConversationSummary>>>());
    expect((state as Data<List<ConversationSummary>>).value, isNotEmpty);
  });

  test('conversationsProvider yields Empty for the inert stub', () async {
    final container = ProviderContainer(
      overrides: [apiClientProvider.overrideWithValue(StubApiClient())],
    );
    addTearDown(container.dispose);
    final state = await container.read(conversationsProvider.future);
    expect(state, isA<Empty<List<ConversationSummary>>>());
  });
}
