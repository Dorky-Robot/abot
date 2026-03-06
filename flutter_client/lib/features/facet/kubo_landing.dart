import 'package:flutter/material.dart';
import '../../core/network/kubo_service.dart';
import '../../core/network/session_service.dart';
import '../../core/theme/abot_theme.dart';

/// Kubo landing page — shows abot card grid or empty onboarding.
class KuboLandingPage extends StatelessWidget {
  final String kubo;
  final Map<String, SessionInfo> sessionInfoMap;
  final List<KuboInfo> kubos;
  final void Function(String abotName) onOpenSession;
  final void Function(String abotName, String kubo) onCreateAbotSession;
  final void Function(String kubo) onAddAbot;
  final VoidCallback onOpenBundle;

  const KuboLandingPage({
    super.key,
    required this.kubo,
    required this.sessionInfoMap,
    required this.kubos,
    required this.onOpenSession,
    required this.onCreateAbotSession,
    required this.onAddAbot,
    required this.onOpenBundle,
  });

  @override
  Widget build(BuildContext context) {
    final kuboSessions = sessionInfoMap.values
        .where((s) => s.kubo == kubo)
        .toList();
    if (kuboSessions.isEmpty) {
      final kuboInfo = kubos.where((k) => k.name == kubo).firstOrNull;
      if (kuboInfo != null && kuboInfo.abots.isNotEmpty) {
        return __AbotCardGrid(
          kubo: kubo,
          cards: [
            for (final name in kuboInfo.abots)
              _AbotCard(
                name: name,
                isRunning: false,
                onTap: () => onCreateAbotSession(name, kubo),
              ),
          ],
          onAddAbot: onAddAbot,
          onOpenBundle: onOpenBundle,
        );
      }
      return _EmptyKuboOnboarding(
        kubo: kubo,
        onAddAbot: onAddAbot,
        onOpenBundle: onOpenBundle,
      );
    }
    return __AbotCardGrid(
      kubo: kubo,
      cards: [
        for (final session in kuboSessions)
          _AbotCard(
            name: session.displayName,
            isRunning: session.isRunning,
            isDirty: session.dirty,
            onTap: () => onOpenSession(session.name),
          ),
      ],
      onAddAbot: onAddAbot,
      onOpenBundle: onOpenBundle,
    );
  }
}

/// Empty state landing page — no kubos open.
class EmptyStateLandingPage extends StatelessWidget {
  final VoidCallback onCreateKubo;
  final VoidCallback onOpenKubo;

