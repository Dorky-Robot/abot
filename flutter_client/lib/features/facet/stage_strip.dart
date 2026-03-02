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
  final void Function(String sessionName) onOpenSession;
  final void Function(String sessionName) onDeleteSession;
  final VoidCallback onNewSession;
  final WsConnectionState connectionState;
  final bool collapsed;
  final VoidCallback onToggleCollapse;
  final VoidCallback? onSettingsTap;
  final VoidCallback? onScroll;

  const StageStrip({
    super.key,
    required this.allFacets,
    required this.focusedId,
    this.cardKeys,
    required this.serverSessions,
    required this.openSessionNames,
    required this.onFocusFacet,
    required this.onOpenSession,
    required this.onDeleteSession,
    required this.onNewSession,
    required this.connectionState,
    required this.collapsed,
    required this.onToggleCollapse,
    this.onSettingsTap,
    this.onScroll,
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
              child: NotificationListener<ScrollNotification>(
                onNotification: (notification) {
                  onScroll?.call();
                  return false;
                },
                child: CustomScrollView(
                  slivers: [
                    // All facets in stable order
                    SliverPadding(
                      padding: const EdgeInsets.symmetric(
                        vertical: AbotSpacing.sm,
                        horizontal: AbotSpacing.sm,
                      ),
                      sliver: SliverList(
                        delegate: SliverChildBuilderDelegate((context, index) {
                          final facet = allFacets[index];
                          final isFocused = facet.id == focusedId;

                          return Padding(
                            key: ValueKey(facet.id),
                            padding: const EdgeInsets.only(
                              bottom: AbotSpacing.sm,
                            ),
                            child: SizedBox(
                              key: cardKeys?[facet.id],
                              child: _StripCard(
                                facet: facet,
                                isFocused: isFocused,
                                onTap: isFocused
                                    ? null
                                    : () => onFocusFacet(facet.id),
                              ),
                            ),
                          );
                        }, childCount: allFacets.length),
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

  const _SidebarFooter({required this.connectionState, this.onSettingsTap});

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

/// Sidebar card for a facet. The CSS-transformed xterm.js overlay provides the
/// visual preview. The card itself shows a subtle border and session name as
/// fallback (visible when no CSS overlay is present, e.g. single-facet state).
class _StripCard extends StatelessWidget {
  final FacetData facet;
  final bool isFocused;
  final VoidCallback? onTap;

  const _StripCard({required this.facet, this.isFocused = false, this.onTap});

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: onTap,
      child: Container(
        height: 100,
        decoration: BoxDecoration(
          color: p.base,
          border: Border.all(
            color: isFocused ? p.mauve : p.surface1,
            width: isFocused ? 1.5 : 1,
          ),
          borderRadius: BorderRadius.circular(AbotRadius.md),
        ),
        alignment: Alignment.bottomRight,
        padding: const EdgeInsets.all(AbotSpacing.xs),
        child: Text(
          facet.sessionName,
          style: TextStyle(
            fontSize: 9,
            color: p.subtext0,
            fontFamily: AbotFonts.mono,
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
    final statusColor = widget.session.isRunning ? p.green : p.subtext0;

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
                      widget.session.status.name,
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
                    child: Icon(
                      Icons.delete_outline,
                      size: 14,
                      color: p.subtext0,
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
