// QCue S4-R29: the shared scaffold — safe-areas, a 4-item bottom bar with the
// active tab highlighted (`nav-state-active`), and the global quick-capture
// compose affordance reachable from every tab. Hairline borders, note-app flat.
// v0.2.2: Activity moved off the bottom bar into Settings (Settings → Activity),
// so the bar is Capture · Wiki · Recall · Settings.
import 'package:flutter/material.dart';
import '../core/theme/qcue_space.dart';
import '../core/theme/qcue_text.dart';
import '../core/theme/qcue_theme.dart';

class _NavItem {
  const _NavItem(this.label, this.icon);
  final String label;
  final IconData icon;
}

const _navItems = [
  _NavItem('Capture', Icons.edit_outlined),
  _NavItem('Wiki', Icons.menu_book_outlined),
  _NavItem('Recall', Icons.auto_awesome_outlined),
  _NavItem('Settings', Icons.settings_outlined),
];

class AppScaffold extends StatelessWidget {
  const AppScaffold({
    super.key,
    required this.title,
    required this.currentIndex,
    required this.onTab,
    required this.onCompose,
    required this.body,
    this.contextAction,
  });

  final String title;
  final int currentIndex;
  final ValueChanged<int> onTab;
  final VoidCallback onCompose;
  final Widget body;
  final Widget? contextAction;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: context.q.bg,
      body: SafeArea(
        bottom: false,
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                QSpace.md,
                QSpace.sm,
                QSpace.sm,
                QSpace.sm,
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      title,
                      style: QCueText.title.copyWith(color: context.q.text),
                    ),
                  ),
                  contextAction ??
                      IconButton(
                        key: const ValueKey('compose-affordance'),
                        icon: Icon(
                          Icons.add_circle_outline,
                          color: context.q.text2,
                        ),
                        tooltip: 'New capture',
                        onPressed: onCompose,
                      ),
                ],
              ),
            ),
            Divider(height: 1, thickness: 1, color: context.q.border),
            Expanded(child: body),
          ],
        ),
      ),
      bottomNavigationBar: SafeArea(
        top: false,
        child: Container(
          decoration: BoxDecoration(
            border: Border(top: BorderSide(color: context.q.border)),
            color: context.q.bg,
          ),
          child: Row(
            children: [
              for (var i = 0; i < _navItems.length; i++)
                Expanded(
                  child: Semantics(
                    selected: i == currentIndex,
                    button: true,
                    label: _navItems[i].label,
                    child: InkWell(
                      onTap: () => onTab(i),
                      child: SizedBox(
                        height: 56,
                        child: Column(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            Icon(
                              _navItems[i].icon,
                              size: 22,
                              color: i == currentIndex
                                  ? context.q.accent
                                  : context.q.text3,
                            ),
                            Text(
                              _navItems[i].label,
                              style: QCueText.caption.copyWith(
                                fontSize: 11,
                                color: i == currentIndex
                                    ? context.q.accent
                                    : context.q.text3,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
