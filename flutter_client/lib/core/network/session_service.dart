import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'api_client.dart';

/// A server-side session returned from the REST API.
class SessionInfo {
  final String name;
  final String status;

  const SessionInfo({required this.name, required this.status});

  factory SessionInfo.fromJson(Map<String, dynamic> json) => SessionInfo(
        name: json['name'] as String,
        status: (json['status'] as String?) ?? 'running',
      );
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
    await _api.put('/sessions/$oldName', {'name': newName});
    state = AsyncData(await listSessions());
  }

  /// Delete a session.
  Future<void> deleteSession(String name) async {
    await _api.delete('/sessions/$name');
    state = AsyncData(await listSessions());
  }

  /// Refresh the session list.
  Future<void> refresh() async {
    state = AsyncData(await listSessions());
  }
}
