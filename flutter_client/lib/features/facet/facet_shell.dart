import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:web/web.dart' as web;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/network/kubo_service.dart';
import '../../core/network/session_service.dart';
import '../../core/network/websocket_service.dart';
import '../../core/network/ws_messages.dart';
import '../../core/theme/abot_theme.dart';
import '../terminal/terminal_facet.dart';
import 'facet.dart';
import 'facet_manager.dart';
import 'stage_strip.dart';
import '../../core/network/api_client.dart';
import '../settings/settings_panel.dart';
import '../settings/session_settings_panel.dart';

const double _narrowBreakpoint = 768;
const String _offscreenTransform = 'translate(-9999px, 0) scale(0.01, 0.01)';

/// The main app shell — iPad Stage Manager-style layout: one focused facet
/// takes center stage while others appear as live-preview cards in a side strip.
class FacetShell extends ConsumerStatefulWidget {
  const FacetShell({super.key});

  @override
  ConsumerState<FacetShell> createState() => _FacetShellState();
}

class _FacetShellState extends ConsumerState<FacetShell>
    with WidgetsBindingObserver {
  /// GlobalKeys per facet — ensures Flutter reuses the same State when a facet
  /// moves between focused/offstage, preserving the HtmlElementView (xterm).
  final Map<String, GlobalKey> _facetKeys = {};

  /// Card position keys for CSS transform calculation.
  final Map<String, GlobalKey> _cardKeys = {};
  final GlobalKey _mainAreaKey = GlobalKey();

  /// Timer for post-sidebar-toggle cleanup (cancelled on rapid re-toggle).
  Timer? _animationCleanup;

  /// DOM label overlays positioned above the CSS-transformed terminal layer.
  final Map<String, web.HTMLElement> _labelOverlays = {};

  /// Subscription from ref.listenManual — cancelled in dispose.
  ProviderSubscription? _wsSubscription;

  /// Whether the sidebar is collapsed to a thin sliver.
  bool _sidebarCollapsed = false;

  /// Active sidebar tab — controls whether terminal previews render.
  SidebarTab _sidebarTab = SidebarTab.abots;

  /// Whether the settings panel overlay is visible.
  bool _showSettings = false;

  /// Session name for per-session settings overlay (null = hidden).
  String? _sessionSettingsName;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);

    // When a terminal finishes initializing, re-apply sidebar transforms.
    TerminalRegistry.instance.onRegistered = _onTerminalReady;

    WidgetsBinding.instance.addPostFrameCallback((_) {
      _initialize();
    });
  }

  void _onTerminalReady() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _updateSidebarTransforms();
    });
  }

  Future<void> _initialize() async {
    final facetManager = ref.read(facetManagerProvider.notifier);
    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.onMessage = _handleServerMessage;

    // Fetch existing sessions from the server and restore facets for them.
    // On first launch 'main' won't exist yet, so we create it as fallback.
    try {
      final sessions = await ref
          .read(sessionServiceProvider.notifier)
          .listSessions();
      if (!mounted) return;
      final running = sessions.where((s) => s.isRunning).toList();

      if (running.isNotEmpty) {
        // Create facets for all running sessions. Focus 'main' if it exists,
        // otherwise focus the first one.
        final mainFirst = <SessionInfo>[
          ...running.where((s) => s.name == 'main'),
          ...running.where((s) => s.name != 'main'),
        ];
        for (final s in mainFirst) {
          facetManager.create(s.name);
          // Bump session counter past existing session-N names
          final match = RegExp(r'^session-(\d+)$').firstMatch(s.name);
          if (match != null) {
            facetManager.bumpSessionCounter(int.parse(match.group(1)!) + 1);
          }
        }
        // Restore persisted sidebar order, then focus 'main'.
        facetManager.loadPersistedOrder();
        final mainFacet = ref.read(facetManagerProvider).getBySession('main');
        if (mainFacet != null) {
          facetManager.focus(mainFacet.id);
        }
      } else {
        // No sessions on server — create 'main'
        facetManager.create('main');
      }
    } catch (e) {
      debugPrint('[FacetShell] Failed to fetch sessions: $e');
      // Server unreachable — create 'main' optimistically
      facetManager.create('main');
    }

    if (!mounted) return;
    wsService.connect();

    _wsSubscription = ref.listenManual(wsServiceProvider, (prev, next) {
      if (!mounted) return;
      if (prev?.connectionState != WsConnectionState.connected &&
          next.connectionState == WsConnectionState.connected) {
        final facets = ref.read(facetManagerProvider).orderedFacets;
        for (final facet in facets) {
          wsService.attachSession(facet.sessionName);
        }
        ref.read(sessionServiceProvider.notifier).refresh();
        ref.read(kuboServiceProvider.notifier).refresh();
      }
    });
  }

  void _handleServerMessage(ServerMessage msg) {
    switch (msg) {
      case AttachedMessage(:final session, :final buffer):
        TerminalRegistry.instance.resetSession(session);
        if (buffer.isNotEmpty) {
          TerminalRegistry.instance.writeToSession(session, buffer);
        }

      case OutputMessage(:final data, :final session):
        if (session != null) {
          TerminalRegistry.instance.writeToSession(session, data);
        } else {
          TerminalRegistry.instance.writeToAll(data);
        }

      case ExitMessage(:final session):
        final sessionName = session ?? 'main';
        TerminalRegistry.instance
            .writeToSession(sessionName, '\r\n[shell exited]\r\n');

      case SessionRemovedMessage(:final session):
        TerminalRegistry.instance
            .writeToSession(session, '\r\n[session deleted]\r\n');

      case ErrorMessage(:final message):
        debugPrint('[WS Error] $message');

      case P2pSignalMessage():
      case P2pReadyMessage():
      case P2pClosedMessage():
        break;

      case ServerDrainingMessage():
      case ReloadMessage():
        break;

      case UnknownMessage(:final type):
        debugPrint('[WS] Unknown message type: $type');
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.onVisibilityChange(state != AppLifecycleState.resumed);
  }

  // --- Sidebar collapse ---

  void _toggleSidebar() {
    setState(() => _sidebarCollapsed = !_sidebarCollapsed);
    if (_sidebarCollapsed) {
      _hideOffstageTerminals();
    } else {
      // Wait for animation + layout settle, then recalculate transforms
      _animationCleanup?.cancel();
      // Wait for sidebar AnimatedContainer to finish + layout settle
      _animationCleanup = Timer(AbotSizes.sidebarAnimDuration + const Duration(milliseconds: 50), () {
        if (!mounted) return;
        _updateSidebarTransforms();
      });
    }
  }

  /// Move all non-focused terminals (and the mirror) offscreen via CSS transform.
  /// They stay in the widget tree (GlobalKey preserves xterm.js state).
  void _hideOffstageTerminals() {
    final state = ref.read(facetManagerProvider);
    for (final id in state.stripOrder) {
      TerminalRegistry.instance.setGenieTransform(
        id,
        _offscreenTransform,
        animate: false,
      );
    }
    // Also hide the mirror of the focused terminal
    if (state.focusedId != null) {
      TerminalRegistry.instance.setGenieTransform(
        '${state.focusedId!}_mirror',
        _offscreenTransform,
        animate: false,
      );
    }
    _removeAllLabelOverlays();
  }

  // --- Facet lifecycle ---

  Future<void> _createNewFacet({String kubo = 'default'}) async {
    await ref.read(facetManagerProvider.notifier).createNewSession(kubo: kubo);
    if (!mounted) return;
    // Refresh kubo list (active session count may have changed)
    ref.read(kuboServiceProvider.notifier).refresh();
  }

  /// Show a dialog to create a new kubo, then optionally create a session in it.
  Future<void> _createNewKubo() async {
    final name = await _showNewKuboDialog();
    if (name == null || name.isEmpty || !mounted) return;
    try {
      await ref.read(kuboServiceProvider.notifier).createKubo(name);
      if (!mounted) return;
      // Create a session inside the new kubo
      await _createNewFacet(kubo: name);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to create kubo: $e')),
      );
    }
  }

  /// Show a dialog to name a new abot, then add it to a kubo and open a session.
  Future<void> _addAbotToKubo(String kubo) async {
    final name = await _showNewAbotDialog(kubo);
    if (name == null || name.isEmpty || !mounted) return;
    try {
      await ref.read(facetManagerProvider.notifier).createAbotInKubo(name, kubo: kubo);
      if (!mounted) return;
      ref.read(kuboServiceProvider.notifier).refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to add abot: $e')),
      );
    }
  }

  Future<String?> _showNewAbotDialog(String kuboName) {
    final controller = TextEditingController();
    return showDialog<String>(
      context: context,
      builder: (ctx) {
        final p = ctx.palette;
        return AlertDialog(
          backgroundColor: p.base,
          title: Text('New Abot in $kuboName',
              style: TextStyle(
                  color: p.text, fontFamily: AbotFonts.mono, fontSize: 14)),
          content: TextField(
            controller: controller,
            autofocus: true,
            style: TextStyle(
                color: p.text, fontFamily: AbotFonts.mono, fontSize: 13),
            decoration: InputDecoration(
              hintText: 'abot name',
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

  Future<String?> _showNewKuboDialog() {
    final controller = TextEditingController();
    return showDialog<String>(
      context: context,
      builder: (ctx) {
        final p = ctx.palette;
        return AlertDialog(
          backgroundColor: p.base,
          title: Text('New Kubo',
              style: TextStyle(
                  color: p.text, fontFamily: AbotFonts.mono, fontSize: 14)),
          content: TextField(
            controller: controller,
            autofocus: true,
            style: TextStyle(
                color: p.text, fontFamily: AbotFonts.mono, fontSize: 13),
            decoration: InputDecoration(
              hintText: 'kubo name',
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

  void _minimizeFacet(String facetId) {
    TerminalRegistry.instance.clearGenieTransform(facetId, animate: false);
    ref.read(facetManagerProvider.notifier).minimizeSession(facetId);
    _facetKeys.remove(facetId);
    _cardKeys.remove(facetId);
  }

  Future<void> _closeFacet(String facetId) async {
    final facet = ref.read(facetManagerProvider).facets[facetId];
    if (facet == null) return;

    final sessionName = facet.sessionName;
    final isDirty = ref.read(sessionServiceProvider).when(
          data: (sessions) =>
              sessions.where((s) => s.name == sessionName).firstOrNull?.dirty ??
              false,
          loading: () => false,
          error: (_, _) => false,
        );

    bool save = false;
    if (isDirty) {
      final action = await showDialog<String>(
        context: context,
        builder: (ctx) {
          final p = ctx.palette;
          return AlertDialog(
            backgroundColor: p.base,
            title: Text('Unsaved changes',
                style: TextStyle(
                    color: p.text,
                    fontFamily: AbotFonts.mono,
                    fontSize: 14)),
            content: Text('Save "$sessionName" before closing?',
                style: TextStyle(
                    color: p.subtext0,
                    fontFamily: AbotFonts.mono,
                    fontSize: 12)),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(ctx, 'discard'),
                child: Text('Discard',
                    style: TextStyle(color: p.red, fontFamily: AbotFonts.mono)),
              ),
              TextButton(
                onPressed: () => Navigator.pop(ctx),
                child: Text('Cancel',
                    style: TextStyle(
                        color: p.subtext0, fontFamily: AbotFonts.mono)),
              ),
              TextButton(
                onPressed: () => Navigator.pop(ctx, 'save'),
                child: Text('Save & Close',
                    style:
                        TextStyle(color: p.mauve, fontFamily: AbotFonts.mono)),
              ),
            ],
          );
        },
      );
      if (!mounted || action == null) return;
      save = action == 'save';
    }

    try {
      await ref
          .read(sessionServiceProvider.notifier)
          .closeSession(sessionName, save: save);
      if (!mounted) return;
      _minimizeFacet(facetId);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Close failed: $e')),
      );
    }
  }

  void _focusFacet(String facetId) {
    final currentFocused = ref.read(facetManagerProvider).focusedId;
    if (facetId == currentFocused) return;

    // Instant swap — change focus and recompute transforms without animation.
    ref.read(facetManagerProvider.notifier).focus(facetId);
  }

  /// Open or focus a server session from the strip.
  void _onOpenSession(String sessionName) {
    ref.read(facetManagerProvider.notifier).openOrFocusSession(sessionName);
  }

  /// Delete a server session (with confirmation).
  Future<void> _onDeleteSession(String sessionName) async {
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

  /// Open a .abot bundle via native OS file picker.
  Future<void> _openBundle() async {
    try {
      final data = await const ApiClient().post('/api/pick-file', {})
          as Map<String, dynamic>;
      final path = data['path'] as String?;
      if (path == null || path.isEmpty || !mounted) return;

      final result = await ref.read(sessionServiceProvider.notifier).openBundle(path);
      if (!mounted) return;
      // Open the session as a focused facet
      final name = result['name'] as String?;
      if (name != null) {
        ref.read(facetManagerProvider.notifier).openOrFocusSession(name);
      }
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Open failed: $e')),
      );
    }
  }

  /// Cycle focus to the next facet in order.
  void _cycleFocus() {
    final state = ref.read(facetManagerProvider);
    if (state.order.length <= 1) return;
    final idx = state.order.indexOf(state.focusedId ?? '');
    _focusFacet(state.order[(idx + 1) % state.order.length]);
  }

  /// Toggle search on the focused terminal.
  void _toggleSearch() {
    final focusedId = ref.read(facetManagerProvider).focusedId;
    if (focusedId != null) {
      TerminalRegistry.instance.toggleSearchOnFacet(focusedId);
    }
  }

  // --- CSS transform positioning ---

  Rect? _getRectForKey(GlobalKey key) {
    final renderBox = key.currentContext?.findRenderObject() as RenderBox?;
    if (renderBox == null || !renderBox.hasSize) return null;
    final position = renderBox.localToGlobal(Offset.zero);
    return position & renderBox.size;
  }

  void _ensureCardKey(String facetId) {
    _cardKeys.putIfAbsent(facetId, () => GlobalKey());
  }

  void _ensureFacetKey(String facetId) {
    _facetKeys.putIfAbsent(facetId, () => GlobalKey());
  }

  /// Compute and apply CSS transforms for all non-focused terminals so they
  /// appear at their sidebar card positions. Called after each layout.
  void _updateSidebarTransforms() {
    if (_sidebarCollapsed || _sidebarTab == SidebarTab.kubos) {
      _hideOffstageTerminals();
      return;
    }

    final state = ref.read(facetManagerProvider);
    final mainRect = _getRectForKey(_mainAreaKey);
    if (mainRect == null || mainRect.width == 0 || mainRect.height == 0) return;

    // Helper: apply cursor-aware CSS transform to position a terminal at a card.
    // [contentFraction] overrides the terminal's own fraction (used for mirrors
    // so they match the main terminal's cursor position, not their own).
    void applyCardTransform(String terminalId, Rect cardRect,
        {double? contentFraction}) {
      final s = cardRect.width / mainRect.width;
      final frac = contentFraction ??
          TerminalRegistry.instance.contentFraction(terminalId);

      // The xterm container sits below a 32px title bar SizedBox in the
      // Column layout. Its natural screen position is (mainRect.left,
      // mainRect.top + titleBarH). The clip-path operates in the container's
      // local coordinates which only contain xterm content, not the title bar.
      final titleBarH = AbotSizes.titleBarHeight;
      final xtermH = mainRect.height - titleBarH;

      // How much xterm content overflows the card (in source pixels).
      final overflow =
          (xtermH * frac - cardRect.height / s).clamp(0.0, double.infinity);

      final tx = cardRect.left - mainRect.left;
      // The container is naturally 32px below mainRect.top (Column layout).
      // titleBarH is a fixed pixel offset (not scaled).
      final ty = cardRect.top - mainRect.top - titleBarH - overflow * s;

      // Clip within the xterm container only (no title bar to skip).
      final topClip = overflow;
      final bottomClip =
          (xtermH - overflow - cardRect.height / s).clamp(0.0, double.infinity);

      // Inset by card border width + border-radius so the card border
      // and rounded corners remain visible beneath the terminal content.
      final borderInset = 2.0 / s; // card border width headroom
      final clipRadius = AbotRadius.md / s;

      TerminalRegistry.instance.setGenieTransform(
        terminalId,
        'translate(${tx}px, ${ty}px) scale($s)',
        animate: false,
        clipPath: 'inset(${topClip + borderInset}px ${borderInset}px '
            '${bottomClip + borderInset}px ${borderInset}px '
            'round ${clipRadius}px)',
      );
    }

    // Transform unfocused terminals to their sidebar card positions.
    for (final id in state.stripOrder) {
      _ensureCardKey(id);
      final cardRect = _getRectForKey(_cardKeys[id]!);
      if (cardRect == null) continue;
      applyCardTransform(id, cardRect);
    }

    // Focused terminal: clear CSS transform (renders full-size in main area).
    // Its mirror is CSS-transformed to the focused sidebar card instead.
    if (state.focusedId != null) {
      TerminalRegistry.instance
          .clearGenieTransform(state.focusedId!, animate: false);

      if (state.count > 1) {
        _ensureCardKey(state.focusedId!);
        final cardRect = _getRectForKey(_cardKeys[state.focusedId!]!);
        if (cardRect != null) {
          // Use the main terminal's contentFraction so the mirror's crop
          // matches the real cursor position, not the mirror's own cursor.
          final mainFrac = TerminalRegistry.instance
              .contentFraction(state.focusedId!);
          applyCardTransform('${state.focusedId!}_mirror', cardRect,
              contentFraction: mainFrac);
        }
      }
    }

    // Update DOM label overlays for all cards (including focused).
    final activeIds = <String>{};
    for (final id in state.stripOrder) {
      final cardRect = _getRectForKey(_cardKeys[id]!);
      if (cardRect == null) continue;
      final name = state.facets[id]?.sessionName ?? id;
      activeIds.add(id);
      _upsertLabelOverlay(id, cardRect, name, isFocused: false);
    }
    if (state.focusedId != null && state.count > 1) {
      final id = state.focusedId!;
      _ensureCardKey(id);
      final cardRect = _getRectForKey(_cardKeys[id]!);
      if (cardRect != null) {
        final name = state.facets[id]?.sessionName ?? id;
        activeIds.add(id);
        _upsertLabelOverlay(id, cardRect, name, isFocused: true);
      }
    }
    // Remove stale labels.
    _labelOverlays.keys
        .where((id) => !activeIds.contains(id))
        .toList()
        .forEach(_removeLabelOverlay);
  }

  void _upsertLabelOverlay(String id, Rect cardRect, String name,
      {bool isFocused = false}) {
    var el = _labelOverlays[id];
    if (el == null) {
      el = web.document.createElement('div') as web.HTMLDivElement;
      el.style
        ..position = 'fixed'
        ..pointerEvents = 'none'
        ..fontFamily = '${AbotFonts.mono}, monospace'
        ..fontSize = '9px'
        ..padding = '2px 4px'
        ..borderRadius = '3px'
        ..zIndex = '10000';
      web.document.body!.append(el);
      _labelOverlays[id] = el;
    }
    el.textContent = name;
    el.style
      ..color = isFocused ? 'rgba(203, 166, 247, 0.95)' : 'rgba(180, 180, 200, 0.8)'
      ..backgroundColor = isFocused ? 'rgba(30, 30, 46, 0.8)' : 'rgba(0, 0, 0, 0.5)'
      ..left = '${cardRect.right - 6}px'
      ..top = '${cardRect.bottom - 6}px'
      ..transform = 'translate(-100%, -100%)';
  }

  void _removeLabelOverlay(String id) {
    _labelOverlays.remove(id)?.remove();
  }

  void _removeAllLabelOverlays() {
    for (final el in _labelOverlays.values) {
      el.remove();
    }
    _labelOverlays.clear();
  }

  @override
  void dispose() {
    _animationCleanup?.cancel();
    _wsSubscription?.close();
    _removeAllLabelOverlays();
    TerminalRegistry.instance.onRegistered = null;
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final facetState = ref.watch(facetManagerProvider);
    final sessionsAsync = ref.watch(sessionServiceProvider);
    final wsState = ref.watch(wsServiceProvider);
    final kubosAsync = ref.watch(kuboServiceProvider);

    return Scaffold(
      backgroundColor: context.palette.base,
      body: Stack(
        children: [
          CallbackShortcuts(
            bindings: {
              // Ctrl+` — cycle focus
              const SingleActivator(LogicalKeyboardKey.backquote,
                  control: true): _cycleFocus,
              // Ctrl+Tab — cycle focus (alias)
              const SingleActivator(LogicalKeyboardKey.tab,
                  control: true): _cycleFocus,
              // Ctrl+N — new session
              SingleActivator(LogicalKeyboardKey.keyN,
                  control: defaultTargetPlatform != TargetPlatform.macOS,
                  meta: defaultTargetPlatform == TargetPlatform.macOS): () {
                _createNewFacet();
              },
              // Ctrl+W — minimize current facet (detach, keep session alive)
              SingleActivator(LogicalKeyboardKey.keyW,
                  control: defaultTargetPlatform != TargetPlatform.macOS,
                  meta: defaultTargetPlatform == TargetPlatform.macOS): () {
                final state = ref.read(facetManagerProvider);
                if (state.focusedId != null) {
                  final facet = state.facets[state.focusedId!];
                  if (facet != null) {
                    _minimizeFacet(facet.id);
                  }
                }
              },
              // Ctrl+Shift+F / Cmd+Shift+F — toggle search
              SingleActivator(LogicalKeyboardKey.keyF,
                  control: defaultTargetPlatform != TargetPlatform.macOS,
                  meta: defaultTargetPlatform == TargetPlatform.macOS,
                  shift: true): () {
                _toggleSearch();
              },
              // Ctrl+B / Cmd+B — toggle sidebar
              SingleActivator(LogicalKeyboardKey.keyB,
                  control: defaultTargetPlatform != TargetPlatform.macOS,
                  meta: defaultTargetPlatform == TargetPlatform.macOS):
                  _toggleSidebar,
            },
            child: Focus(
              autofocus: true,
              child: _buildFacetLayout(facetState, sessionsAsync, wsState, kubosAsync),
            ),
          ),
          if (_showSettings)
            SettingsPanel(
              onClose: () => setState(() => _showSettings = false),
            ),
          if (_sessionSettingsName != null)
            SessionSettingsPanel(
              sessionName: _sessionSettingsName!,
              onClose: () =>
                  setState(() => _sessionSettingsName = null),
              onRenamed: (newName) {
                final oldName = _sessionSettingsName!;
                setState(() => _sessionSettingsName = newName);
                // Update facet data so terminal I/O uses the new name
                ref
                    .read(facetManagerProvider.notifier)
                    .renameSessionInFacets(oldName, newName);
                // Refresh session list to pick up the new name
                ref.read(sessionServiceProvider.notifier).refresh();
              },
            ),
        ],
      ),
    );
  }

  Widget _buildFacetLayout(
      FacetManagerState state, AsyncValue<List<SessionInfo>> sessionsAsync,
      WsState wsState, AsyncValue<List<KuboInfo>> kubosAsync) {
    final allFacets = state.order
        .map((id) => state.facets[id])
        .whereType<FacetData>()
        .toList();

    for (final facet in allFacets) {
      _ensureCardKey(facet.id);
    }

    final serverSessions = sessionsAsync.when(
      data: (list) => list,
      loading: () => <SessionInfo>[],
      error: (_, _) => <SessionInfo>[],
    );
    final openSessionNames = state.facets.values
        .map((f) => f.sessionName)
        .toSet();
    final kubos = kubosAsync.when(
      data: (list) => list,
      loading: () => <KuboInfo>[],
      error: (_, _) => <KuboInfo>[],
    );

    return LayoutBuilder(
      builder: (context, constraints) {
        final narrow = constraints.maxWidth < _narrowBreakpoint;

        final sessionInfoMap = {
          for (final s in serverSessions) s.name: s,
        };

        if (narrow) {
          // Narrow: focused facet fullscreen only
          return _buildFocusedArea(state, sessionInfoMap);
        }

        // Wide: StageStrip always visible on left + focused area
        return Row(
          children: [
            StageStrip(
              allFacets: allFacets,
              focusedId: state.focusedId,
              cardKeys: _cardKeys,
              serverSessions: serverSessions,
              openSessionNames: openSessionNames,
              onFocusFacet: _focusFacet,
              onOpenSession: _onOpenSession,
              onDeleteSession: _onDeleteSession,
              onSessionSettings: (name) =>
                  setState(() => _sessionSettingsName = name),
              onNewSession: _createNewFacet,
              onNewSessionInKubo: (kubo) => _addAbotToKubo(kubo),
              onNewKubo: _createNewKubo,
              onOpenBundle: _openBundle,
              onKuboSettings: (name) {
                ScaffoldMessenger.of(context).showSnackBar(
                  SnackBar(content: Text('Kubo settings: $name')),
                );
              },
              connectionState: wsState.connectionState,
              sessionInfoMap: sessionInfoMap,
              kubos: kubos,
              collapsed: _sidebarCollapsed,
              onToggleCollapse: _toggleSidebar,
              onSettingsTap: () =>
                  setState(() => _showSettings = !_showSettings),
              onScroll: _updateSidebarTransforms,
              onTabChanged: (tab) {
                setState(() => _sidebarTab = tab);
                _updateSidebarTransforms();
              },
            ),
            Expanded(child: _buildFocusedArea(state, sessionInfoMap)),
          ],
        );
      },
    );
  }

  /// Build the focused terminal area. ALL terminals are full-size
  /// (Positioned.fill) so their xterm.js WebGL canvases render at full
  /// resolution. Unfocused terminals are CSS-transformed to their sidebar
  /// card positions (GPU-accelerated).
  Widget _buildFocusedArea(FacetManagerState state, Map<String, SessionInfo> sessionInfoMap) {
    final focusedId = state.focusedId;
    if (focusedId == null) return const SizedBox.shrink();

    for (final id in state.order) {
      _ensureFacetKey(id);
    }

    // After layout, compute CSS transforms for sidebar positioning.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _updateSidebarTransforms();
    });

    return Stack(
      key: _mainAreaKey,
      clipBehavior: Clip.none,
      children: [
        // Unfocused terminals — full-size, CSS-transformed to sidebar slots.
        for (final id in state.stripOrder)
          Positioned.fill(
            child: IgnorePointer(
              child: TerminalFacet(
                key: _facetKeys[id],
                facetId: id,
                sessionName: state.facets[id]!.sessionName,
                isFocused: false,
                showTitleBar: false,
              ),
            ),
          ),
        // Mirror of the focused terminal — a second read-only xterm.js instance
        // connected to the same daemon session, CSS-transformed to the focused
        // sidebar card. Recreated on focus change via ValueKey.
        if (state.count > 1)
          Positioned.fill(
            child: IgnorePointer(
              child: TerminalFacet(
                key: ValueKey('mirror_$focusedId'),
                facetId: '${focusedId}_mirror',
                sessionName: state.facets[focusedId]!.sessionName,
                isFocused: false,
                isMirror: true,
                showTitleBar: false,
              ),
            ),
          ),
        // Focused terminal — on top, no CSS transform.
        Positioned.fill(
          child: TerminalFacet(
            key: _facetKeys[focusedId],
            facetId: focusedId,
            sessionName: state.facets[focusedId]!.sessionName,
            isFocused: true,
            isDirty: sessionInfoMap[state.facets[focusedId]!.sessionName]?.dirty ?? false,
            showTitleBar: true,
            onSettings: () => setState(() =>
                _sessionSettingsName =
                    state.facets[focusedId]!.sessionName),
            onMinimize: () => _minimizeFacet(focusedId),
            onClose: () => _closeFacet(focusedId),
          ),
        ),
      ],
    );
  }
}

