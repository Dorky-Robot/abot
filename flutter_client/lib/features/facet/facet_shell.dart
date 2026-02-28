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

/// The main app shell that holds facets and the shortcut bar.
/// Uses an iPad Stage Manager-style layout: one focused facet takes center
/// stage while others appear as perspective-tilted cards in a side strip.
class FacetShell extends ConsumerStatefulWidget {
  const FacetShell({super.key});

  @override
  ConsumerState<FacetShell> createState() => _FacetShellState();
}

class _FacetShellState extends ConsumerState<FacetShell>
    with WidgetsBindingObserver, SingleTickerProviderStateMixin {
  /// Monotonic counter for session naming (starts at 1 since 'main' is created in _initialize).
  int _nextSessionId = 1;

  /// GlobalKeys per facet — ensures Flutter reuses the same State when a facet
  /// moves between focused/offstage, preserving the HtmlElementView (xterm).
  final Map<String, GlobalKey> _facetKeys = {};

  /// Swap animation controller (300ms, easeOutCubic).
  AnimationController? _swapController;
  String? _swapFromId;
  String? _swapToId;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);

    WidgetsBinding.instance.addPostFrameCallback((_) {
      _initialize();
    });
  }

  void _initialize() {
    final facetManager = ref.read(facetManagerProvider.notifier);
    facetManager.create('main');

    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.onMessage = _handleServerMessage;
    wsService.connect();

    ref.listenManual(wsServiceProvider, (prev, next) {
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
      if (e.statusCode != null && e.statusCode != 409) rethrow;
    }

    facetManager.create(sessionName);
    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.attachSession(sessionName);
  }

  void _closeFacet(String facetId, String sessionName) {
    final facetManager = ref.read(facetManagerProvider.notifier);
    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.detachSession(sessionName);
    facetManager.remove(facetId);
    _facetKeys.remove(facetId);
  }

  void _focusFacet(String facetId) {
    final currentFocused = ref.read(facetManagerProvider).focusedId;
    if (facetId == currentFocused) return;

    _startSwapAnimation(currentFocused, facetId);
    ref.read(facetManagerProvider.notifier).focus(facetId);
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
    if (confirmed == true) {
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

  /// Toggle search on the focused terminal.
  void _toggleSearch() {
    final focusedId = ref.read(facetManagerProvider).focusedId;
    if (focusedId != null) {
      TerminalRegistry.instance.toggleSearchOnFacet(focusedId);
    }
  }

  // --- Swap animation ---

  void _startSwapAnimation(String? fromId, String toId) {
    _swapFromId = fromId;
    _swapToId = toId;

    _swapController?.dispose();
    _swapController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 300),
    );
    _swapController!.addListener(() {
      setState(() {});
    });
    _swapController!.addStatusListener((status) {
      if (status == AnimationStatus.completed) {
        setState(() {
          _swapFromId = null;
          _swapToId = null;
        });
      }
    });
    _swapController!.forward(from: 0);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _swapController?.dispose();
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
              control: true): () {
            final state = ref.read(facetManagerProvider);
            final currentFocused = state.focusedId;
            ref.read(facetManagerProvider.notifier).cycleFocus();
            final newFocused = ref.read(facetManagerProvider).focusedId;
            if (currentFocused != newFocused && newFocused != null) {
              _startSwapAnimation(currentFocused, newFocused);
            }
          },
          // Ctrl+Tab — cycle focus (alias)
          const SingleActivator(LogicalKeyboardKey.tab,
              control: true): () {
            final state = ref.read(facetManagerProvider);
            final currentFocused = state.focusedId;
            ref.read(facetManagerProvider.notifier).cycleFocus();
            final newFocused = ref.read(facetManagerProvider).focusedId;
            if (currentFocused != newFocused && newFocused != null) {
              _startSwapAnimation(currentFocused, newFocused);
            }
          },
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

    final stripFacets = state.stripOrder
        .map((id) => state.facets[id])
        .whereType<FacetData>()
        .toList();

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
              focusedFacet: state.focused,
              stripFacets: stripFacets,
              serverSessions: serverSessions,
              openSessionNames: openSessionNames,
              onFocusFacet: _focusFacet,
              onCloseFacet: _closeFacet,
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

  /// Build the focused terminal area with unfocused terminals kept alive
  /// via Offstage (preserves xterm.js state).
  Widget _buildFocusedArea(FacetManagerState state) {
    final focusedId = state.focusedId;
    if (focusedId == null) return const SizedBox.shrink();

    _ensureKey(focusedId);
    for (final id in state.stripOrder) {
      _ensureKey(id);
    }

    // Compute swap animation values
    final swapT = _swapController != null
        ? Curves.easeOutCubic.transform(_swapController!.value)
        : 1.0;
    final isSwapping = _swapFromId != null && _swapToId != null;

    return Stack(
      children: [
        // Focused terminal — visible, full size, with swap-in animation
        AnimatedBuilder(
          animation: _swapController ?? const AlwaysStoppedAnimation(1.0),
          builder: (context, child) {
            final scale = isSwapping && _swapToId == focusedId
                ? 0.95 + 0.05 * swapT
                : 1.0;
            final opacity = isSwapping && _swapToId == focusedId
                ? swapT
                : 1.0;
            return Opacity(
              opacity: opacity,
              child: Transform.scale(
                scale: scale,
                child: child,
              ),
            );
          },
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
        // Unfocused terminals — alive but hidden (preserves xterm.js state).
        // Using Positioned off-screen because Offstage may not properly hide
        // HtmlElementView DOM elements.
        for (final id in state.stripOrder)
          Positioned(
            left: -9999,
            top: -9999,
            child: SizedBox(
              width: 1,
              height: 1,
              child: TerminalFacet(
                key: _facetKeys[id],
                facetId: id,
                sessionName: state.facets[id]!.sessionName,
                isFocused: false,
                showTitleBar: false,
              ),
            ),
          ),
      ],
    );
  }

  /// Ensure a GlobalKey exists for a facet.
  void _ensureKey(String facetId) {
    _facetKeys.putIfAbsent(facetId, () => GlobalKey());
  }
}
