/// WebSocket message types matching src/stream/messages.rs (flat protocol)
///
/// We use the flat protocol for simplicity — same as the vanilla JS client.
sealed class ClientMessage {
  Map<String, dynamic> toJson();
}

class AttachMessage extends ClientMessage {
  final String session;
  final int cols;
  final int rows;

  AttachMessage({required this.session, this.cols = 120, this.rows = 40});

  @override
  Map<String, dynamic> toJson() => {
        'type': 'attach',
        'session': session,
        'cols': cols,
        'rows': rows,
      };
}

class InputMessage extends ClientMessage {
  final String data;
  final String? session;

  InputMessage({required this.data, this.session});

  @override
  Map<String, dynamic> toJson() => {
        'type': 'input',
        'data': data,
        if (session != null) 'session': session,
      };
}

class ResizeMessage extends ClientMessage {
  final int cols;
  final int rows;
  final String? session;

  ResizeMessage({required this.cols, required this.rows, this.session});

  @override
  Map<String, dynamic> toJson() => {
        'type': 'resize',
        'cols': cols,
        'rows': rows,
        if (session != null) 'session': session,
      };
}

class DetachMessage extends ClientMessage {
  final String? session;

  DetachMessage({this.session});

  @override
  Map<String, dynamic> toJson() => {
        'type': 'detach',
        if (session != null) 'session': session,
      };
}

/// Server messages parsed from JSON
sealed class ServerMessage {
  factory ServerMessage.fromJson(Map<String, dynamic> json) {
    return switch (json['type'] as String?) {
      'attached' => AttachedMessage(
          session: json['session'] as String,
          buffer: json['buffer'] as String? ?? '',
        ),
      'output' => OutputMessage(
          data: json['data'] as String,
          session: json['session'] as String?,
        ),
      'exit' => ExitMessage(
          code: json['code'] as int? ?? 0,
          session: json['session'] as String?,
        ),
      'session-removed' => SessionRemovedMessage(
          session: json['session'] as String,
        ),
      'p2p-signal' => P2pSignalMessage(data: json['data']),
      'p2p-ready' => const P2pReadyMessage(),
      'p2p-closed' => const P2pClosedMessage(),
      'server-draining' => const ServerDrainingMessage(),
      'reload' => const ReloadMessage(),
      'error' => ErrorMessage(message: json['message'] as String? ?? ''),
      _ => UnknownMessage(type: json['type'] as String? ?? 'null'),
    };
  }
}

class AttachedMessage implements ServerMessage {
  final String session;
  final String buffer;
  const AttachedMessage({required this.session, required this.buffer});
}

class OutputMessage implements ServerMessage {
  final String data;
  final String? session;
  const OutputMessage({required this.data, this.session});
}

class ExitMessage implements ServerMessage {
  final int code;
  final String? session;
  const ExitMessage({required this.code, this.session});
}

class SessionRemovedMessage implements ServerMessage {
  final String session;
  const SessionRemovedMessage({required this.session});
}

class P2pSignalMessage implements ServerMessage {
  final dynamic data;
  const P2pSignalMessage({this.data});
}

class P2pReadyMessage implements ServerMessage {
  const P2pReadyMessage();
}

class P2pClosedMessage implements ServerMessage {
  const P2pClosedMessage();
}

class ServerDrainingMessage implements ServerMessage {
  const ServerDrainingMessage();
}

class ReloadMessage implements ServerMessage {
  const ReloadMessage();
}

class ErrorMessage implements ServerMessage {
  final String message;
  const ErrorMessage({required this.message});
}

class UnknownMessage implements ServerMessage {
  final String type;
  const UnknownMessage({required this.type});
}
