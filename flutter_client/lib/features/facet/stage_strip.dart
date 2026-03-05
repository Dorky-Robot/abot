import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:web/web.dart' as web;
import '../../core/network/abot_service.dart';
import '../../core/network/kubo_service.dart';
import '../../core/network/session_service.dart';
import '../../core/network/websocket_service.dart';
import '../../core/theme/abot_theme.dart';
import 'facet.dart';

/// Which sidebar tab is active.
enum SidebarTab { kubos, abots }

/// Side strip with two tabs: Kubos (grouped) and Abots (flat session list).
/// The [+] button is contextual — creates a kubo or abot depending on tab.
class StageStrip extends StatefulWidget {
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
  final void Function(String kubo) onNewSessionInKubo;
  final VoidCallback onNewKubo;
  final VoidCallback? onOpenBundle;
  final void Function(String kubo)? onOpenBundleInKubo;
  final VoidCallback? onOpenKubo;
  final void Function(String kuboName, String abotName)? onRemoveAbot;
  final void Function(String kuboName)? onKuboSettings;
  final WsConnectionState connectionState;
  final Map<String, SessionInfo> sessionInfoMap;
  final List<KuboInfo> kubos;
  final bool collapsed;
  final VoidCallback onToggleCollapse;
  final VoidCallback? onSettingsTap;
  final VoidCallback? onScroll;
  final void Function(SidebarTab tab)? onTabChanged;
  final String? activeKubo;
  final void Function(String kubo)? onActiveKuboChanged;
  final List<AbotInfo> knownAbots;
  final void Function(String abotName)? onAbotDetail;
  final void Function(String abotName, String kuboName)? onIntegrateVariant;
  final void Function(String abotName, String kuboName)? onDiscardVariant;
  final void Function(String abotName, String kuboName)? onDismissVariant;

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
    required this.onNewSessionInKubo,
    required this.onNewKubo,
    this.onOpenBundle,
    this.onOpenBundleInKubo,
    this.onOpenKubo,
    this.onRemoveAbot,
    this.onKuboSettings,
    required this.connectionState,
    this.sessionInfoMap = const {},
    this.kubos = const [],
    required this.collapsed,
    required this.onToggleCollapse,
    this.onSettingsTap,
    this.onScroll,
    this.onTabChanged,
    this.activeKubo,
    this.onActiveKuboChanged,
    this.knownAbots = const [],
    this.onAbotDetail,
    this.onIntegrateVariant,
    this.onDiscardVariant,
    this.onDismissVariant,
  });

  @override
  State<StageStrip> createState() => _StageStripState();
}

class _StageStripState extends State<StageStrip> {
  static const _tabKey = 'abot_sidebar_tab';
  static const _collapsedKey = 'abot_collapsed_kubos';
  static const _collapsedAbotsKey = 'abot_collapsed_abots';

  SidebarTab _activeTab = SidebarTab.abots;
  final Set<String> _collapsedKubos = {};
  final Set<String> _collapsedAbots = {};

