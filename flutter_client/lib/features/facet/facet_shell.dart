import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/network/websocket_service.dart';
import '../../core/network/ws_messages.dart';
import '../../core/theme/abot_theme.dart';
import '../terminal/terminal_facet.dart';
import '../shortcut_bar/shortcut_bar.dart';
import 'facet_manager.dart';

/// The main app shell that holds facets and the shortcut bar.
/// This is the top-level widget that wires WebSocket messages to facets.
class FacetShell extends ConsumerStatefulWidget {
  const FacetShell({super.key});

  @override
  ConsumerState<FacetShell> createState() => _FacetShellState();
}

class _FacetShellState extends ConsumerState<FacetShell>
    with WidgetsBindingObserver {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);

    // Create default facet and connect
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _initialize();
    });
  }

  void _initialize() {
    // Create the default "main" session facet
    final facetManager = ref.read(facetManagerProvider.notifier);
    facetManager.create('main');

    // Set up WebSocket message routing
    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.onMessage = _handleServerMessage;

    // Connect to server
    wsService.connect();

    // Attach default session once connected
    // (the ws service handles re-attach on reconnect,
    //  but we need the initial attach)
    ref.listenManual(wsServiceProvider, (prev, next) {
      if (prev?.connectionState != WsConnectionState.connected &&
          next.connectionState == WsConnectionState.connected) {
        // Connection established — attach all facets
        final facets = ref.read(facetManagerProvider).facets;
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
        // P2P not yet implemented in Flutter client
        break;

      case ServerDrainingMessage():
      case ReloadMessage():
        // Handled by WsServiceNotifier
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

  void _createNewFacet() {
    final facetManager = ref.read(facetManagerProvider.notifier);
    // Generate session name
    final count = ref.read(facetManagerProvider).count;
    final sessionName = count == 0 ? 'main' : 'session-$count';
    facetManager.create(sessionName);

    // Attach new session
    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.attachSession(sessionName);
  }

  void _closeFacet(String facetId, String sessionName) {
    final facetManager = ref.read(facetManagerProvider.notifier);
    final wsService = ref.read(wsServiceProvider.notifier);
    wsService.detachSession(sessionName);
    facetManager.remove(facetId);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
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
      body: CallbackShortcuts(
        bindings: {
          // Ctrl+` to cycle focus
          const SingleActivator(LogicalKeyboardKey.backquote,
              control: true): () {
            ref.read(facetManagerProvider.notifier).cycleFocus();
          },
        },
        child: Focus(
          autofocus: true,
          child: Column(
            children: [
              // Facets area
              Expanded(
                child: _buildFacetLayout(facetState, showTitleBar),
              ),
              // Shortcut bar
              ShortcutBar(
                facets: facetState.facets,
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

    // Single facet: fullscreen
    if (state.facets.length == 1) {
      final facet = state.facets.first;
      return TerminalFacet(
        key: ValueKey(facet.id),
        facetId: facet.id,
        sessionName: facet.sessionName,
        isFocused: true,
        showTitleBar: false,
      );
    }

    // Multiple facets: equal-width columns
    return Row(
      children: state.facets.map((facet) {
        return Expanded(
          child: TerminalFacet(
            key: ValueKey(facet.id),
            facetId: facet.id,
            sessionName: facet.sessionName,
            isFocused: facet.id == state.focusedId,
            showTitleBar: showTitleBar,
            onFocused: () {
              ref.read(facetManagerProvider.notifier).focus(facet.id);
            },
            onClose: () => _closeFacet(facet.id, facet.sessionName),
          ),
        );
      }).toList(),
    );
  }
}
