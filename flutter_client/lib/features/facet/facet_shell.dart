import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/network/api_client.dart';
import '../../core/network/session_service.dart';
import '../../core/network/websocket_service.dart';
import '../../core/network/ws_messages.dart';
import '../../core/theme/abot_theme.dart';
import '../session/session_drawer.dart';
import '../terminal/terminal_facet.dart';
import '../shortcut_bar/shortcut_bar.dart';
import 'drag_controller.dart';
import 'facet.dart';
import 'facet_manager.dart';

const double _narrowBreakpoint = 768;

/// The main app shell that holds facets and the shortcut bar.
/// This is the top-level widget that wires WebSocket messages to facets.
class FacetShell extends ConsumerStatefulWidget {
  const FacetShell({super.key});

  @override
  ConsumerState<FacetShell> createState() => _FacetShellState();
}

class _FacetShellState extends ConsumerState<FacetShell>
    with WidgetsBindingObserver, TickerProviderStateMixin {
  late final DragController _dragController;

  /// Monotonic counter for session naming (starts at 1 since 'main' is created in _initialize).
  int _nextSessionId = 1;

  /// GlobalKeys per facet for FLIP rect tracking.
  final Map<String, GlobalKey> _facetKeys = {};

  /// Drag preview state per facet (only the dragged facet shows a preview).
  final Map<String, DragPreview> _previews = {};

  /// The facet currently being dragged (for opacity).
  String? _draggingId;

  /// FLIP animation: captured rects before a mutation.
  Map<String, Rect>? _flipSnapshot;

  /// Animation controller for FLIP transitions.
  AnimationController? _flipController;

  /// Per-facet translation offsets for FLIP animation.
  final Map<String, Offset> _flipOffsets = {};

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);

    _dragController = DragController(
      onReorderColumns: _onReorderColumns,
      onMoveFacetToColumn: _onMoveFacetToColumn,
      onSplitFacetToOwnColumn: _onSplitFacetToOwnColumn,
      onPreviewChanged: _onPreviewChanged,
      onDragEnd: _onDragEnd,
      hitTest: _hitTestFacet,
      isInMultiFacetColumn: _isInMultiFacetColumn,
      hasMultipleColumns: _hasMultipleColumns,
    );

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

  /// Open or focus a session from the session drawer.
  void _onSessionTap(String sessionName) {
    final facetState = ref.read(facetManagerProvider);
    final existing = facetState.getBySession(sessionName);
    if (existing != null) {
      ref.read(facetManagerProvider.notifier).focus(existing.id);
    } else {
      final facetManager = ref.read(facetManagerProvider.notifier);
      facetManager.create(sessionName);
      final wsService = ref.read(wsServiceProvider.notifier);
      wsService.attachSession(sessionName);
    }
  }

  /// Toggle search on the focused terminal.
  void _toggleSearch() {
    final focusedId = ref.read(facetManagerProvider).focusedId;
    if (focusedId != null) {
      TerminalRegistry.instance.toggleSearchOnFacet(focusedId);
    }
  }

  // --- FLIP animation helpers ---

  /// Snapshot all facet rects before a mutation.
  void _captureFlipSnapshot() {
    _flipSnapshot = {};
    for (final entry in _facetKeys.entries) {
      final key = entry.value;
      final renderBox =
          key.currentContext?.findRenderObject() as RenderBox?;
      if (renderBox != null && renderBox.hasSize) {
        final position = renderBox.localToGlobal(Offset.zero);
        _flipSnapshot![entry.key] =
            Rect.fromLTWH(position.dx, position.dy,
                renderBox.size.width, renderBox.size.height);
      }
    }
  }

  /// After a mutation + rebuild, compute inverse offsets and animate.
  void _animateFlip() {
    if (_flipSnapshot == null) return;
    final snapshot = _flipSnapshot!;
    _flipSnapshot = null;

    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      _flipOffsets.clear();
      bool hasMotion = false;

      for (final entry in _facetKeys.entries) {
        final oldRect = snapshot[entry.key];
        if (oldRect == null) continue;
        final renderBox =
            entry.value.currentContext?.findRenderObject() as RenderBox?;
        if (renderBox == null || !renderBox.hasSize) continue;

        final newPos = renderBox.localToGlobal(Offset.zero);
        final dx = oldRect.left - newPos.dx;
        final dy = oldRect.top - newPos.dy;
        if (dx.abs() > 0.5 || dy.abs() > 0.5) {
          _flipOffsets[entry.key] = Offset(dx, dy);
          hasMotion = true;
        }
      }

      if (!hasMotion) return;

      _flipController?.dispose();
      _flipController = AnimationController(
        vsync: this,
        duration: const Duration(milliseconds: 200),
      );
      _flipController!.addListener(() {
        setState(() {});
      });
      _flipController!.addStatusListener((status) {
        if (status == AnimationStatus.completed) {
          _flipOffsets.clear();
          setState(() {});
        }
      });

      setState(() {}); // Apply inverse offsets
      _flipController!.forward(from: 0);
    });
  }

  /// Wrap a mutation with FLIP: capture → mutate → animate.
  void _flipMutate(VoidCallback mutate) {
    _captureFlipSnapshot();
    mutate();
    _animateFlip();
  }

  // --- Drag controller callbacks ---

  void _onReorderColumns(String draggedId, String targetId) {
    _flipMutate(() {
      ref.read(facetManagerProvider.notifier).reorderColumns(draggedId, targetId);
    });
  }

  void _onMoveFacetToColumn(String facetId, String targetFacetId) {
    _flipMutate(() {
      ref
          .read(facetManagerProvider.notifier)
          .moveFacetToColumn(facetId, targetFacetId);
    });
  }

  void _onSplitFacetToOwnColumn(String facetId) {
    _flipMutate(() {
      ref.read(facetManagerProvider.notifier).splitFacetToOwnColumn(facetId);
    });
  }

  void _onPreviewChanged(String facetId, DragPreview preview) {
    setState(() {
      if (preview == DragPreview.none) {
        _previews.remove(facetId);
      } else {
        _previews[facetId] = preview;
      }
    });
  }

  void _onDragEnd() {
    setState(() {
      _draggingId = null;
      _previews.clear();
    });
  }

  /// Hit-test: find which facet's GlobalKey rect contains the given point.
  String? _hitTestFacet(Offset globalPosition, String excludeId) {
    for (final entry in _facetKeys.entries) {
      if (entry.key == excludeId) continue;
      final renderBox =
          entry.value.currentContext?.findRenderObject() as RenderBox?;
      if (renderBox == null || !renderBox.hasSize) continue;
      final position = renderBox.localToGlobal(Offset.zero);
      final rect = Rect.fromLTWH(
          position.dx, position.dy, renderBox.size.width, renderBox.size.height);
      if (rect.contains(globalPosition)) return entry.key;
    }
    return null;
  }

  bool _isInMultiFacetColumn(String facetId) {
    final state = ref.read(facetManagerProvider);
    final pos = state.findFacet(facetId);
    if (pos == null) return false;
    return state.columns[pos.col].length > 1;
  }

  bool _hasMultipleColumns() {
    return ref.read(facetManagerProvider).columns.length > 1;
  }

  // --- Titlebar drag forwarding ---

  void _onTitleDragStart(String facetId, Offset globalPosition) {
    setState(() => _draggingId = facetId);
    _dragController.onDragStart(facetId, globalPosition);
  }

  void _onTitleDragUpdate(Offset globalPosition) {
    _dragController.onDragUpdate(globalPosition);
  }

  void _onTitleDragEnd() {
    _dragController.onDragEnded();
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _flipController?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final facetState = ref.watch(facetManagerProvider);
    final wsState = ref.watch(wsServiceProvider);
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final showTitleBar = facetState.count > 1;

    return Scaffold(
      backgroundColor: isDark ? CatppuccinMocha.base : CatppuccinLatte.base,
      endDrawer: SessionDrawer(onSessionTap: _onSessionTap),
      body: CallbackShortcuts(
        bindings: {
          // Ctrl+` — cycle focus
          const SingleActivator(LogicalKeyboardKey.backquote,
              control: true): () {
            ref.read(facetManagerProvider.notifier).cycleFocus();
          },
          // Ctrl+Tab — cycle focus (alias)
          const SingleActivator(LogicalKeyboardKey.tab,
              control: true): () {
            ref.read(facetManagerProvider.notifier).cycleFocus();
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
          child: Column(
            children: [
              Expanded(
                child: _buildFacetLayout(facetState, showTitleBar),
              ),
              ShortcutBar(
                facets: facetState.orderedFacets,
                focusedId: facetState.focusedId,
                connected: wsState.connectionState ==
                    WsConnectionState.connected,
                onNewFacet: _createNewFacet,
                onFocusFacet: (id) {
                  ref.read(facetManagerProvider.notifier).focus(id);
                },
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildFacetLayout(FacetManagerState state, bool showTitleBar) {
    if (state.facets.isEmpty) {
      return const Center(
        child: Text(
          'Connecting...',
          style: TextStyle(
            fontFamily: 'JetBrains Mono',
            fontSize: 14,
          ),
        ),
      );
    }

    // Single facet: fullscreen, no titlebar
    if (state.count == 1) {
      final facet = state.orderedFacets.first;
      _ensureKey(facet.id);
      return TerminalFacet(
        key: _facetKeys[facet.id],
        facetId: facet.id,
        sessionName: facet.sessionName,
        isFocused: true,
        showTitleBar: false,
      );
    }

    // Multiple facets: column-based tiling with narrow breakpoint
    return LayoutBuilder(
      builder: (context, constraints) {
        final narrow = constraints.maxWidth < _narrowBreakpoint;

        if (narrow) {
          // Vertical stack — all facets in a single column
          return Column(
            children: state.orderedFacets.map((facet) {
              return Expanded(
                child: _buildFacetTile(facet, state, showTitleBar),
              );
            }).toList(),
          );
        }

        // Wide layout: Row of columns, each column is a Column of facets
        return Row(
          children: state.columns.map((columnIds) {
            return Expanded(
              child: Column(
                children: columnIds.map((facetId) {
                  final facet = state.facets[facetId];
                  if (facet == null) return const SizedBox.shrink();
                  return Expanded(
                    child: _buildFacetTile(facet, state, showTitleBar),
                  );
                }).toList(),
              ),
            );
          }).toList(),
        );
      },
    );
  }

  /// Build a single facet tile with drag callbacks and FLIP animation.
  Widget _buildFacetTile(
      FacetData facet, FacetManagerState state, bool showTitleBar) {
    _ensureKey(facet.id);
    return _buildAnimatedFacet(
      facet.id,
      TerminalFacet(
        key: _facetKeys[facet.id],
        facetId: facet.id,
        sessionName: facet.sessionName,
        isFocused: facet.id == state.focusedId,
        showTitleBar: showTitleBar,
        dragPreview: _previews[facet.id] ?? DragPreview.none,
        isDragging: _draggingId == facet.id,
        onFocused: () {
          ref.read(facetManagerProvider.notifier).focus(facet.id);
        },
        onClose: () => _closeFacet(facet.id, facet.sessionName),
        onTitleDragStart: _onTitleDragStart,
        onTitleDragUpdate: _onTitleDragUpdate,
        onTitleDragEnd: _onTitleDragEnd,
      ),
    );
  }

  /// Ensure a GlobalKey exists for a facet (for FLIP rect tracking).
  void _ensureKey(String facetId) {
    _facetKeys.putIfAbsent(facetId, () => GlobalKey());
  }

  /// Wrap a facet widget with a FLIP translation transform if animating.
  Widget _buildAnimatedFacet(String facetId, Widget child) {
    final offset = _flipOffsets[facetId];
    if (offset == null || _flipController == null) return child;

    // Animate from inverse offset back to zero
    final t = Curves.easeOut.transform(_flipController!.value);
    final dx = offset.dx * (1 - t);
    final dy = offset.dy * (1 - t);

    return Transform.translate(
      offset: Offset(dx, dy),
      child: child,
    );
  }
}
