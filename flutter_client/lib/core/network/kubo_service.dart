import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'api_client.dart';

/// A kubo (shared runtime room) returned from the REST API.
class KuboInfo {
  final String name;
  final String path;
  final bool running;
  final int activeSessions;
  final List<String> abots;

  const KuboInfo({
    required this.name,
    this.path = '',
    required this.running,
    this.activeSessions = 0,
    this.abots = const [],
  });

  factory KuboInfo.fromJson(Map<String, dynamic> json) => KuboInfo(
        name: json['name'] as String,
        path: json['path'] as String? ?? '',
        running: json['running'] as bool? ?? false,
        activeSessions: json['activeSessions'] as int? ?? 0,
        abots: (json['abots'] as List?)?.cast<String>() ?? [],
      );
}

/// Kubo service provider.
final kuboServiceProvider =
    AsyncNotifierProvider<KuboServiceNotifier, List<KuboInfo>>(
        KuboServiceNotifier.new);

class KuboServiceNotifier extends AsyncNotifier<List<KuboInfo>> {
  final _api = const ApiClient();

  @override
  Future<List<KuboInfo>> build() async {
    return listKubos();
  }

  /// List all kubos from the server.
  Future<List<KuboInfo>> listKubos() async {
    final data = await _api.get('/kubos');
    if (data is List) {
      return data
          .map((e) => KuboInfo.fromJson(e as Map<String, dynamic>))
          .toList();
    }
    return [];
  }

  /// Create a new kubo.
  Future<void> createKubo(String name) async {
    await _api.post('/kubos', {'name': name});
    state = AsyncData(await listKubos());
  }

  /// Open a kubo from a path on disk.
  Future<Map<String, dynamic>> openKubo(String path) async {
    final data = await _api.post('/kubos/open', {'path': path});
    state = AsyncData(await listKubos());
    return data as Map<String, dynamic>;
  }

  /// Start a kubo container.
  Future<void> startKubo(String name) async {
    await _api.post('/kubos/${Uri.encodeComponent(name)}/start', {});
    state = AsyncData(await listKubos());
  }

  /// Stop a kubo container.
  Future<void> stopKubo(String name) async {
    await _api.post('/kubos/${Uri.encodeComponent(name)}/stop', {});
    state = AsyncData(await listKubos());
  }

  /// Add an abot to a kubo. When [createSession] is true, also creates a
  /// terminal session and returns the response (including session name).
  Future<Map<String, dynamic>> addAbotToKubo(
    String kuboName,
    String abotName, {
    bool createSession = false,
    int cols = 120,
    int rows = 40,
  }) async {
    final body = <String, dynamic>{
      'abot': abotName,
      'createSession': createSession,
      'cols': cols,
      'rows': rows,
    };
    final data = await _api.post('/kubos/${Uri.encodeComponent(kuboName)}/abots', body);
    // Refresh kubo list to pick up new abot count
    state = AsyncData(await listKubos());
    return data as Map<String, dynamic>;
  }

  /// Remove an abot from a kubo (close session, remove worktree).
  Future<void> removeAbotFromKubo(String kuboName, String abotName) async {
    await _api.delete(
      '/kubos/${Uri.encodeComponent(kuboName)}/abots/${Uri.encodeComponent(abotName)}',
    );
    state = AsyncData(await listKubos());
  }

  /// Refresh the kubo list.
  Future<void> refresh() async {
    state = AsyncData(await listKubos());
  }
}
