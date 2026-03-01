import 'package:flutter/material.dart';
import '../../core/network/session_service.dart';
import '../../core/network/websocket_service.dart';
import '../../core/theme/abot_theme.dart';
import 'facet.dart';

/// Side strip combining all open facets and unattached server sessions.
/// All facets appear in stable order; the focused one is highlighted inline.
class StageStrip extends StatelessWidget {
  /// All facets in their stable order (focused included).
  final List<FacetData> allFacets;
  final String? focusedId;
  final Map<String, GlobalKey>? cardKeys;
  final List<SessionInfo> serverSessions;
  final Set<String> openSessionNames;
  final void Function(String facetId) onFocusFacet;
  final void Function(int oldIndex, int newIndex) onReorder;
  final void Function(String sessionName) onOpenSession;
  final void Function(String sessionName) onDeleteSession;
  final VoidCallback onNewSession;
  final WsConnectionState connectionState;
  final bool collapsed;
  final VoidCallback onToggleCollapse;
  final VoidCallback? onSettingsTap;

  const StageStrip({
    super.key,
    required this.allFacets,
    required this.focusedId,
    this.cardKeys,
    required this.serverSessions,
    required this.openSessionNames,
    required this.onFocusFacet,
    required this.onReorder,
    required this.onOpenSession,
    required this.onDeleteSession,
    required this.onNewSession,
    required this.connectionState,
    required this.collapsed,
    required this.onToggleCollapse,
    this.onSettingsTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    // Server sessions not currently open as facets
    final unattachedSessions = serverSessions
        .where((s) => !openSessionNames.contains(s.name))
        .toList();

    return AnimatedContainer(
      duration: AbotSizes.sidebarAnimDuration,
      curve: Curves.easeInOut,
      width: collapsed
          ? AbotSizes.sidebarCollapsedWidth
          : AbotSizes.sidebarExpandedWidth,
      color: p.mantle,
      child: Column(
        children: [
          // Top bar: toggle + add button
          Padding(
            padding: const EdgeInsets.only(
              top: AbotSpacing.sm,
              left: AbotSpacing.xs,
              right: AbotSpacing.xs,
            ),
            child: collapsed
                ? Column(
                    children: [
                      _IconBtn(
                        icon: Icons.chevron_right,
                        color: p.subtext0,
                        onTap: onToggleCollapse,
                        tooltip: 'Expand sidebar',
                      ),
                      const SizedBox(height: AbotSpacing.xs),
                      _IconBtn(
                        icon: Icons.add,
                        color: p.subtext0,
                        onTap: onNewSession,
                        tooltip: 'New session',
                      ),
                    ],
                  )
                : Row(
                    children: [
                      _IconBtn(
                        icon: Icons.chevron_left,
                        color: p.subtext0,
                        onTap: onToggleCollapse,
                        tooltip: 'Collapse sidebar',
                      ),
                      const Spacer(),
                      _IconBtn(
                        icon: Icons.add,
                        color: p.subtext0,
                        onTap: onNewSession,
                        tooltip: 'New session',
                      ),
                    ],
                  ),
          ),

          // Middle: cards or spacer
          if (collapsed)
            const Spacer()
          else
            Expanded(
              child: CustomScrollView(
                slivers: [
                  // All facets in stable order — reorderable
                  SliverPadding(
                    padding: const EdgeInsets.symmetric(
                      vertical: AbotSpacing.sm,
                      horizontal: AbotSpacing.sm,
                    ),
                    sliver: SliverReorderableList(
                      itemCount: allFacets.length,
                      onReorder: onReorder,
                      itemBuilder: (context, index) {
                        final facet = allFacets[index];
                        final isFocused = facet.id == focusedId;

                        return ReorderableDelayedDragStartListener(
                          key: ValueKey(facet.id),
                          index: index,
                          child: Padding(
                            padding: const EdgeInsets.only(
                              bottom: AbotSpacing.sm,
                            ),
                            child: SizedBox(
                              key: cardKeys?[facet.id],
                              child: isFocused
                                  ? _FocusedCard(facet: facet)
                                  : _StripCard(
                                      facet: facet,
                                      onTap: () => onFocusFacet(facet.id),
                                    ),
                            ),
                          ),
                        );
                      },
                    ),
                  ),

                  // Unattached server sessions
                  if (unattachedSessions.isNotEmpty)
                    SliverPadding(
                      padding: const EdgeInsets.symmetric(
                        horizontal: AbotSpacing.sm,
                      ),
                      sliver: SliverList(
                        delegate: SliverChildListDelegate([
                          Padding(
                            padding: const EdgeInsets.symmetric(
                              vertical: AbotSpacing.xs,
                            ),
                            child: Row(
                              children: [
                                Expanded(
                                  child: Divider(
                                    color: p.surface1, height: 1,
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
                                    color: p.surface1, height: 1,
                                  ),
                                ),
                              ],
                            ),
                          ),
                          const SizedBox(height: AbotSpacing.xs),
                          for (final session in unattachedSessions) ...[
                            _SessionTile(
                              session: session,
                              onTap: () => onOpenSession(session.name),
                              onDelete: () => onDeleteSession(session.name),
                            ),
                            const SizedBox(height: AbotSpacing.xs),
                          ],
                        ]),
                      ),
                    ),
                ],
              ),
            ),

          // Footer: gear above dot (both states)
          _SidebarFooter(
            connectionState: connectionState,
            onSettingsTap: onSettingsTap,
          ),
        ],
      ),
    );
  }
}

/// Small icon button used in the strip.
class _IconBtn extends StatelessWidget {
  final IconData icon;
  final Color color;
  final VoidCallback? onTap;
  final String? tooltip;
  final double size;