  const EmptyStateLandingPage({
    super.key,
    required this.onCreateKubo,
    required this.onOpenKubo,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Center(
      child: Container(
        constraints: const BoxConstraints(maxWidth: 360),
        padding: const EdgeInsets.all(AbotSpacing.lg),
        decoration: BoxDecoration(
          color: p.surface0,
          borderRadius: BorderRadius.circular(AbotRadius.md),
          border: Border.all(color: p.surface1),
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.dashboard_outlined, size: 40, color: p.mauve),
            const SizedBox(height: AbotSpacing.md),
            Text('Welcome to abot',
              style: TextStyle(
                color: p.text, fontFamily: AbotFonts.mono,
                fontSize: 16, fontWeight: FontWeight.w600,
              )),
            const SizedBox(height: AbotSpacing.sm),
            Text(
              'A kubo is a shared runtime room. Create one to get started, or open an existing one from disk.',
              textAlign: TextAlign.center,
              style: TextStyle(
                color: p.subtext0, fontFamily: AbotFonts.mono, fontSize: 12,
              ),
            ),
            const SizedBox(height: AbotSpacing.md),
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                ElevatedButton.icon(
                  onPressed: onCreateKubo,
                  icon: const Icon(Icons.add, size: 16),
                  label: const Text('Create kubo'),
                  style: ElevatedButton.styleFrom(
                    backgroundColor: p.mauve,
                    foregroundColor: p.base,
                    textStyle: TextStyle(fontFamily: AbotFonts.mono, fontSize: 12),
                  ),
                ),
                const SizedBox(width: AbotSpacing.sm),
                OutlinedButton.icon(
                  onPressed: onOpenKubo,
                  icon: const Icon(Icons.folder_open, size: 16),
                  label: const Text('Open kubo'),
                  style: OutlinedButton.styleFrom(
                    foregroundColor: p.text,
                    side: BorderSide(color: p.surface1),
                    textStyle: TextStyle(fontFamily: AbotFonts.mono, fontSize: 12),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

// ── Internal widgets ───────────────────────────────────

class _KuboActionButtons extends StatelessWidget {
  final String kubo;
  final void Function(String kubo) onAddAbot;
  final VoidCallback onOpenBundle;

  const _KuboActionButtons({
    required this.kubo,
    required this.onAddAbot,
    required this.onOpenBundle,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        FilledButton.icon(
          onPressed: () => onAddAbot(kubo),
          icon: const Icon(Icons.add, size: 16),
          label: Text('Add abot',
            style: TextStyle(fontFamily: AbotFonts.mono, fontSize: 13)),
          style: FilledButton.styleFrom(
            backgroundColor: p.mauve, foregroundColor: p.base,
          ),
        ),
        const SizedBox(width: AbotSpacing.sm),
        OutlinedButton.icon(
          onPressed: onOpenBundle,
          icon: const Icon(Icons.folder_open, size: 16),
          label: Text('Open .abot bundle',
            style: TextStyle(fontFamily: AbotFonts.mono, fontSize: 13)),
          style: OutlinedButton.styleFrom(
            foregroundColor: p.subtext0,
            side: BorderSide(color: p.surface1),
          ),
        ),
      ],
    );
  }
}

class __AbotCardGrid extends StatelessWidget {
  final String kubo;
  final List<Widget> cards;
  final void Function(String kubo) onAddAbot;
  final VoidCallback onOpenBundle;

  const __AbotCardGrid({
    required this.kubo,
    required this.cards,
    required this.onAddAbot,
    required this.onOpenBundle,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(AbotSpacing.lg),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(kubo,
              style: TextStyle(
                color: p.text, fontFamily: AbotFonts.mono,
                fontSize: 16, fontWeight: FontWeight.w600,
              )),
            const SizedBox(height: AbotSpacing.md),
            Wrap(
              spacing: AbotSpacing.md,
              runSpacing: AbotSpacing.md,
              children: cards,
            ),
            const SizedBox(height: AbotSpacing.lg),
            _KuboActionButtons(kubo: kubo, onAddAbot: onAddAbot, onOpenBundle: onOpenBundle),
          ],
        ),
      ),
    );
  }
}

class _EmptyKuboOnboarding extends StatelessWidget {
  final String kubo;
  final void Function(String kubo) onAddAbot;
  final VoidCallback onOpenBundle;

  const _EmptyKuboOnboarding({
    required this.kubo,
    required this.onAddAbot,
    required this.onOpenBundle,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Center(
      child: Container(
        constraints: const BoxConstraints(maxWidth: 360),
        padding: const EdgeInsets.all(AbotSpacing.lg),
        decoration: BoxDecoration(
          color: p.surface0,
          borderRadius: BorderRadius.circular(AbotRadius.md),
          border: Border.all(color: p.surface1),
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('No abots in $kubo',
              style: TextStyle(
                color: p.text, fontFamily: AbotFonts.mono,
                fontSize: 16, fontWeight: FontWeight.w600,
              )),
            const SizedBox(height: AbotSpacing.sm),
            Text(
              'Add an abot to get started. Each abot is a git-backed workspace with its own terminal.',
              textAlign: TextAlign.center,
              style: TextStyle(
                color: p.subtext0, fontFamily: AbotFonts.mono, fontSize: 12,
              ),
            ),
            const SizedBox(height: AbotSpacing.md),
            _KuboActionButtons(kubo: kubo, onAddAbot: onAddAbot, onOpenBundle: onOpenBundle),
          ],
        ),
      ),
    );
  }
}

/// A card representing an abot in the kubo landing page grid.
class _AbotCard extends StatefulWidget {
  final String name;
  final bool isRunning;
  final bool isDirty;
  final VoidCallback onTap;

  const _AbotCard({
    required this.name,
    required this.isRunning,
    this.isDirty = false,
    required this.onTap,
  });

  @override
  State<_AbotCard> createState() => __AbotCardState();
}

class __AbotCardState extends State<_AbotCard> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return MouseRegion(
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        onTap: widget.onTap,
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 150),
          width: 180,
          padding: const EdgeInsets.all(AbotSpacing.md),
          decoration: BoxDecoration(
            color: p.surface0,
            border: Border.all(
              color: _hovered ? p.mauve : p.surface1,
            ),
            borderRadius: BorderRadius.circular(AbotRadius.md),
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Container(
                    width: 8,
                    height: 8,
                    decoration: BoxDecoration(
                      color: widget.isRunning ? p.green : p.overlay0,
                      shape: BoxShape.circle,
                    ),
                  ),
                  const SizedBox(width: AbotSpacing.sm),
                  Expanded(
                    child: Text(
                      widget.name,
                      style: TextStyle(
                        color: p.text,
                        fontFamily: AbotFonts.mono,
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                      ),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  if (widget.isDirty)
                    Container(
                      width: 6,
                      height: 6,
                      decoration: BoxDecoration(
                        color: p.yellow,
                        shape: BoxShape.circle,
                      ),
                    ),
                ],
              ),
              const SizedBox(height: AbotSpacing.xs),
              Text(
                widget.isRunning ? 'running' : 'stopped',
                style: TextStyle(
                  color: p.subtext0,
                  fontFamily: AbotFonts.mono,
                  fontSize: 11,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
