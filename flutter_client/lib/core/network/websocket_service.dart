import 'dart:async';
import 'dart:convert';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import 'package:web_socket_channel/web_socket_channel.dart';
import 'api_client.dart';
import 'ws_messages.dart';

/// Connection state
enum WsConnectionState { disconnected, connecting, connected }

/// WebSocket service state
class WsState {
  final WsConnectionState connectionState;
  final bool attached;

  const WsState({
    this.connectionState = WsConnectionState.disconnected,
    this.attached = false,
  });

  WsState copyWith({
    WsConnectionState? connectionState,
    bool? attached,
  }) =>
      WsState(
        connectionState: connectionState ?? this.connectionState,
        attached: attached ?? this.attached,
      );
}

/// WebSocket service provider
final wsServiceProvider =
    NotifierProvider<WsServiceNotifier, WsState>(WsServiceNotifier.new);

/// Callback for routing server messages to the right handler
typedef MessageCallback = void Function(ServerMessage message);

class WsServiceNotifier extends Notifier<WsState> {
  WebSocketChannel? _channel;
  StreamSubscription? _subscription;
  Timer? _reconnectTimer;
  int _reconnectDelay = 1000;
  bool _isConnecting = false;
  DateTime? _hiddenAt;
  int _consecutiveFailures = 0;

  MessageCallback? onMessage;

  /// Sessions to re-attach on reconnect
  final Map<String, ({int cols, int rows})> _attachedSessions = {};

  WebSocketChannel? get channel => _channel;

  @override
  WsState build() => const WsState();

  void connect() {
    if (_isConnecting) return;
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
    _isConnecting = true;

    state = state.copyWith(connectionState: WsConnectionState.connecting);

    final proto =
        web.window.location.protocol == 'https:' ? 'wss:' : 'ws:';
    final host = web.window.location.host;
    final uri = Uri.parse('$proto//$host/stream');

    _channel = WebSocketChannel.connect(uri);

    _channel!.ready.then((_) {
      _isConnecting = false;
      _reconnectDelay = 1000;
      _consecutiveFailures = 0;
      state = state.copyWith(connectionState: WsConnectionState.connected);

      // Re-attach all sessions
      for (final entry in _attachedSessions.entries) {
        send(AttachMessage(
          session: entry.key,
          cols: entry.value.cols,
          rows: entry.value.rows,
        ));
      }

      _subscription = _channel!.stream.listen(
        _onMessage,
        onDone: _onDisconnect,
        onError: (_) => _onDisconnect(),
      );
    }).catchError((_) {
      _isConnecting = false;
      _scheduleReconnect();
    });
  }

  void _onMessage(dynamic data) {
    if (data is! String) return;
    try {
      final json = jsonDecode(data) as Map<String, dynamic>;
      final msg = ServerMessage.fromJson(json);

      // Handle connection-level messages
      switch (msg) {
        case AttachedMessage():
          state = state.copyWith(attached: true);
        case ServerDrainingMessage():
          _reconnectDelay = 500;
          _channel?.sink.close();
          return;
        case ReloadMessage():
          web.window.location.reload();
          return;
        default:
          break;
      }

      onMessage?.call(msg);
    } catch (_) {
      // Ignore malformed messages
    }
  }

  void _onDisconnect() {
    _isConnecting = false;
    _subscription?.cancel();
    _subscription = null;
    _channel = null;
    state = state.copyWith(
      connectionState: WsConnectionState.disconnected,
      attached: false,
    );
    _scheduleReconnect();
  }

  void _scheduleReconnect() {
    _consecutiveFailures++;
    _reconnectTimer?.cancel();

    if (_consecutiveFailures == 2) {
      // Check if we've been kicked (credential revoked)
      _checkAuthAndMaybeRedirect();
      return;
    }

    _reconnectTimer = Timer(Duration(milliseconds: _reconnectDelay), connect);
    _reconnectDelay = (_reconnectDelay * 2).clamp(1000, 10000);
  }

  Future<void> _checkAuthAndMaybeRedirect() async {
    try {
      final data = await const ApiClient().get('/auth/status')
          as Map<String, dynamic>;
      final authenticated = data['authenticated'] as bool? ?? false;
      if (!authenticated) {
        web.window.location.href = '/login';
        return;
      }
    } catch (_) {
      // Network error — keep trying
    }
    // Still authenticated, keep reconnecting
    _reconnectTimer = Timer(Duration(milliseconds: _reconnectDelay), connect);
    _reconnectDelay = (_reconnectDelay * 2).clamp(1000, 10000);
  }

  /// Send a client message
  void send(ClientMessage msg) {
    if (_channel == null) return;
    _channel!.sink.add(jsonEncode(msg.toJson()));
  }

  /// Send raw input data for a session
  void sendInput(String data, {String? session}) {
    send(InputMessage(data: data, session: session));
  }

  /// Attach to a session and track it for reconnection
  void attachSession(String name, {int cols = 120, int rows = 40}) {
    _attachedSessions[name] = (cols: cols, rows: rows);
    send(AttachMessage(session: name, cols: cols, rows: rows));
  }

  /// Detach from a session
  void detachSession(String name) {
    _attachedSessions.remove(name);
    send(DetachMessage(session: name));
  }

  /// Resize a session
  void resizeSession(String name, int cols, int rows) {
    _attachedSessions[name] = (cols: cols, rows: rows);
    send(ResizeMessage(cols: cols, rows: rows, session: name));
  }

  /// Handle visibility changes (reconnect after backgrounding)
  void onVisibilityChange(bool hidden) {
    if (hidden) {
      _hiddenAt = DateTime.now();
    } else if (_hiddenAt != null) {
      final duration = DateTime.now().difference(_hiddenAt!);
      if (duration.inSeconds > 5 && _channel != null) {
        _channel!.sink.close();
      }
      _hiddenAt = null;
    }
  }
}
