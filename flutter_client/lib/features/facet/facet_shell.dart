import 'dart:async';
import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:web/web.dart' as web;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/network/abot_service.dart';
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
import '../settings/kubo_settings_panel.dart';
import '../settings/abot_detail_panel.dart';
import 'kubo_landing.dart';

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

  /// Kubo name for per-kubo settings overlay (null = hidden).
  String? _kuboSettingsName;

  /// Abot name for abot detail overlay (null = hidden).
  String? _abotDetailName;

  /// Active kubo shown in main area landing page (null = none selected).
  String? _activeKubo;

  /// Which kubos are visible in the sidebar (persisted in localStorage).
  Set<String> _openKubos = {};

  static const _activeKuboKey = 'abot_active_kubo';
  static const _openKubosKey = 'abot_open_kubos';

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);

    // Restore active kubo from localStorage.
    final stored = web.window.localStorage.getItem(_activeKuboKey);
    if (stored != null && stored.isNotEmpty) {
      _activeKubo = stored;
    }

    // Restore open kubos from localStorage.
    final openKubosJson = web.window.localStorage.getItem(_openKubosKey);
    if (openKubosJson != null) {
      try {
        final list = (jsonDecode(openKubosJson) as List).cast<String>();
        _openKubos = {...list};
      } catch (_) {}
    }

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
    // If no sessions exist, the empty kubo onboarding page is shown.
    try {
      final sessions = await ref
          .read(sessionServiceProvider.notifier)
          .listSessions();
      if (!mounted) return;

      if (sessions.isNotEmpty) {
        for (final s in sessions) {
          facetManager.create(s.name);
        }
        facetManager.loadPersistedOrder();
      }
      // If no sessions, we show the empty kubo onboarding page.
    } catch (e) {
      debugPrint('[FacetShell] Failed to fetch sessions: $e');
      // Server unreachable — empty state, onboarding page shown.
    }

    // Reconcile localStorage kubos against server — prune stale entries,
    // and auto-create sessions for abots in running kubos.
    try {
      final kubos = await ref.read(kuboServiceProvider.notifier).listKubos();
      if (!mounted) return;
      final serverKuboNames = kubos.map((k) => k.name).toSet();
      final stale = _openKubos.difference(serverKuboNames);
      if (stale.isNotEmpty) {
        setState(() {
          _openKubos.removeAll(stale);
          if (_activeKubo != null && !serverKuboNames.contains(_activeKubo)) {
            _activeKubo = _openKubos.isNotEmpty ? _openKubos.first : null;
          }
          _persistOpenKubos();
          if (_activeKubo != null) {
            web.window.localStorage.setItem(_activeKuboKey, _activeKubo!);
          } else {
            web.window.localStorage.removeItem(_activeKuboKey);
          }
        });
      }

      // Auto-create sessions for abots in open/active kubos.
      // createAbotInKubo will start the kubo container if needed.
      for (final kubo in kubos) {
        if (!_openKubos.contains(kubo.name) && kubo.name != _activeKubo) continue;
        if (kubo.abots.isEmpty) continue;
        for (final abot in kubo.abots) {
          try {
            await facetManager.createAbotInKubo(abot, kubo: kubo.name);
            if (!mounted) return;
          } catch (e) {
            debugPrint('[FacetShell] Auto-start $abot in ${kubo.name}: $e');
          }
        }
      }
    } catch (_) {}

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
        ref.read(abotServiceProvider.notifier).refresh();
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

  void _showAbotDetail(String name) {
    setState(() => _abotDetailName = name);
  }

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
    // Re-focus the terminal after sidebar toggle
    final focusedId = ref.read(facetManagerProvider).focusedId;
    if (focusedId != null) {
      TerminalRegistry.instance.focusTerminal(focusedId);
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
    if (state.focusedId != null) {
      // Clear the focused terminal's transform so it renders full-size.
      // Without this, a terminal that was previously offscreen (unfocused)
      // would keep its offscreen transform after gaining focus.
      TerminalRegistry.instance
          .clearGenieTransform(state.focusedId!, animate: false);
      // Hide the mirror of the focused terminal
      TerminalRegistry.instance.setGenieTransform(
        '${state.focusedId!}_mirror',
        _offscreenTransform,
        animate: false,
      );
    }
    _removeAllLabelOverlays();
  }

  // --- Facet lifecycle ---

  /// Show a dialog to create a new kubo (empty — user adds abots later).
  Future<void> _createNewKubo() async {
    final name = await _showNewKuboDialog();
    if (name == null || name.isEmpty || !mounted) return;
    try {
      await ref.read(kuboServiceProvider.notifier).createKubo(name);
      if (!mounted) return;
      setState(() {
        _openKubos.add(name);
        _activeKubo = name;
      });
      web.window.localStorage.setItem(_activeKuboKey, name);
      _persistOpenKubos();
      ref.read(kuboServiceProvider.notifier).refresh();
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
      ref.read(abotServiceProvider.notifier).refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to add abot: $e')),
      );
    }
  }

  Future<void> _createAbotSession(String abotName, String kuboName) async {
    try {
      await ref.read(facetManagerProvider.notifier).createAbotInKubo(
        abotName,
        kubo: kuboName,
      );
      if (!mounted) return;
      ref.read(kuboServiceProvider.notifier).refresh();
      ref.read(abotServiceProvider.notifier).refresh();
    } catch (e) {
      if (!mounted) return;
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to start abot: $e')),
        );
      }
    }
  }

  void _persistOpenKubos() {
    web.window.localStorage.setItem(
      _openKubosKey,
      jsonEncode(_openKubos.toList()),
    );
  }

  /// Open a .kubo directory via native OS directory picker.
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
        setState(() {
          _openKubos.add(name);
          _activeKubo = name;
        });
        web.window.localStorage.setItem(_activeKuboKey, name);
        _persistOpenKubos();
      }
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Open kubo failed: $e')),
      );
    }
  }

  /// Remove an abot from a kubo (unemploy).
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
          _minimizeFacet(facet.id);
        }
      }

      await ref
          .read(kuboServiceProvider.notifier)
          .removeAbotFromKubo(kuboName, abotName);
      if (!mounted) return;
      ref.read(sessionServiceProvider.notifier).refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to remove abot: $e')),
      );
    }
  }

  Future<String?> _showNewAbotDialog(String kuboName) =>
      _showNameDialog(title: 'New Abot in $kuboName', hint: 'abot name');

  Future<String?> _showNewKuboDialog() =>
      _showNameDialog(title: 'New Kubo', hint: 'kubo name');

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

    // Re-focus the terminal after focus switch (didUpdateWidget may not fire
    // if xterm.js initialized after the widget was built).
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) TerminalRegistry.instance.focusTerminal(facetId);
    });
  }

  /// Open or focus a server session from the strip.
  void _onOpenSession(String sessionName) {
    ref.read(facetManagerProvider.notifier).openOrFocusSession(sessionName);
    // Re-focus the terminal after the facet is created or focused
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final focusedId = ref.read(facetManagerProvider).focusedId;
      if (focusedId != null) {
        TerminalRegistry.instance.focusTerminal(focusedId);
      }
    });
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

  /// Open a .abot bundle via native OS file picker (into active kubo).
  Future<void> _openBundle() async {
    if (_activeKubo == null) return;
    await _openBundleInKubo(_activeKubo!);
  }

  /// Open a .abot bundle via native OS file picker into a specific kubo.
  Future<void> _openBundleInKubo(String kubo) async {
    try {
      final data = await const ApiClient().post('/api/pick-file', {})
          as Map<String, dynamic>;
      final path = data['path'] as String?;
      if (path == null || path.isEmpty || !mounted) return;

      // Open the bundle into the specified kubo via the REST endpoint.
      final result = await ref
          .read(sessionServiceProvider.notifier)
          .openBundle(path, kubo: kubo);
      final sessionName = result['name'] as String?;
      if (sessionName != null && mounted) {
        ref.read(facetManagerProvider.notifier).openOrFocusSession(sessionName);
      }
      if (!mounted) return;
      ref.read(kuboServiceProvider.notifier).refresh();
      ref.read(abotServiceProvider.notifier).refresh();
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
    final abotsAsync = ref.watch(abotServiceProvider);

    return Scaffold(
      backgroundColor: context.palette.base,
      // Remove any system-inferred bottom padding (e.g. virtual keyboard,
      // browser chrome) so the terminal fills to the window edge.
      resizeToAvoidBottomInset: false,
      body: MediaQuery.removePadding(
        context: context,
        removeBottom: true,
        child: Stack(
        children: [
          CallbackShortcuts(
            bindings: {
              // Ctrl+` — cycle focus
              const SingleActivator(LogicalKeyboardKey.backquote,
                  control: true): _cycleFocus,
              // Ctrl+Tab — cycle focus (alias)
              const SingleActivator(LogicalKeyboardKey.tab,
                  control: true): _cycleFocus,
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
              // Cmd+B (macOS) / Ctrl+Shift+B (other) — toggle sidebar
              // Ctrl+B alone passes through as tmux prefix
              SingleActivator(LogicalKeyboardKey.keyB,
                  control: defaultTargetPlatform != TargetPlatform.macOS,
                  meta: defaultTargetPlatform == TargetPlatform.macOS,
                  shift: defaultTargetPlatform != TargetPlatform.macOS):
                  _toggleSidebar,
            },
            child: Focus(
              autofocus: true,
              child: _buildFacetLayout(facetState, sessionsAsync, wsState, kubosAsync, abotsAsync),
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
          if (_abotDetailName != null)
            Builder(builder: (context) {
              final abots = ref.read(abotServiceProvider).when(
                data: (list) => list,
                loading: () => <AbotInfo>[],
                error: (_, _) => <AbotInfo>[],
              );
              final abot = abots.where((a) => a.name == _abotDetailName).firstOrNull
                  ?? AbotInfo(name: _abotDetailName!);
              return AbotDetailPanel(
                detail: abot,
                onClose: () => setState(() => _abotDetailName = null),
                onRemove: () async {
                  final name = _abotDetailName!;
                  await ref.read(abotServiceProvider.notifier).removeAbot(name);
                  if (!mounted) return;
                  setState(() => _abotDetailName = null);
                },
                onSwitchToKubo: (kuboName) {
                  setState(() {
                    _activeKubo = kuboName;
                    _abotDetailName = null;
                  });
                  web.window.localStorage.setItem(_activeKuboKey, kuboName);
                },
                onIntegrate: (kuboName) async {
                  final name = _abotDetailName!;
                  await ref.read(abotServiceProvider.notifier).integrateVariant(name, kuboName);
                  if (!mounted) return;
                },
                onDiscard: (kuboName) async {
                  final name = _abotDetailName!;
                  await ref.read(abotServiceProvider.notifier).discardVariant(name, kuboName);
                  if (!mounted) return;
                },
                onDismiss: (kuboName) async {
                  final name = _abotDetailName!;
                  await ref.read(abotServiceProvider.notifier).dismissVariant(name, kuboName);
                  if (!mounted) return;
                },
              );
            }),
          if (_kuboSettingsName != null)
            Builder(builder: (context) {
              final kubos = ref.read(kuboServiceProvider).when(
                data: (list) => list,
                loading: () => <KuboInfo>[],
                error: (_, _) => <KuboInfo>[],
              );
              final kubo = kubos.where((k) => k.name == _kuboSettingsName).firstOrNull
                  ?? KuboInfo(name: _kuboSettingsName!, running: false);
              return KuboSettingsPanel(
                kubo: kubo,
                onClose: () => setState(() => _kuboSettingsName = null),
                onStart: !kubo.running ? () async {
                  final messenger = ScaffoldMessenger.of(context);
                  try {
                    await ref.read(kuboServiceProvider.notifier).startKubo(kubo.name);
                  } catch (e) {
                    if (!mounted) return;
                    messenger.showSnackBar(
                      SnackBar(content: Text('Failed to start kubo: $e')),
                    );
                  }
                  if (!mounted) return;
                  setState(() {}); // rebuild with fresh kubo state
                } : null,
                onShutdown: kubo.running ? () async {
                  final messenger = ScaffoldMessenger.of(context);
                  try {
                    await ref.read(kuboServiceProvider.notifier).stopKubo(kubo.name);
                  } catch (e) {
                    if (!mounted) return;
                    messenger.showSnackBar(
                      SnackBar(content: Text('Failed to stop kubo: $e')),
                    );
                  }
                  if (!mounted) return;
                  setState(() {}); // rebuild with fresh kubo state
                } : null,
              );
            }),
        ],
      ),
      ),
    );
  }

  Widget _buildFacetLayout(
      FacetManagerState state, AsyncValue<List<SessionInfo>> sessionsAsync,
      WsState wsState, AsyncValue<List<KuboInfo>> kubosAsync,
      AsyncValue<List<AbotInfo>> abotsAsync) {
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

    final knownAbots = abotsAsync.when(
      data: (list) => list,
      loading: () => <AbotInfo>[],
      error: (_, _) => <AbotInfo>[],
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
              onNewSession: () { if (_activeKubo != null) _addAbotToKubo(_activeKubo!); },
              onNewSessionInKubo: (kubo) => _addAbotToKubo(kubo),
              onNewKubo: _createNewKubo,
              onOpenBundle: _openBundle,
              onOpenBundleInKubo: _openBundleInKubo,
              onOpenKubo: _openKuboFromDisk,
              onRemoveAbot: _removeAbotFromKubo,
              onKuboSettings: (name) =>
                  setState(() => _kuboSettingsName = name),
              connectionState: wsState.connectionState,
              sessionInfoMap: sessionInfoMap,
              kubos: kubos.where((k) => _openKubos.contains(k.name)).toList(),
              collapsed: _sidebarCollapsed,
              onToggleCollapse: _toggleSidebar,
              onSettingsTap: () =>
                  setState(() => _showSettings = !_showSettings),
              onScroll: _updateSidebarTransforms,
              onTabChanged: (tab) {
                setState(() => _sidebarTab = tab);
                _updateSidebarTransforms();
                // Re-focus the terminal after tab switch
                final focusedId = ref.read(facetManagerProvider).focusedId;
                if (focusedId != null) {
                  TerminalRegistry.instance.focusTerminal(focusedId);
                }
              },
              knownAbots: knownAbots,
              onAbotDetail: (name) => _showAbotDetail(name),
              onIntegrateVariant: (abotName, kuboName) async {
                await ref.read(abotServiceProvider.notifier).integrateVariant(abotName, kuboName);
              },
              onDiscardVariant: (abotName, kuboName) async {
                await ref.read(abotServiceProvider.notifier).discardVariant(abotName, kuboName);
              },
              onDismissVariant: (abotName, kuboName) async {
                await ref.read(abotServiceProvider.notifier).dismissVariant(abotName, kuboName);
              },
              onCreateAbotSession: _createAbotSession,
              activeKubo: _activeKubo,
              onActiveKuboChanged: (kubo) {
                setState(() => _activeKubo = kubo);
                web.window.localStorage.setItem(_activeKuboKey, kubo);
                // Unfocus if the focused facet doesn't belong to the new kubo.
                final focusedId = ref.read(facetManagerProvider).focusedId;
                if (focusedId != null) {
                  final focusedSession = ref.read(facetManagerProvider).facets[focusedId]?.sessionName;
                  final focusedKubo = sessionInfoMap[focusedSession]?.kubo;
                  if (focusedKubo != kubo) {
                    ref.read(facetManagerProvider.notifier).unfocus();
                  }
                }
              },
            ),
            Expanded(child: _buildFocusedArea(state, sessionInfoMap)),
          ],
        );
      },
    );
  }

  Widget _buildKuboLandingPage(String kubo, Map<String, SessionInfo> sessionInfoMap) {
    final kubos = ref.read(kuboServiceProvider).when(
      data: (list) => list,
      loading: () => <KuboInfo>[],
      error: (_, _) => <KuboInfo>[],
    );
    return KuboLandingPage(
      kubo: kubo,
      sessionInfoMap: sessionInfoMap,
      kubos: kubos,
      onOpenSession: _onOpenSession,
      onCreateAbotSession: _createAbotSession,
      onAddAbot: _addAbotToKubo,
      onOpenBundle: _openBundle,
    );
  }

  /// Build the focused terminal area. ALL terminals are full-size
  /// (Positioned.fill) so their xterm.js WebGL canvases render at full
  /// resolution. Unfocused terminals are CSS-transformed to their sidebar
  /// card positions (GPU-accelerated).
  Widget _buildFocusedArea(FacetManagerState state, Map<String, SessionInfo> sessionInfoMap) {
    final focusedId = state.focusedId;
    if (focusedId == null) {
      if (_activeKubo != null) {
        return _buildKuboLandingPage(_activeKubo!, sessionInfoMap);
      }
      return EmptyStateLandingPage(
        onCreateKubo: _createNewKubo,
        onOpenKubo: _openKuboFromDisk,
      );
    }

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

