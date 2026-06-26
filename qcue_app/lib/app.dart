// QCue S4: the root app — wires the active theme (from themeProvider) and the
// router. The router instance is built once and held for the app's lifetime.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'core/router/qcue_router.dart';
import 'core/theme/qcue_theme.dart';
import 'core/theme/theme_provider.dart';

class QCueApp extends ConsumerWidget {
  const QCueApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final themeId = ref.watch(themeProvider);
    // The single router instance (a provider) so native deep-links (notification
    // taps, widget compose) can navigate through the same GoRouter (S5-R34/R45).
    final router = ref.watch(routerProvider);
    return MaterialApp.router(
      title: 'QCue',
      theme: QCueTheme.build(themeId),
      routerConfig: router,
      debugShowCheckedModeBanner: false,
    );
  }
}
