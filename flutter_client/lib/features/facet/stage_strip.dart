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
  final void Function(String sessionName)? onSessionSettings;
  final VoidCallback onNewSession;
  final VoidCallback? onOpenBundle;
  final WsConnectionState connectionState;
  final Map<String, SessionInfo> sessionInfoMap;
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
    this.onSessionSettings,
    required this.onNewSession,
    this.onOpenBundle,
    required this.connectionState,
    this.sessionInfoMap = const {},
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
                                isDirty: sessionInfoMap[facet.sessionName]?.dirty ?? false,
                                onTap: isFocused
                                    ? null
                                    : () => onFocusFacet(facet.id),
                                onSettings: onSessionSettings != null
                                    ? () => onSessionSettings!(facet.sessionName)
                                    : null,
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
                                        fontSize: 11,
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
                                isDirty: sessionInfoMap[session.name]?.dirty ?? false,
                                onTap: () => onOpenSession(session.name),
                                onDelete: () => onDeleteSession(session.name),
                                onSettings: onSessionSettings != null
                                    ? () => onSessionSettings!(session.name)
                                    : null,
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

          // Footer: [folder] [gear] ... [dot]
          _SidebarFooter(
            connectionState: connectionState,
            onSettingsTap: onSettingsTap,
            onOpenBundle: onOpenBundle,
            collapsed: collapsed,
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
    this.size = 20,
  });

  @override
  Widget build(BuildContext context) {
    final child = Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(AbotRadius.sm),
        child: Padding(
          padding: const EdgeInsets.all(AbotSpacing.sm),
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

/// Sidebar footer — horizontal row: [folder] [gear] ... [dot]
/// Matches Katulong sidebar footer layout.
class _SidebarFooter extends StatelessWidget {
  final WsConnectionState connectionState;
  final VoidCallback? onSettingsTap;
  final VoidCallback? onOpenBundle;
  final bool collapsed;

  const _SidebarFooter({
    required this.connectionState,
    this.onSettingsTap,
    this.onOpenBundle,
    required this.collapsed,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    final Color dotColor;
    final String dotTooltip;
    switch (connectionState) {
      case WsConnectionState.connected:
        dotColor = p.green;
        dotTooltip = 'Connected';
      case WsConnectionState.connecting:
        dotColor = p.yellow;
        dotTooltip = 'Connecting...';
      case WsConnectionState.disconnected:
        dotColor = p.overlay0;
        dotTooltip = 'Disconnected';
    }

    if (collapsed) {
      return Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Divider(color: p.surface1, height: 1),
          Padding(
            padding: const EdgeInsets.symmetric(vertical: AbotSpacing.sm),
            child: Column(
              children: [
                if (onOpenBundle != null)
                  _IconBtn(
                    icon: Icons.folder_open_outlined,
                    size: 16,
                    color: p.overlay1,
                    tooltip: 'Open .abot',
                    onTap: onOpenBundle,
                  ),
                _IconBtn(
                  icon: Icons.settings,
                  size: 18,
                  color: p.overlay1,
                  tooltip: 'Settings',
                  onTap: onSettingsTap,
                ),
                const SizedBox(height: AbotSpacing.xs),
                Tooltip(
                  message: dotTooltip,
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
              ],
            ),
          ),
        ],
      );
    }

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Divider(color: p.surface1, height: 1),
        Padding(
          padding: const EdgeInsets.all(AbotSpacing.sm),
          child: Row(
            children: [
              if (onOpenBundle != null)
                _IconBtn(
                  icon: Icons.folder_open_outlined,
                  size: 18,
                  color: p.overlay1,
                  tooltip: 'Open .abot',
                  onTap: onOpenBundle,
                ),
              _IconBtn(
                icon: Icons.settings,
                size: 16,
                color: p.overlay1,
                tooltip: 'Settings',
                onTap: onSettingsTap,
              ),
              const Spacer(),
              Tooltip(
                message: dotTooltip,
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
            ],
          ),
        ),
      ],
    );
  }
}

/// Sidebar card for a facet. Matches Katulong sidebar proportions:
/// 88px preview area + footer with pill-badge session name.
class _StripCard extends StatefulWidget {
  final FacetData facet;
  final bool isFocused;
  final bool isDirty;
  final VoidCallback? onTap;
  final VoidCallback? onSettings;

  const _StripCard({
    required this.facet,
    this.isFocused = false,
    this.isDirty = false,
    this.onTap,
    this.onSettings,
  });

  @override
  State<_StripCard> createState() => _StripCardState();
}

class _StripCardState extends State<_StripCard> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return MouseRegion(
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          decoration: BoxDecoration(
            color: p.base,
            border: Border.all(
              color: widget.isFocused ? p.mauve : p.surface1,
              width: 2,
            ),
            borderRadius: BorderRadius.circular(AbotRadius.lg),
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              // Preview area — 88px
              SizedBox(
                height: 88,
                child: Stack(
                  children: [
                    if (_hovered && widget.onSettings != null)
                      Positioned(
                        right: AbotSpacing.sm,
                        top: AbotSpacing.sm,
                        child: InkWell(
                          onTap: widget.onSettings,
                          borderRadius: BorderRadius.circular(AbotRadius.sm),
                          child: Container(
                            padding: const EdgeInsets.all(3),
                            decoration: BoxDecoration(
                              color: p.surface0.withValues(alpha: 0.8),
                              borderRadius:
                                  BorderRadius.circular(AbotRadius.sm),
                            ),
                            child: Icon(
                              Icons.settings_outlined,
                              size: 16,
                              color: p.subtext0,
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
              ),
              // Footer with pill badge name
              Padding(
                padding: const EdgeInsets.fromLTRB(8, 0, 8, 6),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.end,
                  children: [
                    if (widget.isDirty)
                      Padding(
                        padding: const EdgeInsets.only(right: 4),
                        child: Container(
                          width: 5,
                          height: 5,
                          decoration: BoxDecoration(
                            color: p.yellow,
                            shape: BoxShape.circle,
                          ),
                        ),
                      ),
                    Flexible(
                      child: Container(
                        padding: const EdgeInsets.symmetric(
                          horizontal: 8,
                          vertical: 3,
                        ),
                        decoration: BoxDecoration(
                          color: p.surface0,
                          borderRadius: BorderRadius.circular(AbotRadius.sm),
                        ),
                        child: Text(
                          widget.facet.sessionName,
                          style: TextStyle(
                            fontSize: 12,
                            color: widget.isFocused ? p.mauve : p.subtext0,
                            fontFamily: AbotFonts.mono,
                          ),
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Compact tile for an unattached server session.
class _SessionTile extends StatefulWidget {
  final SessionInfo session;
  final bool isDirty;
  final VoidCallback onTap;
  final VoidCallback onDelete;
  final VoidCallback? onSettings;

  const _SessionTile({
    required this.session,
    this.isDirty = false,
    required this.onTap,
    required this.onDelete,
    this.onSettings,
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
              Icon(Icons.terminal, size: 16, color: p.subtext0),
              const SizedBox(width: AbotSpacing.sm),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Row(
                      children: [
                        Flexible(
                          child: Text(
                            widget.session.name,
                            style: TextStyle(
                              fontSize: 13,
                              color: p.text,
                              fontFamily: AbotFonts.mono,
                            ),
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                        if (widget.isDirty)
                          Padding(
                            padding: const EdgeInsets.only(left: 4),
                            child: Container(
                              width: 5,
                              height: 5,
                              decoration: BoxDecoration(
                                color: p.yellow,
                                shape: BoxShape.circle,
                              ),
                            ),
                          ),
                      ],
                    ),
                    Text(
                      widget.session.status.name,
                      style: TextStyle(
                        fontSize: 11,
                        color: statusColor,
                        fontFamily: AbotFonts.mono,
                      ),
                    ),
                  ],
                ),
              ),
              if (_hovered) ...[
                if (widget.onSettings != null)
                  InkWell(
                    onTap: widget.onSettings,
                    borderRadius: BorderRadius.circular(AbotRadius.sm),
                    child: Padding(
                      padding: const EdgeInsets.all(4),
                      child: Icon(
                        Icons.settings_outlined,
                        size: 16,
                        color: p.subtext0,
                      ),
                    ),
                  ),
                InkWell(
                  onTap: widget.onDelete,
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  child: Padding(
                    padding: const EdgeInsets.all(4),
                    child: Icon(
                      Icons.delete_outline,
                      size: 16,
                      color: p.subtext0,
                    ),
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}
