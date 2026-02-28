import 'package:flutter/material.dart';
import '../../core/network/session_service.dart';
import '../../core/theme/abot_theme.dart';
import 'facet.dart';

/// Side strip combining open facets (perspective-tilted cards) and
/// server-side sessions in a single scrollable panel.
class StageStrip extends StatelessWidget {
  final FacetData? focusedFacet;
  final List<FacetData> stripFacets;
  final List<SessionInfo> serverSessions;
  final Set<String> openSessionNames;
  final void Function(String facetId) onFocusFacet;
  final void Function(String facetId, String sessionName) onCloseFacet;
  final void Function(String sessionName) onOpenSession;
  final void Function(String sessionName) onDeleteSession;
  final VoidCallback onNewSession;

  const StageStrip({
    super.key,
    required this.focusedFacet,
    required this.stripFacets,
    required this.serverSessions,
    required this.openSessionNames,
    required this.onFocusFacet,
    required this.onCloseFacet,
    required this.onOpenSession,
    required this.onDeleteSession,
    required this.onNewSession,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    // Server sessions not currently open as facets
    final unattachedSessions = serverSessions
        .where((s) => !openSessionNames.contains(s.name))
        .toList();

    return Container(
      width: 200,
      decoration: BoxDecoration(
        color: p.mantle,
        border: Border(right: BorderSide(color: p.surface1, width: 1)),
      ),
      child: Column(
        children: [
          Expanded(
            child: ListView(
              padding: const EdgeInsets.symmetric(
                vertical: AbotSpacing.sm,
                horizontal: AbotSpacing.sm,
              ),
              children: [
                // Focused facet card (highlighted, no tilt)
                if (focusedFacet != null) ...[
                  _FocusedCard(facet: focusedFacet!),
                  const SizedBox(height: AbotSpacing.sm),
                ],

                // Open (non-focused) facets as tilted cards
                for (final facet in stripFacets) ...[
                  _StripCard(
                    facet: facet,
                    onTap: () => onFocusFacet(facet.id),
                    onClose: () => onCloseFacet(facet.id, facet.sessionName),
                  ),
                  const SizedBox(height: AbotSpacing.sm),
                ],

                // Divider if there are unattached sessions
                if (unattachedSessions.isNotEmpty) ...[
                  Padding(
                    padding: const EdgeInsets.symmetric(
                      vertical: AbotSpacing.xs,
                    ),
                    child: Row(
                      children: [
                        Expanded(
                          child: Divider(
                            color: p.surface1,
                            height: 1,
                          ),
                        ),
                        Padding(
                          padding: const EdgeInsets.symmetric(
                            horizontal: AbotSpacing.sm,
                          ),
                          child: Text(
                            'Sessions',
                            style: TextStyle(
                              fontSize: 10,
                              color: p.subtext0,
                              fontFamily: AbotFonts.mono,
                            ),
                          ),
                        ),
                        Expanded(
                          child: Divider(
                            color: p.surface1,
                            height: 1,
                          ),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(height: AbotSpacing.xs),

                  // Unattached server sessions
                  for (final session in unattachedSessions) ...[
                    _SessionTile(
                      session: session,
                      onTap: () => onOpenSession(session.name),
                      onDelete: () => onDeleteSession(session.name),
                    ),
                    const SizedBox(height: AbotSpacing.xs),
                  ],
                ],
              ],
            ),
          ),

          // New session button at bottom
          Container(
            decoration: BoxDecoration(
              border: Border(top: BorderSide(color: p.surface1, width: 1)),
            ),
            child: Material(
              color: Colors.transparent,
              child: InkWell(
                onTap: onNewSession,
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: AbotSpacing.md,
                    vertical: AbotSpacing.sm,
                  ),
                  child: Row(
                    children: [
                      Icon(Icons.add, size: 16, color: p.mauve),
                      const SizedBox(width: AbotSpacing.xs),
                      Text(
                        'New Session',
                        style: TextStyle(
                          fontSize: 12,
                          color: p.mauve,
                          fontFamily: AbotFonts.mono,
                          fontWeight: FontWeight.w500,
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
    );
  }
}

/// Highlighted card for the focused facet (flat, no perspective tilt).
class _FocusedCard extends StatelessWidget {
  final FacetData facet;

  const _FocusedCard({required this.facet});

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return Container(
      height: 48,
      decoration: BoxDecoration(
        color: p.surface0,
        borderRadius: BorderRadius.circular(AbotRadius.md),
        border: Border.all(color: p.mauve, width: 1.5),
      ),
      padding: const EdgeInsets.symmetric(horizontal: AbotSpacing.md),
      child: Row(
        children: [
          Icon(Icons.terminal, size: 16, color: p.mauve),
          const SizedBox(width: AbotSpacing.xs),
          Expanded(
            child: Text(
              facet.sessionName,
              style: TextStyle(
                fontSize: 12,
                color: p.text,
                fontFamily: AbotFonts.mono,
                fontWeight: FontWeight.w600,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }
}

class _StripCard extends StatefulWidget {
  final FacetData facet;
  final VoidCallback onTap;
  final VoidCallback onClose;

  const _StripCard({
    required this.facet,
    required this.onTap,
    required this.onClose,
  });

  @override
  State<_StripCard> createState() => _StripCardState();
}

class _StripCardState extends State<_StripCard> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    final tilt = _hovered ? 0.05 : 0.15;
    final scale = _hovered ? 1.05 : 1.0;

    return MouseRegion(
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        onTap: widget.onTap,
        child: AnimatedScale(
          scale: scale,
          duration: const Duration(milliseconds: 150),
          curve: Curves.easeOut,
          child: AnimatedContainer(
            duration: const Duration(milliseconds: 150),
            curve: Curves.easeOut,
            transform: Matrix4.identity()
              ..setEntry(3, 2, 0.001)
              ..rotateY(tilt),
            transformAlignment: Alignment.centerRight,
            height: 100,
            decoration: BoxDecoration(
              color: p.surface0,
              borderRadius: BorderRadius.circular(AbotRadius.md),
              boxShadow: [
                BoxShadow(
                  color: Colors.black.withValues(alpha: 0.3),
                  blurRadius: _hovered ? 12 : 6,
                  offset: const Offset(-2, 2),
                ),
              ],
            ),
            child: Padding(
              padding: const EdgeInsets.all(AbotSpacing.md),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Icon(Icons.terminal, size: 16, color: p.subtext0),
                      const SizedBox(width: AbotSpacing.xs),
                      Expanded(
                        child: Text(
                          widget.facet.sessionName,
                          style: TextStyle(
                            fontSize: 12,
                            color: p.text,
                            fontFamily: AbotFonts.mono,
                            fontWeight: FontWeight.w500,
                          ),
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                      InkWell(
                        onTap: widget.onClose,
                        borderRadius: BorderRadius.circular(AbotRadius.sm),
                        child: Padding(
                          padding: const EdgeInsets.all(2),
                          child: Icon(Icons.close, size: 14, color: p.subtext0),
                        ),
                      ),
                    ],
                  ),
                  const Spacer(),
                  // Decorative terminal lines
                  _TerminalPreviewLine(color: p.subtext0, widthFraction: 0.7),
                  const SizedBox(height: 4),
                  _TerminalPreviewLine(color: p.subtext0, widthFraction: 0.5),
                  const SizedBox(height: 4),
                  _TerminalPreviewLine(color: p.subtext0, widthFraction: 0.85),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Compact tile for an unattached server session.
class _SessionTile extends StatefulWidget {
  final SessionInfo session;
  final VoidCallback onTap;
  final VoidCallback onDelete;

  const _SessionTile({
    required this.session,
    required this.onTap,
    required this.onDelete,
  });

  @override
  State<_SessionTile> createState() => _SessionTileState();
}

class _SessionTileState extends State<_SessionTile> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    final statusColor = widget.session.status == 'running' ? p.green : p.subtext0;

    return MouseRegion(
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        onTap: widget.onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(
            horizontal: AbotSpacing.sm,
            vertical: AbotSpacing.sm,
          ),
          decoration: BoxDecoration(
            color: _hovered ? p.surface0 : Colors.transparent,
            borderRadius: BorderRadius.circular(AbotRadius.sm),
          ),
          child: Row(
            children: [
              Icon(Icons.terminal, size: 14, color: p.subtext0),
              const SizedBox(width: AbotSpacing.xs),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      widget.session.name,
                      style: TextStyle(
                        fontSize: 11,
                        color: p.text,
                        fontFamily: AbotFonts.mono,
                      ),
                      overflow: TextOverflow.ellipsis,
                    ),
                    Text(
                      widget.session.status,
                      style: TextStyle(
                        fontSize: 9,
                        color: statusColor,
                        fontFamily: AbotFonts.mono,
                      ),
                    ),
                  ],
                ),
              ),
              if (_hovered)
                InkWell(
                  onTap: widget.onDelete,
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  child: Padding(
                    padding: const EdgeInsets.all(2),
                    child: Icon(Icons.delete_outline, size: 14, color: p.subtext0),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Decorative line mimicking terminal output.
class _TerminalPreviewLine extends StatelessWidget {
  final Color color;
  final double widthFraction;

  const _TerminalPreviewLine({
    required this.color,
    required this.widthFraction,
  });

  @override
  Widget build(BuildContext context) {
    return FractionallySizedBox(
      widthFactor: widthFraction,
      alignment: Alignment.centerLeft,
      child: Container(
        height: 3,
        decoration: BoxDecoration(
          color: color.withValues(alpha: 0.15),
          borderRadius: BorderRadius.circular(1.5),
        ),
      ),
    );
  }
}
