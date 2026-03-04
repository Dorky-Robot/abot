import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'api_client.dart';

/// A kubo (shared runtime room) returned from the REST API.
class KuboInfo {
  final String name;
  final bool running;
  final int activeSessions;

  const KuboInfo({
    required this.name,
    required this.running,
    this.activeSessions = 0,
  });

  factory KuboInfo.fromJson(Map<String, dynamic> json) => KuboInfo(
        name: json['name'] as String,
        running: json['running'] as bool? ?? false,
        activeSessions: json['activeSessions'] as int? ?? 0,
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

  /// Refresh the kubo list.
  Future<void> refresh() async {
    state = AsyncData(await listKubos());
  }
}
