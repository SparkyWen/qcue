// QCue S4-R55: skeleton placeholders for content waits that cross 300ms (a
// sub-300ms wait shows nothing, to avoid a flash). A gentle opacity pulse
// (transform/opacity only, master §10 motion); the reduced-motion gate (QMotion)
// collapses it to a STATIC block — so content is never gated by animation, and
// so widget tests never hang on a perpetual animation. Buttons keep their
// spinner; this is for content regions.
import 'dart:async';

import 'package:flutter/material.dart';
import '../core/theme/qcue_motion.dart';
import '../core/theme/qcue_space.dart';
import '../core/theme/qcue_theme.dart';

/// A single placeholder block. Under reduced motion it renders as a static
/// surface2 block (no AnimationController, no perpetual ticker to settle on).
class Skeleton extends StatefulWidget {
  const Skeleton({
    super.key,
    required this.width,
    required this.height,
    this.radius = QRadius.input,
  });

  final double width;
  final double height;
  final double radius;

  @override
  State<Skeleton> createState() => _SkeletonState();
}

class _SkeletonState extends State<Skeleton>
    with SingleTickerProviderStateMixin {
  AnimationController? _ctrl;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // Only run the pulse when motion is allowed. Under reduced motion we never
    // start a controller, so there is no infinite animation to settle on.
    if (QMotion.reduced(context)) {
      _ctrl?.dispose();
      _ctrl = null;
    } else {
      _ctrl ??= AnimationController(
        vsync: this,
        duration: const Duration(milliseconds: 1100),
      )..repeat(reverse: true);
    }
  }

  @override
  void dispose() {
    _ctrl?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final block = DecoratedBox(
      decoration: BoxDecoration(
        color: context.q.surface2,
        borderRadius: BorderRadius.circular(widget.radius),
      ),
      child: SizedBox(width: widget.width, height: widget.height),
    );
    final ctrl = _ctrl;
    if (ctrl == null) return block; // reduced-motion: static block
    return FadeTransition(
      opacity: Tween<double>(begin: 0.45, end: 1.0).animate(
        CurvedAnimation(parent: ctrl, curve: Curves.easeInOut),
      ),
      child: block,
    );
  }
}

/// Renders [child] only after [delay] (default 300ms) so sub-300ms waits show
/// nothing — no flash (S4-R55). Wrap a [SkeletonList] in this for a loading
/// branch. The delay is a single one-shot timer (test-safe).
class DelayedSkeleton extends StatefulWidget {
  const DelayedSkeleton({
    super.key,
    required this.child,
    this.delay = const Duration(milliseconds: 300),
  });

  final Widget child;
  final Duration delay;

  @override
  State<DelayedSkeleton> createState() => _DelayedSkeletonState();
}

class _DelayedSkeletonState extends State<DelayedSkeleton> {
  bool _show = false;
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    // A CANCELLABLE one-shot timer: if the load finishes before [delay] (the
    // common case) the widget is disposed and the timer is cancelled — otherwise
    // it would linger and flutter_test fails with "A Timer is still pending".
    _timer = Timer(widget.delay, () {
      if (mounted) setState(() => _show = true);
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) =>
      _show ? widget.child : const SizedBox.shrink();
}

/// A ready-made list-shaped placeholder (title + summary rows) for an index /
/// feed / page-body loading state. Wrap in [DelayedSkeleton] at call sites.
class SkeletonList extends StatelessWidget {
  const SkeletonList({super.key, this.rows = 6});
  final int rows;

  @override
  Widget build(BuildContext context) {
    return ListView.separated(
      key: const ValueKey('skeleton-list'),
      padding: const EdgeInsets.all(QSpace.md),
      itemCount: rows,
      separatorBuilder: (_, __) => const SizedBox(height: QSpace.lg),
      itemBuilder: (_, __) => const Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Skeleton(width: 180, height: 16),
          SizedBox(height: QSpace.sm),
          Skeleton(width: double.infinity, height: 12),
          SizedBox(height: QSpace.xs),
          Skeleton(width: 240, height: 12),
        ],
      ),
    );
  }
}
