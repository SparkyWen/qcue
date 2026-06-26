// QCue S4-R6/R7/R29: go_router with a StatefulShellRoute per tab (state-
// preserving), deep links, and a typed not-found. Branch roots are the real
// (placeholder) feature screens; deep sub-routes render a tested [RouteStub]
// that shows its location so router/back-stack tests assert without screen code.
// Later milestones swap each sub-route for the real detail screen.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import '../../features/activity/activity_screen.dart';
import '../../features/auth/auth_provider.dart';
import '../../features/auth/login_screen.dart';
import '../../features/auth/signup_screen.dart';
import '../../features/onboarding/onboarding_screen.dart';
import '../../features/onboarding/onboarding_store.dart';
import '../../features/capture/capture_detail_screen.dart';
import '../../features/capture/capture_screen.dart';
import '../../features/capture/quick_capture_screen.dart';
import '../../features/recall/recall_screen.dart';
import '../../features/settings/settings_screen.dart';
import '../update/update_required_screen.dart';
import '../update/update_service.dart';
import '../../features/wiki/connections_view.dart';
import '../../features/wiki/wiki_page_screen.dart';
import '../../features/wiki/wiki_screen.dart';
import '../../widgets/app_scaffold.dart';
import '../theme/qcue_text.dart';
import '../theme/qcue_theme.dart';

/// Placeholder body for a deep sub-route. Shows the location so router tests can
/// assert routing/back-stack without screen code. A real, tested widget.
class RouteStub extends StatelessWidget {
  const RouteStub(this.location, {super.key});
  final String location;
  @override
  Widget build(BuildContext context) => Center(
        child: Text(
          location,
          style: QCueText.body.copyWith(color: context.q.text),
        ),
      );
}

class NotFoundScreen extends StatelessWidget {
  const NotFoundScreen({super.key});
  @override
  Widget build(BuildContext context) => Scaffold(
        backgroundColor: context.q.bg,
        body: Center(
          key: const ValueKey('not-found'),
          child: Text(
            "This page hasn't been built yet",
            style: QCueText.body.copyWith(color: context.q.text2),
          ),
        ),
      );
}

// v0.2.2: Activity is no longer a top-level tab — it lives under Settings
// (`/settings/activity`). The bottom bar is the 4 roots below.
const tabRoots = ['/capture', '/wiki', '/recall', '/settings'];
const _tabTitles = ['Capture', 'Wiki', 'Recall', 'Settings'];

/// The single app router instance, held for the app's lifetime. Exposed as a
/// provider so the bootstrap can drive deep-link navigation from native channels
/// (S5-R34/R45 — notification taps + widget compose deep-link via go_router).
///
/// Wired to [authStateProvider] (Task 5): when no valid session token is held
/// the router redirects to `/login`; once signed in it returns to the intended
/// route. `QCUE_STUB` demo mode seeds `authed`, so the demo bypasses login.
final routerProvider = Provider<GoRouter>((ref) {
  // A Listenable the router refreshes on, so a sign-in/out re-runs `redirect`.
  final refresh = ValueNotifier<int>(0);
  ref.onDispose(refresh.dispose);
  ref.listen<AuthStatus>(authStateProvider, (_, __) => refresh.value++);
  // AU-R19: re-run the redirect when the force-update gate flips (e.g. an update check resolves).
  ref.listen<bool>(updateGateProvider, (_, __) => refresh.value++);
  return buildQcueRouter(
    refreshListenable: refresh,
    isAuthed: () => ref.read(authStateProvider) == AuthStatus.authed,
    // S4-R52: first-run gate — an unauthed, not-yet-onboarded user → /onboarding.
    hasOnboarded: () => ref.read(onboardingStoreProvider).hasSeen,
    isUpdateRequired: () => ref.read(updateGateProvider),
  );
});