  @override
  void initState() {
    super.initState();
    _restoreState();
    // Notify parent of restored tab so CSS transforms update correctly.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      widget.onTabChanged?.call(_activeTab);
    });
  }

  void _restoreState() {
    final storage = web.window.localStorage;
    final tab = storage.getItem(_tabKey);
    if (tab == 'kubos') _activeTab = SidebarTab.kubos;
    if (tab == 'abots') _activeTab = SidebarTab.abots;

    final collapsed = storage.getItem(_collapsedKey);
    if (collapsed != null) {
      try {
        final list = (jsonDecode(collapsed) as List).cast<String>();
        _collapsedKubos.addAll(list);
      } catch (e) {
        debugPrint('[StageStrip] Failed to restore collapsed kubos: $e');
      }
    }

    final collapsedAbots = storage.getItem(_collapsedAbotsKey);
    if (collapsedAbots != null) {
      try {
        final list = (jsonDecode(collapsedAbots) as List).cast<String>();
        _collapsedAbots.addAll(list);
      } catch (e) {
        debugPrint('[StageStrip] Failed to restore collapsed abots: $e');
      }
    }
  }

  void _persistTab() {
    web.window.localStorage.setItem(
        _tabKey, _activeTab == SidebarTab.kubos ? 'kubos' : 'abots');
  }

  void _persistCollapsed() {
    web.window.localStorage.setItem(
        _collapsedKey, jsonEncode(_collapsedKubos.toList()));
  }

  void _persistCollapsedAbots() {
    web.window.localStorage.setItem(
        _collapsedAbotsKey, jsonEncode(_collapsedAbots.toList()));
  }

  /// Return abot names from the kubo manifest that have no sessions (neither open nor unattached).
  List<String> _manifestOnlyAbots(String kuboName, _KuboGroup group) {
    final kuboInfo = widget.kubos.where((k) => k.name == kuboName).firstOrNull;
    if (kuboInfo == null) return [];
    final sessionNames = <String>{
      ...group.facets.map((f) => f.sessionName),
      ...group.unattachedSessions.map((s) => s.name),
    };
    return kuboInfo.abots.where((a) => !sessionNames.contains(a)).toList();
  }

  String _kuboFor(String sessionName) {
    return widget.sessionInfoMap[sessionName]?.kubo ?? 'default';
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return AnimatedContainer(
      duration: AbotSizes.sidebarAnimDuration,
      curve: Curves.easeInOut,
      width: widget.collapsed
          ? AbotSizes.sidebarCollapsedWidth
          : AbotSizes.sidebarExpandedWidth,
      color: p.mantle,
      child: Column(
        children: [
          _buildTopBar(p),
          if (widget.collapsed)
            const Spacer()
          else ...[
            // Tab bar
            _buildTabBar(p),
            // Tab content
            Expanded(
              child: NotificationListener<ScrollNotification>(
                onNotification: (notification) {
                  widget.onScroll?.call();
                  return false;
                },
                child: _activeTab == SidebarTab.kubos
                    ? _buildKubosTab(p)
                    : _buildAbotsTab(p),
              ),
            ),
          ],
          _SidebarFooter(
            connectionState: widget.connectionState,
            onSettingsTap: widget.onSettingsTap,
            onOpenBundle: widget.onOpenBundle,
            collapsed: widget.collapsed,
          ),
        ],
      ),
    );
  }

  Widget _buildTopBar(CatPalette p) {
    return Padding(
      padding: const EdgeInsets.only(
        top: AbotSpacing.sm,
        left: AbotSpacing.xs,
        right: AbotSpacing.xs,
      ),
      child: widget.collapsed
          ? Column(
              children: [
                _IconBtn(
                  icon: Icons.chevron_right,
                  color: p.subtext0,
                  onTap: widget.onToggleCollapse,
                  tooltip: 'Expand sidebar',
                ),
                const SizedBox(height: AbotSpacing.xs),
                _IconBtn(
                  icon: Icons.add,
                  color: p.subtext0,
                  onTap: _activeTab == SidebarTab.kubos
                      ? widget.onNewKubo
                      : widget.onNewSession,
                  tooltip: _activeTab == SidebarTab.kubos
                      ? 'New kubo'
                      : 'New abot',
                ),
              ],
            )
          : Row(
              children: [
                _IconBtn(
                  icon: Icons.chevron_left,
                  color: p.subtext0,
                  onTap: widget.onToggleCollapse,
                  tooltip: 'Collapse sidebar',
                ),
                const Spacer(),
                if (_activeTab == SidebarTab.kubos && widget.onOpenKubo != null)
                  _IconBtn(
                    icon: Icons.folder_open_outlined,
                    color: p.subtext0,
                    onTap: widget.onOpenKubo,
                    tooltip: 'Open kubo',
                  ),
                _IconBtn(
                  icon: Icons.add,
                  color: p.subtext0,
                  onTap: _activeTab == SidebarTab.kubos
                      ? widget.onNewKubo
                      : widget.onNewSession,
                  tooltip: _activeTab == SidebarTab.kubos
                      ? 'New kubo'
                      : 'New abot',
                ),
              ],
            ),
    );
  }

  Widget _buildTabBar(CatPalette p) {
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: AbotSpacing.sm,
        vertical: AbotSpacing.xs,
      ),
      child: Container(
        height: 28,
        decoration: BoxDecoration(
          color: p.surface0,
          borderRadius: BorderRadius.circular(AbotRadius.sm),
        ),
        child: Row(
          children: [
            _TabButton(
              label: 'Kubos',
              isActive: _activeTab == SidebarTab.kubos,
              onTap: () {
                setState(() => _activeTab = SidebarTab.kubos);
                _persistTab();
                widget.onTabChanged?.call(SidebarTab.kubos);
              },
            ),
            _TabButton(
              label: 'Abots',
              isActive: _activeTab == SidebarTab.abots,
              onTap: () {
                setState(() => _activeTab = SidebarTab.abots);
                _persistTab();
                widget.onTabChanged?.call(SidebarTab.abots);
              },
            ),
          ],
        ),
      ),
    );
  }

  // ── Kubos tab ────────────────────────────────────────────────────────

  Widget _buildKubosTab(CatPalette p) {
    final groups = <String, _KuboGroup>{};
    for (final k in widget.kubos) {
      groups.putIfAbsent(k.name, () => _KuboGroup(kuboName: k.name));
    }
    groups.putIfAbsent('default', () => _KuboGroup(kuboName: 'default'));

    // Assign all sessions (open and unattached) to groups
    for (final facet in widget.allFacets) {
      final kubo = _kuboFor(facet.sessionName);
      groups.putIfAbsent(kubo, () => _KuboGroup(kuboName: kubo));
      groups[kubo]!.facets.add(facet);
    }
    for (final session in widget.serverSessions) {
      if (widget.openSessionNames.contains(session.name)) continue;
      final kubo = session.kubo ?? 'default';
      groups.putIfAbsent(kubo, () => _KuboGroup(kuboName: kubo));
      groups[kubo]!.unattachedSessions.add(session);
    }

    final sortedKeys = groups.keys.toList()
      ..sort((a, b) {
        if (a == 'default') return -1;
        if (b == 'default') return 1;
        return a.compareTo(b);
      });

    final kuboRunning = <String, bool>{};
    for (final k in widget.kubos) {
      kuboRunning[k.name] = k.running;
    }

    if (sortedKeys.isEmpty) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.workspaces_outlined, size: 32, color: p.overlay0),
            const SizedBox(height: AbotSpacing.sm),
            Text('No kubos yet',
                style: TextStyle(
                    fontSize: 12,
                    color: p.overlay0,
                    fontFamily: AbotFonts.mono)),
            const SizedBox(height: AbotSpacing.sm),
            Text('Press + to create one',
                style: TextStyle(
                    fontSize: 11,
                    color: p.overlay0,
                    fontFamily: AbotFonts.mono)),
          ],
        ),
      );
    }

    return CustomScrollView(
      slivers: [
        for (final kuboName in sortedKeys)
          _buildKuboSection(
              p, kuboName, groups[kuboName]!, kuboRunning[kuboName] ?? false),
      ],
    );
  }

  Widget _buildKuboSection(
      CatPalette p, String kuboName, _KuboGroup group, bool running) {
    final isCollapsed = _collapsedKubos.contains(kuboName);
    final isActive = widget.activeKubo == kuboName;

    return SliverPadding(
      padding: const EdgeInsets.symmetric(horizontal: AbotSpacing.sm),
      sliver: SliverList(
        delegate: SliverChildListDelegate([
          GestureDetector(
            onTap: () => widget.onActiveKuboChanged?.call(kuboName),
            behavior: HitTestBehavior.translucent,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.only(top: AbotSpacing.sm),
                  child: _KuboHeader(
                    name: kuboName,
                    running: running,
                    isCollapsed: isCollapsed,
                    isActive: isActive,
                    onToggle: () {
                      setState(() {
                        if (isCollapsed) {
                          _collapsedKubos.remove(kuboName);
                        } else {
                          _collapsedKubos.add(kuboName);
                        }
                      });
                      _persistCollapsed();
                    },
                  ),
                ),
                if (!isCollapsed) ...[
                  // Open abots (have facets)
                  for (final facet in group.facets)
                    _AbotRow(
                      name: facet.sessionName,
                      isRunning: widget.sessionInfoMap[facet.sessionName]?.isRunning ?? true,
                      isFocused: facet.id == widget.focusedId,
                      isDirty: widget.sessionInfoMap[facet.sessionName]?.dirty ?? false,
                      onTap: facet.id == widget.focusedId
                          ? null
                          : () => widget.onFocusFacet(facet.id),
                      onRemove: widget.onRemoveAbot != null
                          ? () => widget.onRemoveAbot!(kuboName, facet.sessionName)
                          : null,
                    ),
                  // Unattached abots (server sessions not open as facets)
                  for (final session in group.unattachedSessions)
                    _AbotRow(
                      name: session.name,
                      isRunning: session.isRunning,
                      isFocused: false,
                      isDirty: session.dirty,
                      onTap: () => widget.onOpenSession(session.name),
                      onRemove: widget.onRemoveAbot != null
                          ? () => widget.onRemoveAbot!(kuboName, session.name)
                          : null,
                    ),
                  // Abots from manifest that have no sessions at all
                  for (final abotName in _manifestOnlyAbots(kuboName, group))
                    _AbotRow(
                      name: abotName,
                      isRunning: false,
                      isFocused: false,
                      onTap: () => widget.onOpenSession(abotName),
                      onRemove: widget.onRemoveAbot != null
                          ? () => widget.onRemoveAbot!(kuboName, abotName)
                          : null,
                    ),
                  if (group.facets.isEmpty && group.unattachedSessions.isEmpty && _manifestOnlyAbots(kuboName, group).isEmpty)
                    Padding(
                      padding: const EdgeInsets.only(
                          top: AbotSpacing.xs, left: 24),
                      child: Text(
                        'no abots',
                        style: TextStyle(
                          fontSize: 11,
                          color: p.overlay0,
                          fontFamily: AbotFonts.mono,
                          fontStyle: FontStyle.italic,
                        ),
                      ),
                    ),
                  _KuboActionBar(
                    kuboName: kuboName,
                    onAdd: () => widget.onNewSessionInKubo(kuboName),
                    onOpen: widget.onOpenBundleInKubo != null
                        ? () => widget.onOpenBundleInKubo!(kuboName)
                        : widget.onOpenBundle,
                    onSettings: widget.onKuboSettings != null
                        ? () => widget.onKuboSettings!(kuboName)
                        : null,
                  ),
                ],
                const SizedBox(height: AbotSpacing.sm),
              ],
            ),
          ),
        ]),
      ),
    );
  }

  // ── Abots tab (collapsible groups with kubo branches) ────────────────

  Widget _buildAbotsTab(CatPalette p) {
    final knownAbots = widget.knownAbots;

    // Build a set of abot names that have active sessions
    final activeAbotNames = <String>{};
    for (final session in widget.serverSessions) {
      activeAbotNames.add(session.name);
    }
    for (final facet in widget.allFacets) {
      activeAbotNames.add(facet.sessionName);
    }

    if (knownAbots.isEmpty) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.smart_toy_outlined, size: 32, color: p.overlay0),
            const SizedBox(height: AbotSpacing.sm),
            Text('No abots yet',
                style: TextStyle(
                    fontSize: 12,
                    color: p.overlay0,
                    fontFamily: AbotFonts.mono)),
            const SizedBox(height: AbotSpacing.sm),
            Text('Add an abot to a kubo to get started',
                style: TextStyle(
                    fontSize: 11,
                    color: p.overlay0,
                    fontFamily: AbotFonts.mono)),
          ],
        ),
      );
    }

    return CustomScrollView(
      slivers: [
        for (final abot in knownAbots)
          _buildAbotSection(p, abot, activeAbotNames.contains(abot.name)),
      ],
    );
  }

  Widget _buildAbotSection(CatPalette p, AbotInfo abot, bool hasActiveSession) {
    final isCollapsed = _collapsedAbots.contains(abot.name);
    final activeBranches = abot.kuboBranches.where((b) => b.hasWorktree).toList();
    final pastBranches = abot.kuboBranches.where((b) => !b.hasWorktree).toList();

    return SliverPadding(
      padding: const EdgeInsets.symmetric(horizontal: AbotSpacing.sm),
      sliver: SliverList(
        delegate: SliverChildListDelegate([
          Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.only(top: AbotSpacing.sm),
                child: _AbotGroupHeader(
                  name: abot.name,
                  hasActiveSession: hasActiveSession,
                  isCollapsed: isCollapsed,
                  onToggle: () {
                    setState(() {
                      if (isCollapsed) {
                        _collapsedAbots.remove(abot.name);
                      } else {
                        _collapsedAbots.add(abot.name);
                      }
                    });
                    _persistCollapsedAbots();
                  },
                  onTapDetail: widget.onAbotDetail != null
                      ? () => widget.onAbotDetail!(abot.name)
                      : null,
                ),
              ),
              if (!isCollapsed) ...[
                // Active kubo branches (with worktrees)
                for (final branch in activeBranches)
                  _KuboBranchRow(
                    kuboName: branch.kuboName,
                    hasSession: branch.hasSession,
                    isActive: true,
                    onTap: widget.onActiveKuboChanged != null
                        ? () => widget.onActiveKuboChanged!(branch.kuboName)
                        : null,
                    onDismiss: widget.onDismissVariant != null
                        ? () => widget.onDismissVariant!(abot.name, branch.kuboName)
                        : null,
                  ),
                // Past kubo branches (no worktree)
                for (final branch in pastBranches)
                  _KuboBranchRow(
                    kuboName: branch.kuboName,
                    hasSession: false,
                    isActive: false,
                    onIntegrate: widget.onIntegrateVariant != null
                        ? () => widget.onIntegrateVariant!(abot.name, branch.kuboName)
                        : null,
                    onDiscard: widget.onDiscardVariant != null
                        ? () => widget.onDiscardVariant!(abot.name, branch.kuboName)
                        : null,
                  ),
                if (abot.kuboBranches.isEmpty)
                  Padding(
                    padding: const EdgeInsets.only(
                        top: AbotSpacing.xs, left: 24),
                    child: Text(
                      'not employed',
                      style: TextStyle(
                        fontSize: 11,
                        color: p.overlay0,
                        fontFamily: AbotFonts.mono,
                        fontStyle: FontStyle.italic,
                      ),
                    ),
                  ),
              ],
              const SizedBox(height: AbotSpacing.sm),
            ],
          ),
        ]),
      ),
    );
  }
}