  const _IconBtn({
    required this.icon,
    required this.color,
    this.onTap,
    this.tooltip,
    this.size = 18,
  });

  @override
  Widget build(BuildContext context) {
    final child = Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(AbotRadius.sm),
        child: Padding(
          padding: const EdgeInsets.all(AbotSpacing.xs),
          child: Icon(icon, size: size, color: color),
        ),
      ),
    );
    if (tooltip != null) {
      return Tooltip(message: tooltip!, child: child);
    }
    return child;
  }
}

/// Sidebar footer showing gear icon above connection status dot.
class _SidebarFooter extends StatelessWidget {
  final WsConnectionState connectionState;
  final VoidCallback? onSettingsTap;

  const _SidebarFooter({
    required this.connectionState,
    this.onSettingsTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    final Color dotColor;
    final String tooltip;
    switch (connectionState) {
      case WsConnectionState.connected:
        dotColor = p.green;
        tooltip = 'Connected';
      case WsConnectionState.connecting:
        dotColor = p.yellow;
        tooltip = 'Connecting...';
      case WsConnectionState.disconnected:
        dotColor = p.overlay0;
        tooltip = 'Disconnected';
    }

    return Align(
      alignment: Alignment.bottomLeft,
      child: Padding(
        padding: const EdgeInsets.only(
          left: AbotSpacing.xs,
          top: AbotSpacing.sm,
          bottom: AbotSpacing.sm,
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _IconBtn(
              icon: Icons.settings,
              size: 16,
              color: p.overlay1,
              tooltip: 'Settings',
              onTap: onSettingsTap,
            ),
            const SizedBox(height: AbotSpacing.sm),
            Padding(
              // Nudge dot to visually center under the 16px gear icon
              padding: const EdgeInsets.only(left: AbotSpacing.xs + 3),
              child: Tooltip(
                message: tooltip,
                child: AnimatedContainer(
                  duration: const Duration(milliseconds: 200),
                  width: AbotSizes.statusDotSize,
                  height: AbotSizes.statusDotSize,
                  decoration: BoxDecoration(
                    color: dotColor,
                    shape: BoxShape.circle,
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Highlighted indicator for the focused facet — terminal content is in the
/// main area so this card just shows a label.
class _FocusedCard extends StatelessWidget {
  final FacetData facet;

  const _FocusedCard({required this.facet});

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return Container(
      height: 100,
      decoration: BoxDecoration(
        color: p.surface0,
        borderRadius: BorderRadius.circular(AbotRadius.md),
        border: Border.all(color: p.mauve, width: 1.5),
      ),
      padding: const EdgeInsets.all(AbotSpacing.sm),
      child: Row(
        children: [
          Icon(Icons.terminal, size: 14, color: p.mauve),
          const SizedBox(width: AbotSpacing.xs),
          Expanded(
            child: Text(
              facet.sessionName,
              style: TextStyle(
                fontSize: 11,
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

/// Slot for a non-focused facet. The actual terminal content is rendered on
/// top of this card via CSS-transformed xterm.js DOM element (GPU-accelerated).
/// This widget provides the click target and background.
class _StripCard extends StatelessWidget {
  final FacetData facet;
  final VoidCallback onTap;

  const _StripCard({
    required this.facet,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return GestureDetector(
      onTap: onTap,
      child: Container(
        height: 100,
        decoration: BoxDecoration(
          color: p.surface0,
          borderRadius: BorderRadius.circular(AbotRadius.md),
          border: Border.all(
            color: p.surface1,
            width: 0.5,
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