/// go_router with a StatefulShellRoute per tab (state-preserving, S4-R6),
/// deep links (S4-R7), and a typed not-found (S4-R7).
///
/// When [isAuthed] is supplied, an unauthenticated session is redirected to
/// `/login` (Task 5). Tests that omit it get the original always-authed router.
GoRouter buildQcueRouter({
  String initialLocation = '/capture',
  Listenable? refreshListenable,
  bool Function()? isAuthed,
  bool Function()? hasOnboarded,
  bool Function()? isUpdateRequired, // AU-R19
}) {
  return GoRouter(
    initialLocation: initialLocation,
    errorBuilder: (_, __) => const NotFoundScreen(),
    refreshListenable: refreshListenable,
    redirect: (isAuthed == null && isUpdateRequired == null)
        ? null
        : (context, state) {
            final loc = state.matchedLocation;
            // AU-R19 — the force gate preempts everything (even auth): a build below
            // min_supported_build is blocked behind /update-required until it clears.
            if (isUpdateRequired?.call() ?? false) {
              return loc == '/update-required' ? null : '/update-required';
            }
            if (loc == '/update-required') return initialLocation; // gate cleared ⇒ leave the screen
            if (isAuthed == null) return null;
            final authed = isAuthed();
            // Authed: nothing gates; bounce off the auth/onboarding screens.
            if (authed) {
              return (loc == '/login' ||
                      loc == '/signup' ||
                      loc == '/onboarding')
                  ? initialLocation
                  : null;
            }
            // Unauthed + first run (not yet onboarded) → /onboarding (S4-R52).
            final onboarded = hasOnboarded?.call() ?? true;
            if (!onboarded) return loc == '/onboarding' ? null : '/onboarding';
            // Onboarded but unauthed → /login (/login + /signup allowed, WS-A2).
            return (loc == '/login' || loc == '/signup') ? null : '/login';
          },
    routes: [
      GoRoute(
        path: '/update-required',
        builder: (_, __) => const UpdateRequiredScreen(),
      ),
      GoRoute(
        path: '/login',
        builder: (_, __) => const LoginScreen(),
      ),
      GoRoute(
        path: '/signup',
        builder: (_, __) => const SignupScreen(),
      ),
      GoRoute(
        // S4-R52: first-run onboarding, skippable to a usable keyless /capture.
        path: '/onboarding',
        builder: (context, _) =>
            OnboardingScreen(onDone: () => context.go('/capture')),
      ),
      StatefulShellRoute.indexedStack(
        builder: (context, state, navShell) => AppScaffold(
          title: _tabTitles[navShell.currentIndex],
          currentIndex: navShell.currentIndex,
          onTab: (i) => navShell.goBranch(
            i,
            initialLocation: i == navShell.currentIndex,
          ),
          onCompose: () => context.go('/capture/compose'),
          body: navShell,
        ),
        branches: [
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: '/capture',
                builder: (_, __) => const CaptureScreen(),
                routes: [
                  GoRoute(
                    // S4-R51: the in-app quick-capture compose screen (also the
                    // qcue://capture/compose widget/notification deep-link target).
                    path: 'compose',
                    builder: (_, __) => const QuickCaptureScreen(),
                  ),
                  GoRoute(
                    // CAP-R1: capture detail — tap a feed row to inspect its
                    // time/location + edit/delete. Listed AFTER `compose` so the
                    // static segment wins the match over this `:id` wildcard.
                    path: ':id',
                    builder: (_, s) =>
                        CaptureDetailScreen(id: s.pathParameters['id']!),
                  ),
                ],
              ),
            ],
          ),
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: '/wiki',
                builder: (context, __) => WikiScreen(
                  onOpenPage: (slug) => context.go('/wiki/page/$slug'),
                ),
                routes: [
                  GoRoute(
                    path: 'page/:slug',
                    builder: (context, s) {
                      final slug = s.pathParameters['slug']!;
                      return WikiPageScreen(
                        slug: slug,
                        onOpenPage: (next) => context.go('/wiki/page/$next'),
                        onOpenConnections: () =>
                            context.go('/wiki/page/$slug/connections'),
                      );
                    },
                    routes: [
                      GoRoute(
                        // S4-R36: the 1-hop connections view (was a RouteStub).
                        path: 'connections',
                        builder: (context, s) => ConnectionsView(
                          slug: s.pathParameters['slug']!,
                          onOpenPage: (next) => context.go('/wiki/page/$next'),
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ],
          ),
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: '/recall',
                builder: (context, __) => RecallScreen(
                  onOpenPage: (slug) => context.go('/wiki/page/$slug'),
                  onOpenCitation: (ref) => context.go('/recall/citation/$ref'),
                ),
                routes: [
                  GoRoute(
                    path: ':threadId',
                    builder: (_, s) =>
                        RouteStub('recall/${s.pathParameters['threadId']}'),
                  ),
                  GoRoute(
                    path: 'citation/:ref',
                    builder: (_, s) =>
                        RouteStub('recall/citation/${s.pathParameters['ref']}'),
                  ),
                ],
              ),
            ],
          ),
          StatefulShellBranch(
            routes: [
              GoRoute(
                path: '/settings',
                builder: (_, __) => const SettingsScreen(),
                routes: [
                  // v0.2.2: Activity moved here from a top-level tab. It renders
                  // the unchanged ActivityScreen inside the Settings branch, so
                  // the bottom bar stays and Back returns to Settings. The deep
                  // sub-routes (running Dream detail, candidate diff) are nested
                  // under it with the /settings/activity prefix. Listed BEFORE
                  // the `:section` stub so the static segment wins the match.
                  GoRoute(
                    path: 'activity',
                    builder: (context, __) => ActivityScreen(
                      onOpenDream: (jobId) =>
                          context.go('/settings/activity/dream/$jobId'),
                      onOpenCandidates: () =>
                          context.go('/settings/activity/candidate/all'),
                    ),
                    routes: [
                      GoRoute(
                        path: 'dream/:jobId',
                        builder: (_, s) =>
                            DreamDetailRoute(jobId: s.pathParameters['jobId']!),
                      ),
                      GoRoute(
                        path: 'candidate/:id',
                        builder: (_, s) => RouteStub(
                          'settings/activity/candidate/${s.pathParameters['id']}',
                        ),
                      ),
                    ],
                  ),
                  GoRoute(
                    path: ':section',
                    builder: (_, s) =>
                        RouteStub('settings/${s.pathParameters['section']}'),
                  ),
                ],
              ),
            ],
          ),
        ],
      ),
    ],
  );
}