// ── Abot group header (used in Abots tab) ────────────────────────────

class _AbotGroupHeader extends StatefulWidget {
  final String name;
  final bool hasActiveSession;
  final bool isCollapsed;
  final VoidCallback onToggle;
  final VoidCallback? onTapDetail;

  const _AbotGroupHeader({
    required this.name,
    required this.hasActiveSession,
    required this.isCollapsed,
    required this.onToggle,
    this.onTapDetail,
  });

  @override
  State<_AbotGroupHeader> createState() => _AbotGroupHeaderState();
}

class _AbotGroupHeaderState extends State<_AbotGroupHeader> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return MouseRegion(
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        onTap: widget.onToggle,
        behavior: HitTestBehavior.opaque,
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: AbotSpacing.xs),
          child: Row(
            children: [
              Icon(
                widget.isCollapsed ? Icons.chevron_right : Icons.expand_more,
                size: 16,
                color: p.overlay1,
              ),
              const SizedBox(width: 4),
              if (widget.hasActiveSession)
                Padding(
                  padding: const EdgeInsets.only(right: 4),
                  child: Container(
                    width: 6,
                    height: 6,
                    decoration: BoxDecoration(
                      color: p.green,
                      shape: BoxShape.circle,
                    ),
                  ),
                ),
              Expanded(
                child: Text(
                  widget.name,
                  style: TextStyle(
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                    color: p.subtext0,
                    fontFamily: AbotFonts.mono,
                    letterSpacing: 0.5,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (_hovered && widget.onTapDetail != null)
                InkWell(
                  onTap: widget.onTapDetail,
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  child: Padding(
                    padding: const EdgeInsets.all(2),
                    child: Icon(Icons.info_outline, size: 14, color: p.overlay1),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

// ── Kubo branch row (used in Abots tab) ──────────────────────────────

class _KuboBranchRow extends StatefulWidget {
  final String kuboName;
  final bool isActive;
  final bool hasSession;
  final VoidCallback? onTap;
  final VoidCallback? onDismiss;
  final VoidCallback? onIntegrate;
  final VoidCallback? onDiscard;

  const _KuboBranchRow({
    required this.kuboName,
    required this.isActive,
    this.hasSession = false,
    this.onTap,
    this.onDismiss,
    this.onIntegrate,
    this.onDiscard,
  });

  @override
  State<_KuboBranchRow> createState() => _KuboBranchRowState();
}

class _KuboBranchRowState extends State<_KuboBranchRow> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    // Dot color: bright green if session active, dim green if worktree only, grey if past
    final dotColor = widget.hasSession
        ? p.green
        : widget.isActive
            ? p.green.withValues(alpha: 0.5)
            : p.overlay0;

    return MouseRegion(
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        onTap: widget.onTap,
        behavior: HitTestBehavior.opaque,
        child: Container(
          padding: const EdgeInsets.symmetric(
            horizontal: AbotSpacing.sm,
            vertical: 5,
          ),
          margin: const EdgeInsets.only(left: 12),
          decoration: BoxDecoration(
            color: _hovered ? p.surface0 : Colors.transparent,
            borderRadius: BorderRadius.circular(AbotRadius.sm),
          ),
          child: Row(
            children: [
              Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(
                  color: dotColor,
                  shape: BoxShape.circle,
                ),
              ),
              const SizedBox(width: AbotSpacing.sm),
              Expanded(
                child: Text(
                  widget.kuboName,
                  style: TextStyle(
                    fontSize: 12,
                    color: widget.onTap != null ? p.text : p.subtext0,
                    fontFamily: AbotFonts.mono,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (_hovered) ...[
                if (widget.isActive && widget.onDismiss != null)
                  _ActionChip(label: 'dismiss', color: p.subtext0, onTap: widget.onDismiss!),
                if (!widget.isActive && widget.onIntegrate != null)
                  _ActionChip(label: 'integrate', color: p.green, onTap: widget.onIntegrate!),
                if (!widget.isActive && widget.onDiscard != null) ...[
                  const SizedBox(width: 4),
                  _ActionChip(label: 'discard', color: p.red, onTap: widget.onDiscard!),
                ],
              ],
            ],
          ),
        ),
      ),
    );
  }
}

class _ActionChip extends StatelessWidget {
  final String label;
  final Color color;
  final VoidCallback onTap;

  const _ActionChip({
    required this.label,
    required this.color,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 1),
        decoration: BoxDecoration(
          color: p.surface1,
          borderRadius: BorderRadius.circular(AbotRadius.sm),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 9,
            color: color,
            fontFamily: AbotFonts.mono,
          ),
        ),
      ),
    );
  }
}

// ── Internal grouping helper ─────────────────────────────────────────

class _KuboGroup {
  final String kuboName;
  final List<FacetData> facets = [];
  final List<SessionInfo> unattachedSessions = [];
  _KuboGroup({required this.kuboName});
}

// ── Compact abot participant row (used in Kubos tab) ─────────────────

class _AbotRow extends StatefulWidget {
  final String name;
  final bool isRunning;
  final bool isFocused;
  final bool isDirty;
  final VoidCallback? onTap;
  final VoidCallback? onRemove;

  const _AbotRow({
    required this.name,
    required this.isRunning,
    required this.isFocused,
    this.isDirty = false,
    this.onTap,
    this.onRemove,
  });

  @override
  State<_AbotRow> createState() => _AbotRowState();
}

class _AbotRowState extends State<_AbotRow> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return MouseRegion(
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        onTap: widget.onTap,
        behavior: HitTestBehavior.opaque,
        child: Container(
          padding: const EdgeInsets.symmetric(
            horizontal: AbotSpacing.sm,
            vertical: 5,
          ),
          margin: const EdgeInsets.only(left: 12),
          decoration: BoxDecoration(
            color: widget.isFocused
                ? p.surface1
                : (_hovered ? p.surface0 : Colors.transparent),
            borderRadius: BorderRadius.circular(AbotRadius.sm),
          ),
          child: Row(
            children: [
              // Status dot
              Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(
                  color: widget.isRunning
                      ? (widget.isFocused ? p.mauve : p.green)
                      : p.overlay0,
                  shape: BoxShape.circle,
                ),
              ),
              const SizedBox(width: AbotSpacing.sm),
              // Name
              Expanded(
                child: Text(
                  widget.name,
                  style: TextStyle(
                    fontSize: 12,
                    color: widget.isFocused ? p.text : p.subtext0,
                    fontWeight: widget.isFocused
                        ? FontWeight.w600
                        : FontWeight.normal,
                    fontFamily: AbotFonts.mono,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              // Dirty indicator
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
              // Remove button on hover
              if (_hovered && widget.onRemove != null)
                Padding(
                  padding: const EdgeInsets.only(left: 2),
                  child: InkWell(
                    onTap: widget.onRemove,
                    borderRadius: BorderRadius.circular(AbotRadius.sm),
                    child: Padding(
                      padding: const EdgeInsets.all(2),
                      child: Icon(Icons.close, size: 12, color: p.overlay1),
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

// ── Tab button ───────────────────────────────────────────────────────

class _TabButton extends StatelessWidget {
  final String label;
  final bool isActive;
  final VoidCallback onTap;

  const _TabButton({
    required this.label,
    required this.isActive,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Expanded(
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: isActive ? p.surface1 : Colors.transparent,
            borderRadius: BorderRadius.circular(AbotRadius.sm),
          ),
          child: Text(
            label,
            style: TextStyle(
              fontSize: 11,
              fontWeight: isActive ? FontWeight.w600 : FontWeight.normal,
              color: isActive ? p.text : p.overlay1,
              fontFamily: AbotFonts.mono,
            ),
          ),
        ),
      ),
    );
  }
}

// ── Kubo section header ──────────────────────────────────────────────

class _KuboHeader extends StatelessWidget {
  final String name;
  final bool running;
  final bool isCollapsed;
  final bool isActive;
  final VoidCallback onToggle;

  const _KuboHeader({
    required this.name,
    required this.running,
    required this.isCollapsed,
    this.isActive = false,
    required this.onToggle,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return GestureDetector(
      onTap: onToggle,
      behavior: HitTestBehavior.opaque,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: AbotSpacing.xs),
        child: Row(
          children: [
            Icon(
              isCollapsed ? Icons.chevron_right : Icons.expand_more,
              size: 16,
              color: p.overlay1,
            ),
            const SizedBox(width: 4),
            if (running)
              Padding(
                padding: const EdgeInsets.only(right: 4),
                child: Container(
                  width: 6,
                  height: 6,
                  decoration: BoxDecoration(
                    color: p.green,
                    shape: BoxShape.circle,
                  ),
                ),
              ),
            Expanded(
              child: Text(
                name,
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                  color: isActive ? p.mauve : p.subtext0,
                  fontFamily: AbotFonts.mono,
                  letterSpacing: 0.5,
                ),
                overflow: TextOverflow.ellipsis,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// ── Kubo action bar (inside collapsible area) ────────────────────────

class _KuboActionBar extends StatelessWidget {
  final String kuboName;
  final VoidCallback onAdd;
  final VoidCallback? onOpen;
  final VoidCallback? onSettings;

  const _KuboActionBar({
    required this.kuboName,
    required this.onAdd,
    this.onOpen,
    this.onSettings,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Padding(
      padding: const EdgeInsets.only(left: 12, top: 2, bottom: 2),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          _IconBtn(
            icon: Icons.add,
            color: p.overlay1,
            size: 14,
            onTap: onAdd,
            tooltip: 'New abot in $kuboName',
          ),
          if (onOpen != null)
            _IconBtn(
              icon: Icons.folder_open_outlined,
              color: p.overlay1,
              size: 14,
              onTap: onOpen,
              tooltip: 'Open abot in $kuboName',
            ),
          if (onSettings != null)
            _IconBtn(
              icon: Icons.settings_outlined,
              color: p.overlay1,
              size: 14,
              onTap: onSettings,
              tooltip: 'Settings for $kuboName',
            ),
        ],
      ),
    );
  }
}

// ── Icon button ──────────────────────────────────────────────────────

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

// ── Sidebar footer ───────────────────────────────────────────────────

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

// ── Strip card ───────────────────────────────────────────────────────

class _StripCard extends StatefulWidget {
  final FacetData facet;

  const _StripCard({
    required this.facet,
  });

  @override
  State<_StripCard> createState() => _StripCardState();
}

class _StripCardState extends State<_StripCard> {
  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Container(
          decoration: BoxDecoration(
            color: p.base,
            border: Border.all(
              color: p.surface1,
              width: 2,
            ),
            borderRadius: BorderRadius.circular(AbotRadius.lg),
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const SizedBox(height: 88),
              Padding(
                padding: const EdgeInsets.fromLTRB(8, 0, 8, 6),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.end,
                  children: [
                    Flexible(
                      child: Container(
                        padding: const EdgeInsets.symmetric(
                            horizontal: 8, vertical: 3),
                        decoration: BoxDecoration(
                          color: p.surface0,
                          borderRadius:
                              BorderRadius.circular(AbotRadius.sm),
                        ),
                        child: Text(
                          widget.facet.sessionName,
                          style: TextStyle(
                            fontSize: 12,
                            color: p.subtext0,
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
    );
  }
}

// ── Session tile ─────────────────────────────────────────────────────

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
              Icon(Icons.terminal, size: 16, color: p.subtext0),
              const SizedBox(width: AbotSpacing.sm),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      widget.session.name,
                      style: TextStyle(
                        fontSize: 13,
                        color: p.text,
                        fontFamily: AbotFonts.mono,
                      ),
                      overflow: TextOverflow.ellipsis,
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
                InkWell(
                  onTap: widget.onDelete,
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  child: Padding(
                    padding: const EdgeInsets.all(4),
                    child: Icon(Icons.delete_outline,
                        size: 16, color: p.subtext0),
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
