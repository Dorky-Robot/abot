import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import '../../core/network/abot_service.dart';
import '../../core/network/api_client.dart';
import '../../core/network/kubo_service.dart';
import '../../core/network/session_service.dart';
import '../../core/network/websocket_service.dart';
import '../../core/theme/abot_theme.dart';
import '../terminal/terminal_facet.dart';
import 'facet.dart';
import 'facet_manager.dart';
import 'overlay_provider.dart';
import 'sidebar_provider.dart';
import 'workspace_provider.dart';

/// Side strip with two tabs: Kubos (grouped) and Abots (flat session list).
/// Watches providers directly for reactive updates (no prop-drilling for data).
/// Only CSS-transform callbacks remain as constructor params — everything else
/// (dialogs, provider mutations, file pickers) is handled internally.
class StageStrip extends ConsumerStatefulWidget {
  final Map<String, GlobalKey>? cardKeys;
  final VoidCallback? onScroll;
  final VoidCallback onToggleCollapse;

  const StageStrip({
    super.key,
    this.cardKeys,
    this.onScroll,
    required this.onToggleCollapse,
  });

  @override
  ConsumerState<StageStrip> createState() => _StageStripState();
}

class _StageStripState extends ConsumerState<StageStrip> {
  static const _collapsedKey = 'abot_collapsed_kubos';
  static const _collapsedAbotsKey = 'abot_collapsed_abots';

  final Set<String> _collapsedKubos = {};
  final Set<String> _collapsedAbots = {};

