import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'api_client.dart';

/// Server-side instance configuration.
class ConfigState {
  final String instanceName;

  const ConfigState({this.instanceName = ''});
}

/// Config service provider.
final configProvider =
    AsyncNotifierProvider<ConfigNotifier, ConfigState>(ConfigNotifier.new);

class ConfigNotifier extends AsyncNotifier<ConfigState> {
  final _api = const ApiClient();

  @override
  Future<ConfigState> build() async {
    return _fetchConfig();
  }

  Future<ConfigState> _fetchConfig() async {
    final data = await _api.get('/api/config') as Map<String, dynamic>;
    final config = data['config'] as Map<String, dynamic>;
    return ConfigState(
      instanceName: config['instanceName'] as String? ?? '',
    );
  }

  Future<void> setInstanceName(String name) async {
    await _api.put('/api/config/instance-name', {'instanceName': name});
    state = AsyncData(await _fetchConfig());
  }
}
