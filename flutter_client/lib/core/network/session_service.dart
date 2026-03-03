import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'api_client.dart';

enum SessionStatus { running, exited }

/// A server-side session returned from the REST API.
class SessionInfo {
  final String name;
  final SessionStatus status;
  final String? bundlePath;
  final bool dirty;

  const SessionInfo({
    required this.name,
    required this.status,
    this.bundlePath,
    this.dirty = false,
  });

  factory SessionInfo.fromJson(Map<String, dynamic> json) => SessionInfo(
        name: json['name'] as String,
        status: (json['alive'] as bool? ?? true)
            ? SessionStatus.running
            : SessionStatus.exited,
        bundlePath: json['bundlePath'] as String?,
        dirty: json['dirty'] as bool? ?? false,
      );

  bool get isRunning => status == SessionStatus.running;

  /// Whether this session has been saved to a .abot bundle.
  bool get isSaved => bundlePath != null;
}

/// Session service provider.
final sessionServiceProvider =
    AsyncNotifierProvider<SessionServiceNotifier, List<SessionInfo>>(
        SessionServiceNotifier.new);

class SessionServiceNotifier extends AsyncNotifier<List<SessionInfo>> {
  final _api = const ApiClient();

  @override
  Future<List<SessionInfo>> build() async {
    return listSessions();
  }

  /// List all sessions from the server.
  Future<List<SessionInfo>> listSessions() async {
    final data = await _api.get('/sessions');
    if (data is List) {
      return data
          .map((e) => SessionInfo.fromJson(e as Map<String, dynamic>))
          .toList();
    }
    return [];
  }

  /// Create a new session.
  Future<SessionInfo> createSession(String name) async {
    final data =
        await _api.post('/sessions', {'name': name}) as Map<String, dynamic>;
    final session = SessionInfo.fromJson(data);
    // Refresh the list
    state = AsyncData(await listSessions());
    return session;
  }

  /// Rename an existing session.
  Future<void> renameSession(String oldName, String newName) async {
    await _api.put('/sessions/${Uri.encodeComponent(oldName)}', {'name': newName});
    state = AsyncData(await listSessions());
  }

  /// Delete a session.
  Future<void> deleteSession(String name) async {
    await _api.delete('/sessions/${Uri.encodeComponent(name)}');
    state = AsyncData(await listSessions());
  }

  /// Open a .abot bundle as a new session.
  Future<Map<String, dynamic>> openBundle(String path) async {
    final data =
        await _api.post('/sessions/open', {'path': path}) as Map<String, dynamic>;
    state = AsyncData(await listSessions());
    return data;
  }

  /// Save session to its tracked bundle path.
  Future<Map<String, dynamic>> saveSession(String name) async {
    final data = await _api.post(
        '/sessions/${Uri.encodeComponent(name)}/save', {}) as Map<String, dynamic>;
    state = AsyncData(await listSessions());
    return data;
  }

  /// Save session to a new bundle path.
  Future<Map<String, dynamic>> saveSessionAs(String name, String path) async {
    final data = await _api.post(
        '/sessions/${Uri.encodeComponent(name)}/save-as',
        {'path': path}) as Map<String, dynamic>;
    state = AsyncData(await listSessions());
    return data;
  }

  /// Close session (optionally save first).
  Future<void> closeSession(String name, {bool save = false}) async {
    await _api.post(
        '/sessions/${Uri.encodeComponent(name)}/close', {'save': save});
    state = AsyncData(await listSessions());
  }

  /// Refresh the session list.
  Future<void> refresh() async {
    state = AsyncData(await listSessions());
  }
}