  @override
  void initState() {
    super.initState();
    _restoreCollapsed();
    // Notify parent of restored tab so CSS transforms update correctly.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      widget.onScroll?.call();
    });
  }

  void _restoreCollapsed() {
    final storage = web.window.localStorage;

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

  void _persistCollapsed() {
    web.window.localStorage.setItem(
        _collapsedKey, jsonEncode(_collapsedKubos.toList()));
  }

  void _persistCollapsedAbots() {
    web.window.localStorage.setItem(
        _collapsedAbotsKey, jsonEncode(_collapsedAbots.toList()));
  }

  // ── Actions (formerly callbacks from FacetShell) ──────────────────

  void _focusFacet(String facetId) {
    final currentFocused = ref.read(facetManagerProvider).focusedId;
    if (facetId == currentFocused) return;
    ref.read(facetManagerProvider.notifier).focus(facetId);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) TerminalRegistry.instance.focusTerminal(facetId);
    });
  }

  void _openSession(String sessionName) {
    ref.read(facetManagerProvider.notifier).openOrFocusSession(sessionName);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final focusedId = ref.read(facetManagerProvider).focusedId;
      if (focusedId != null) {
        TerminalRegistry.instance.focusTerminal(focusedId);
      }
    });
  }

  Future<void> _deleteSession(String sessionName) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete Session'),
        content: Text('Delete session "$sessionName"?'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirmed == true && mounted) {
      try {
        await ref
            .read(sessionServiceProvider.notifier)
            .deleteSession(sessionName);
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text('Failed to delete session: $e')),
          );
        }
      }
    }
  }

  Future<void> _addAbotToKubo(String kubo) async {
    final name = await _showNameDialog(
        title: 'New Abot in $kubo', hint: 'abot name');
    if (name == null || name.isEmpty || !mounted) return;
    await _createAbotSession(name, kubo);
  }

  Future<void> _createAbotSession(String abotName, String kuboName) async {
    try {
      await ref.read(facetManagerProvider.notifier).createAbotInKubo(
        abotName,
        kubo: kuboName,
      );
      if (!mounted) return;
      ref.invalidate(kuboServiceProvider);
      ref.invalidate(abotServiceProvider);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to start abot: $e')),
      );
    }
  }

  Future<void> _createNewKubo() async {
    final name = await _showNameDialog(title: 'New Kubo', hint: 'kubo name');
    if (name == null || name.isEmpty || !mounted) return;
    try {
      await ref.read(kuboServiceProvider.notifier).createKubo(name);
      if (!mounted) return;
      ref.invalidate(kuboServiceProvider);
      ref.read(workspaceProvider.notifier).openKubo(name);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to create kubo: $e')),
      );
    }
  }

  Future<void> _openBundle() async {
    final activeKubo = ref.read(workspaceProvider).activeKubo;
    if (activeKubo == null) return;
    await _openBundleInKubo(activeKubo);
  }

  Future<void> _openBundleInKubo(String kubo) async {
    try {
      final data = await const ApiClient().post('/api/pick-file', {})
          as Map<String, dynamic>;
      final path = data['path'] as String?;
      if (path == null || path.isEmpty || !mounted) return;

      final result = await ref
          .read(sessionServiceProvider.notifier)
          .openBundle(path, kubo: kubo);
      final sessionName = result['name'] as String?;
      if (sessionName != null && mounted) {
        ref.read(facetManagerProvider.notifier).openOrFocusSession(sessionName);
      }
      if (!mounted) return;
      ref.invalidate(kuboServiceProvider);
      ref.invalidate(abotServiceProvider);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Open failed: $e')),
      );
    }
  }

  Future<void> _openKuboFromDisk() async {
    try {
      final data = await const ApiClient().post('/api/pick-directory', {})
          as Map<String, dynamic>;
      final path = data['path'] as String?;
      if (path == null || path.isEmpty || !mounted) return;

      final result = await ref
          .read(kuboServiceProvider.notifier)
          .openKubo(path);
      if (!mounted) return;

      final name = result['name'] as String?;
      if (name != null && name.isNotEmpty) {
        ref.read(workspaceProvider.notifier).openKubo(name);
      }
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Open kubo failed: $e')),
      );
    }
  }

  Future<void> _removeAbotFromKubo(String kuboName, String abotName) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) {
        final p = ctx.palette;
        return AlertDialog(
          backgroundColor: p.base,
          title: Text('Remove abot',
              style: TextStyle(
                  color: p.text, fontFamily: AbotFonts.mono, fontSize: 14)),
          content: Text('Remove "$abotName" from $kuboName?',
              style: TextStyle(
                  color: p.subtext0, fontFamily: AbotFonts.mono, fontSize: 12)),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text('Cancel',
                  style: TextStyle(
                      color: p.subtext0, fontFamily: AbotFonts.mono)),
            ),
            TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text('Remove',
                  style: TextStyle(color: p.red, fontFamily: AbotFonts.mono)),
            ),
          ],
        );
      },
    );
    if (confirmed != true || !mounted) return;
    try {
      // Minimize facet if open (sessionName is qualified: abot@kubo)
      final qualified = '$abotName@$kuboName';
      final state = ref.read(facetManagerProvider);
      for (final facet in state.facets.values.toList()) {
        if (facet.sessionName == qualified) {
          TerminalRegistry.instance.clearGenieTransform(facet.id, animate: false);
          ref.read(facetManagerProvider.notifier).minimizeSession(facet.id);
        }
      }

      await ref
          .read(kuboServiceProvider.notifier)
          .removeAbotFromKubo(kuboName, abotName);
      if (!mounted) return;
      ref.invalidate(sessionServiceProvider);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to remove abot: $e')),
      );
    }
  }

  Future<String?> _showNameDialog({required String title, required String hint}) {
    final controller = TextEditingController();
    return showDialog<String>(
      context: context,
      builder: (ctx) {
        final p = ctx.palette;
        return AlertDialog(
          backgroundColor: p.base,
          title: Text(title,
              style: TextStyle(
                  color: p.text, fontFamily: AbotFonts.mono, fontSize: 14)),
          content: TextField(
            controller: controller,
            autofocus: true,
            style: TextStyle(
                color: p.text, fontFamily: AbotFonts.mono, fontSize: 13),
            decoration: InputDecoration(
              hintText: hint,
              hintStyle: TextStyle(color: p.overlay0, fontFamily: AbotFonts.mono),
              enabledBorder: UnderlineInputBorder(
                  borderSide: BorderSide(color: p.surface1)),
              focusedBorder: UnderlineInputBorder(
                  borderSide: BorderSide(color: p.mauve)),
            ),
            onSubmitted: (v) => Navigator.pop(ctx, v.trim()),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx),
              child: Text('Cancel',
                  style:
                      TextStyle(color: p.subtext0, fontFamily: AbotFonts.mono)),
            ),
            TextButton(
              onPressed: () => Navigator.pop(ctx, controller.text.trim()),
              child: Text('Create',
                  style:
                      TextStyle(color: p.mauve, fontFamily: AbotFonts.mono)),
            ),
          ],
        );
      },
    ).whenComplete(() => controller.dispose());
  }

  /// Return abot names from the kubo manifest that have no sessions (neither open nor unattached).
  List<String> _manifestOnlyAbots(
      String kuboName, _KuboGroup group, List<KuboInfo> kubos,
      Map<String, SessionInfo> sessionInfoMap) {
    final kuboInfo = kubos.where((k) => k.name == kuboName).firstOrNull;
    if (kuboInfo == null) return [];
    // Use displayName (bare abot name) to compare with manifest abots
    final sessionDisplayNames = <String>{
      ...group.facets.map((f) => sessionInfoMap[f.sessionName]?.displayName ?? f.sessionName),
      ...group.unattachedSessions.map((s) => s.displayName),
    };
    return kuboInfo.abots.where((a) => !sessionDisplayNames.contains(a)).toList();
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    // Watch all data providers directly — no prop-drilling.
    final facetState = ref.watch(facetManagerProvider);
    final sessionsAsync = ref.watch(sessionServiceProvider);
    final wsState = ref.watch(wsServiceProvider);
    final kubosAsync = ref.watch(kuboServiceProvider);
    final abotsAsync = ref.watch(abotServiceProvider);
    final sidebar = ref.watch(sidebarProvider);
    final workspace = ref.watch(workspaceProvider);

    final serverSessions = sessionsAsync.when(
      data: (list) => list,
      loading: () => <SessionInfo>[],
      error: (_, _) => <SessionInfo>[],
    );
    final allFacets = facetState.order
        .map((id) => facetState.facets[id])
        .whereType<FacetData>()
        .toList();
    final focusedId = facetState.focusedId;
    final openSessionNames = facetState.facets.values
        .map((f) => f.sessionName)
        .toSet();
    final sessionInfoMap = {for (final s in serverSessions) s.name: s};
    final kubos = kubosAsync.when(
      data: (list) => list.where((k) => workspace.openKubos.contains(k.name)).toList(),
      loading: () => <KuboInfo>[],
      error: (_, _) => <KuboInfo>[],
    );
    final collapsed = sidebar.collapsed;
    final activeKubo = workspace.activeKubo;
    final knownAbots = abotsAsync.when(
      data: (list) => list,
      loading: () => <AbotInfo>[],
      error: (_, _) => <AbotInfo>[],
    );
    final activeTab = sidebar.tab;
    final connectionState = wsState.connectionState;

    return AnimatedContainer(
      duration: AbotSizes.sidebarAnimDuration,
      curve: Curves.easeInOut,
      width: collapsed
          ? AbotSizes.sidebarCollapsedWidth
          : AbotSizes.sidebarExpandedWidth,
      color: p.mantle,
      child: Column(
        children: [
          _buildTopBar(p, collapsed, activeTab),
          if (collapsed)
            const Spacer()
          else ...[
            // Tab bar
            _buildTabBar(p, activeTab),
            // Tab content
            Expanded(
              child: NotificationListener<ScrollNotification>(
                onNotification: (notification) {
                  widget.onScroll?.call();
                  return false;
                },
                child: activeTab == SidebarTab.kubos
                    ? _buildKubosTab(p, kubos, allFacets, serverSessions,
                        openSessionNames, sessionInfoMap, focusedId, activeKubo)
                    : _buildAbotsTab(p, knownAbots, serverSessions,
                        allFacets, sessionInfoMap),
              ),
            ),
          ],
          _SidebarFooter(
            connectionState: connectionState,
            onSettingsTap: () =>
                ref.read(overlayProvider.notifier).toggleSettings(),
            collapsed: collapsed,
          ),
        ],
      ),
    );
  }

  Widget _buildTopBar(CatPalette p, bool collapsed, SidebarTab activeTab) {
    return Padding(
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
                  onTap: widget.onToggleCollapse,
                  tooltip: 'Expand sidebar',
                ),
                const SizedBox(height: AbotSpacing.xs),
                _IconBtn(
                  icon: Icons.add,
                  color: p.subtext0,
                  onTap: activeTab == SidebarTab.kubos
                      ? _createNewKubo
                      : () {
                          final ak = ref.read(workspaceProvider).activeKubo;
                          if (ak != null) _addAbotToKubo(ak);
                        },
                  tooltip: activeTab == SidebarTab.kubos
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
                if (activeTab == SidebarTab.kubos)
                  _IconBtn(
                    icon: Icons.folder_open_outlined,
                    color: p.subtext0,
                    onTap: _openKuboFromDisk,
                    tooltip: 'Open kubo',
                  ),
                _IconBtn(
                  icon: Icons.add,
                  color: p.subtext0,
                  onTap: activeTab == SidebarTab.kubos
                      ? _createNewKubo
                      : () {
                          final ak = ref.read(workspaceProvider).activeKubo;
                          if (ak != null) _addAbotToKubo(ak);
                        },
                  tooltip: activeTab == SidebarTab.kubos
                      ? 'New kubo'
                      : 'New abot',
                ),
              ],
            ),
    );
  }

  Widget _buildTabBar(CatPalette p, SidebarTab activeTab) {
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
              isActive: activeTab == SidebarTab.kubos,
              onTap: () {
                ref.read(sidebarProvider.notifier).setTab(SidebarTab.kubos);
                widget.onScroll?.call();
              },
            ),
            _TabButton(
              label: 'Abots',
              isActive: activeTab == SidebarTab.abots,
              onTap: () {
                ref.read(sidebarProvider.notifier).setTab(SidebarTab.abots);
                widget.onScroll?.call();
              },
            ),
          ],
        ),
      ),
    );
  }

  // ── Kubos tab ────────────────────────────────────────────────────────

  Widget _buildKubosTab(
      CatPalette p,
      List<KuboInfo> kubos,
      List<FacetData> allFacets,
      List<SessionInfo> serverSessions,
      Set<String> openSessionNames,
      Map<String, SessionInfo> sessionInfoMap,
      String? focusedId,
      String? activeKubo) {
    final groups = <String, _KuboGroup>{};
    for (final k in kubos) {
      groups.putIfAbsent(k.name, () => _KuboGroup(kuboName: k.name));
    }

    // Assign all sessions (open and unattached) to groups
    for (final facet in allFacets) {
      final kubo = sessionInfoMap[facet.sessionName]?.kubo;
      if (kubo != null) {
        groups.putIfAbsent(kubo, () => _KuboGroup(kuboName: kubo));
        groups[kubo]!.facets.add(facet);
      }
    }
    for (final session in serverSessions) {
      if (openSessionNames.contains(session.name)) continue;
      final kubo = session.kubo;
      if (kubo != null) {
        groups.putIfAbsent(kubo, () => _KuboGroup(kuboName: kubo));
        groups[kubo]!.unattachedSessions.add(session);
      }
    }

    final sortedKeys = groups.keys.toList()..sort();

    final kuboRunning = <String, bool>{};
    for (final k in kubos) {
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
          _buildKuboSection(p, kuboName, groups[kuboName]!,
              kuboRunning[kuboName] ?? false, kubos, sessionInfoMap,
              focusedId, activeKubo),
      ],
    );
  }

  Widget _buildKuboSection(
      CatPalette p, String kuboName, _KuboGroup group, bool running,
      List<KuboInfo> kubos, Map<String, SessionInfo> sessionInfoMap,
      String? focusedId, String? activeKubo) {
    final isCollapsed = _collapsedKubos.contains(kuboName);
    final isActive = activeKubo == kuboName;
    final manifestOnly = _manifestOnlyAbots(kuboName, group, kubos, sessionInfoMap);

    return SliverPadding(
      padding: const EdgeInsets.symmetric(horizontal: AbotSpacing.sm),
      sliver: SliverList(
        delegate: SliverChildListDelegate([
          GestureDetector(
            onTap: () => ref.read(workspaceProvider.notifier).setActiveKubo(kuboName),
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
                      name: sessionInfoMap[facet.sessionName]?.displayName ?? facet.sessionName,
                      isRunning: sessionInfoMap[facet.sessionName]?.isRunning ?? true,
                      isFocused: facet.id == focusedId,
                      isDirty: sessionInfoMap[facet.sessionName]?.dirty ?? false,
                      onTap: facet.id == focusedId
                          ? null
                          : () => _focusFacet(facet.id),
                      onRemove: () => _removeAbotFromKubo(kuboName, sessionInfoMap[facet.sessionName]?.displayName ?? facet.sessionName),
                    ),
                  // Unattached abots (server sessions not open as facets)
                  for (final session in group.unattachedSessions)
                    _AbotRow(
                      name: session.displayName,
                      isRunning: session.isRunning,
                      isFocused: false,
                      isDirty: session.dirty,
                      onTap: () => _openSession(session.name),
                      onRemove: () => _removeAbotFromKubo(kuboName, session.displayName),
                    ),
                  // Abots from manifest that have no sessions at all
                  for (final abotName in manifestOnly)
                    _AbotRow(
                      name: abotName,
                      isRunning: false,
                      isFocused: false,
                      onTap: () => _createAbotSession(abotName, kuboName),
                      onRemove: () => _removeAbotFromKubo(kuboName, abotName),
                    ),
                  if (group.facets.isEmpty && group.unattachedSessions.isEmpty && manifestOnly.isEmpty)
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
                    onAdd: () => _addAbotToKubo(kuboName),
                    onOpen: () => _openBundleInKubo(kuboName),
                    onSettings: () =>
                        ref.read(overlayProvider.notifier).showKuboSettings(kuboName),
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

  Widget _buildAbotsTab(CatPalette p, List<AbotInfo> knownAbots,
      List<SessionInfo> serverSessions, List<FacetData> allFacets,
      Map<String, SessionInfo> sessionInfoMap) {
    // Build a set of bare abot names that have active sessions
    final activeAbotNames = <String>{};
    for (final session in serverSessions) {
      activeAbotNames.add(session.displayName);
    }
    for (final facet in allFacets) {
      final info = sessionInfoMap[facet.sessionName];
      activeAbotNames.add(info?.displayName ?? facet.sessionName);
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
                  onTapDetail: () =>
                      ref.read(overlayProvider.notifier).showAbotDetail(abot.name),
                ),
              ),
              if (!isCollapsed) ...[
                // Active kubo branches (with worktrees)
                for (final branch in activeBranches)
                  _KuboBranchRow(
                    kuboName: branch.kuboName,
                    hasSession: branch.hasSession,
                    isActive: true,
                    onTap: () {
                      ref.read(workspaceProvider.notifier).setActiveKubo(branch.kuboName);
                      _createAbotSession(abot.name, branch.kuboName);
                    },
                    onDismiss: () =>
                        ref.read(abotServiceProvider.notifier).dismissVariant(abot.name, branch.kuboName),
                  ),
                // Past kubo branches (no worktree)
                for (final branch in pastBranches)
                  _KuboBranchRow(
                    kuboName: branch.kuboName,
                    hasSession: false,
                    isActive: false,
                    onIntegrate: () =>
                        ref.read(abotServiceProvider.notifier).integrateVariant(abot.name, branch.kuboName),
                    onDiscard: () =>
                        ref.read(abotServiceProvider.notifier).discardVariant(abot.name, branch.kuboName),
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
    // Dot color: green if session active, grey if worktree only or past
    final dotColor = widget.hasSession
        ? p.green
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
  final bool collapsed;

  const _SidebarFooter({
    required this.connectionState,
    this.onSettingsTap,
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
                      widget.session.displayName,
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
