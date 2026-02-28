import 'dart:async';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/network/api_client.dart';
import '../../core/network/session_service.dart';
import '../../core/network/websocket_service.dart';
import '../../core/network/ws_messages.dart';
import '../../core/theme/abot_theme.dart';
import '../terminal/terminal_facet.dart';
import 'facet.dart';
import 'facet_manager.dart';
import 'stage_strip.dart';

const double _narrowBreakpoint = 768;

/// The main app shell — iPad Stage Manager-style layout: one focused facet
/// takes center stage while others appear as live-preview cards in a side strip.
class FacetShell extends ConsumerStatefulWidget {
  const FacetShell({super.key});

  @override
  ConsumerState<FacetShell> createState() => _FacetShellState();
}

class _FacetShellState extends ConsumerState<FacetShell>
    with WidgetsBindingObserver {
  /// Monotonic counter for session naming (starts at 1 since 'main' is created in _initialize).
  int _nextSessionId = 1;

  /// GlobalKeys per facet — ensures Flutter reuses the same State when a facet
  /// moves between focused/offstage, preserving the HtmlElementView (xterm).
  final Map<String, GlobalKey> _facetKeys = {};

  /// Card position keys for CSS transform calculation.
  final Map<String, GlobalKey> _cardKeys = {};
  final GlobalKey _mainAreaKey = GlobalKey();

  /// IDs of terminals currently animating (skip instant transform updates).
  final Set<String> _animatingIds = {};

  /// Timer for post-animation cleanup (cancelled on rapid re-focus).
  Timer? _animationCleanup;

  /// Subscription from ref.listenManual — cancelled in dispose.
  ProviderSubscription? _wsSubscription;

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
      final running = sessions.where((s) => s.status == 'running').toList();

      if (running.isNotEmpty) {
        // Create facets for all running sessions. Focus 'main' if it exists,
        // otherwise focus the first one.
        final mainFirst = <SessionInfo>[
          ...running.where((s) => s.name == 'main'),
          ...running.where((s) => s.name != 'main'),
        ];
        for (final s in mainFirst) {
          facetManager.create(s.name);
          // Bump _nextSessionId past existing session-N names
          final match = RegExp(r'^session-(\d+)$').firstMatch(s.name);
          if (match != null) {
            final n = int.parse(match.group(1)!) + 1;
            if (n > _nextSessionId) _nextSessionId = n;
          }
        }
        // Focus 'main' if present
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
        // Refresh server session list on connect
        ref.read(sessionServiceProvider.notifier).refresh();
      }
    });
  }

  void _handleServerMessage(ServerMessage msg) {
    switch (msg) {
      case AttachedMessage(:final session, :final buffer):
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

  // --- Facet lifecycle ---

  Future<void> _createNewFacet() async {
    final facetManager = ref.read(facetManagerProvider.notifier);
    final sessionName = 'session-$_nextSessionId';
    _nextSessionId++;

    try {
      await ref.read(sessionServiceProvider.notifier).createSession(sessionName);
    } on ApiException catch (e) {
      // 409 Conflict means session already exists — safe to proceed with attach
      if (e.statusCode != 409) rethrow;
    }

    if (!mounted) return;
    facetManager.create(sessionName);
    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.attachSession(sessionName);
  }

  void _closeFacet(String facetId, String sessionName) {
    TerminalRegistry.instance.clearGenieTransform(facetId, animate: false);
    final facetManager = ref.read(facetManagerProvider.notifier);
    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.detachSession(sessionName);
    facetManager.remove(facetId);
    _facetKeys.remove(facetId);
    _cardKeys.remove(facetId);
  }

  void _focusFacet(String facetId) {
    final currentFocused = ref.read(facetManagerProvider).focusedId;
    if (facetId == currentFocused) return;

    // Capture card rects BEFORE focus changes the sidebar layout.
    _ensureCardKey(facetId);
    if (currentFocused != null) _ensureCardKey(currentFocused);

    final mainRect = _getRectForKey(_mainAreaKey);

    // Animate outgoing terminal: full-size → sidebar card position (CSS transition).
    if (currentFocused != null && mainRect != null) {
      final outgoingCardRect = _getRectForKey(_cardKeys[currentFocused]!);
      if (outgoingCardRect != null) {
        _animatingIds.add(currentFocused);
        final tx = outgoingCardRect.left - mainRect.left;
        final ty = outgoingCardRect.top - mainRect.top;
        final sx = outgoingCardRect.width / mainRect.width;
        final sy = outgoingCardRect.height / mainRect.height;
        TerminalRegistry.instance.setGenieTransform(
          currentFocused,
          'translate(${tx}px, ${ty}px) scale($sx, $sy)',
        );
      }
    }

    // Animate incoming terminal: sidebar card → full-size (CSS transition).
    _animatingIds.add(facetId);
    TerminalRegistry.instance.clearGenieTransform(facetId);

    // Change focus instantly (terminal input works immediately).
    ref.read(facetManagerProvider.notifier).focus(facetId);

    // After CSS transition completes, refresh all transforms.
    // Cancel any previous cleanup timer (handles rapid focus cycling).
    _animationCleanup?.cancel();
    _animationCleanup = Timer(const Duration(milliseconds: 450), () {
      if (!mounted) return;
      _animatingIds.clear();
      _updateSidebarTransforms();
    });
  }

  /// Open or focus a server session from the strip.
  void _onOpenSession(String sessionName) {
    final facetState = ref.read(facetManagerProvider);
    final existing = facetState.getBySession(sessionName);
    if (existing != null) {
      _focusFacet(existing.id);
    } else {
      final facetManager = ref.read(facetManagerProvider.notifier);
      facetManager.create(sessionName);
      final wsService = ref.read(wsServiceProvider.notifier);
      wsService.attachSession(sessionName);
    }
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
    final state = ref.read(facetManagerProvider);
    final mainRect = _getRectForKey(_mainAreaKey);
    if (mainRect == null || mainRect.width == 0 || mainRect.height == 0) return;

    for (final id in state.stripOrder) {
      if (_animatingIds.contains(id)) continue;
      _ensureCardKey(id);
      final cardRect = _getRectForKey(_cardKeys[id]!);
      if (cardRect == null) continue;

      final tx = cardRect.left - mainRect.left;
      final ty = cardRect.top - mainRect.top;
      final sx = cardRect.width / mainRect.width;
      final sy = cardRect.height / mainRect.height;

      TerminalRegistry.instance.setGenieTransform(
        id,
        'translate(${tx}px, ${ty}px) scale($sx, $sy)',
        animate: false,
      );
    }

    // Ensure the focused terminal has no transform.
    if (state.focusedId != null &&
        !_animatingIds.contains(state.focusedId)) {
      TerminalRegistry.instance
          .clearGenieTransform(state.focusedId!, animate: false);
    }
  }

  @override
  void dispose() {
    _animationCleanup?.cancel();
    _wsSubscription?.close();
    TerminalRegistry.instance.onRegistered = null;
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final facetState = ref.watch(facetManagerProvider);
    final sessionsAsync = ref.watch(sessionServiceProvider);

    return Scaffold(
      backgroundColor: context.palette.base,
      body: CallbackShortcuts(
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
          // Ctrl+W — close current facet
          SingleActivator(LogicalKeyboardKey.keyW,
              control: defaultTargetPlatform != TargetPlatform.macOS,
              meta: defaultTargetPlatform == TargetPlatform.macOS): () {
            final state = ref.read(facetManagerProvider);
            if (state.focusedId != null && state.count > 1) {
              final facet = state.facets[state.focusedId!];
              if (facet != null) {
                _closeFacet(facet.id, facet.sessionName);
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
        },
        child: Focus(
          autofocus: true,
          child: _buildFacetLayout(facetState, sessionsAsync),
        ),
      ),
    );
  }

  Widget _buildFacetLayout(
      FacetManagerState state, AsyncValue<List<SessionInfo>> sessionsAsync) {
    if (state.facets.isEmpty) {
      return const Center(
        child: Text(
          'Connecting...',
          style: TextStyle(
            fontFamily: AbotFonts.mono,
            fontSize: 14,
          ),
        ),
      );
    }

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
      error: (_, __) => <SessionInfo>[],
    );
    final openSessionNames = state.facets.values
        .map((f) => f.sessionName)
        .toSet();

    return LayoutBuilder(
      builder: (context, constraints) {
        final narrow = constraints.maxWidth < _narrowBreakpoint;

        if (narrow) {
          // Narrow: focused facet fullscreen only
          return _buildFocusedArea(state);
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
              onReorder: (oldIndex, newIndex) {
                ref.read(facetManagerProvider.notifier)
                    .reorder(oldIndex, newIndex);
              },
              onOpenSession: _onOpenSession,
              onDeleteSession: _onDeleteSession,
              onNewSession: _createNewFacet,
            ),
            Expanded(child: _buildFocusedArea(state)),
          ],
        );
      },
    );
  }

  /// Build the focused terminal area. ALL terminals are full-size
  /// (Positioned.fill) so their xterm.js WebGL canvases render at full
  /// resolution. Unfocused terminals are CSS-transformed to their sidebar
  /// card positions (GPU-accelerated).
  Widget _buildFocusedArea(FacetManagerState state) {
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
        // Focused terminal — on top, no CSS transform.
        Positioned.fill(
          child: TerminalFacet(
            key: _facetKeys[focusedId],
            facetId: focusedId,
            sessionName: state.facets[focusedId]!.sessionName,
            isFocused: true,
            showTitleBar: state.count > 1,
            onClose: state.count > 1
                ? () => _closeFacet(
                    focusedId, state.facets[focusedId]!.sessionName)
                : null,
          ),
        ),
      ],
    );
  }

}
