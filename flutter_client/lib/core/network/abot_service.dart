import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'api_client.dart';
import 'kubo_service.dart';
import 'session_service.dart';

/// A known abot from the registry (now includes detail inline).
class AbotInfo {
  final String name;
  final String path;
  final String? createdAt;
  final String defaultBranch;
  final List<KuboBranchInfo> kuboBranches;
  final String gitStatus;

  const AbotInfo({
    required this.name,
    this.path = '',
    this.createdAt,
    this.defaultBranch = 'main',
    this.kuboBranches = const [],
    this.gitStatus = '',
  });

  factory AbotInfo.fromJson(Map<String, dynamic> json) => AbotInfo(
        name: json['name'] as String,
        path: json['path'] as String? ?? '',
        createdAt: json['createdAt'] as String?,
        defaultBranch: json['defaultBranch'] as String? ?? 'main',
        kuboBranches: (json['kuboBranches'] as List?)
                ?.map((e) =>
                    KuboBranchInfo.fromJson(e as Map<String, dynamic>))
                .toList() ??
            [],
        gitStatus: json['gitStatus'] as String? ?? '',
      );
}

/// A kubo branch in an abot's git repo.
class KuboBranchInfo {
  final String kuboName;
  final String branch;
  final bool hasWorktree;
  final bool hasSession;

  const KuboBranchInfo({
    required this.kuboName,
    required this.branch,
    this.hasWorktree = false,
    this.hasSession = false,
  });

  factory KuboBranchInfo.fromJson(Map<String, dynamic> json) =>
      KuboBranchInfo(
        kuboName: json['kuboName'] as String,
        branch: json['branch'] as String,
        hasWorktree: json['hasWorktree'] as bool? ?? false,
        hasSession: json['hasSession'] as bool? ?? false,
      );
}

/// Abot service provider.
final abotServiceProvider =
    AsyncNotifierProvider<AbotServiceNotifier, List<AbotInfo>>(
        AbotServiceNotifier.new);

class AbotServiceNotifier extends AsyncNotifier<List<AbotInfo>> {
  final _api = const ApiClient();

  @override
  Future<List<AbotInfo>> build() async {
    return listAbots();
  }

  /// List all known abots (with detail inline).
  Future<List<AbotInfo>> listAbots() async {
    final data = await _api.get('/abots');
    if (data is Map && data['abots'] is List) {
      return (data['abots'] as List)
          .map((e) => AbotInfo.fromJson(e as Map<String, dynamic>))
          .toList();
    }
    return [];
  }

  /// Remove an abot from the known list.
  Future<void> removeAbot(String name) async {
    await _api.delete('/abots/${Uri.encodeComponent(name)}');
    state = AsyncData(await listAbots());
  }

  /// Dismiss a kubo variant (remove worktree, keep branch as past work).
  Future<void> dismissVariant(String abotName, String kuboName) async {
    await _api.post('/abots/${Uri.encodeComponent(abotName)}/dismiss', {
      'kubo': kuboName,
    });
    state = AsyncData(await listAbots());
    ref.invalidate(kuboServiceProvider);
    ref.invalidate(sessionServiceProvider);
  }

  /// Integrate a kubo variant into the abot's default branch.
  Future<void> integrateVariant(String abotName, String kuboName) async {
    await _api.post('/abots/${Uri.encodeComponent(abotName)}/integrate', {
      'kubo': kuboName,
    });
    state = AsyncData(await listAbots());
    ref.invalidate(kuboServiceProvider);
  }

  /// Discard a kubo variant (delete branch + worktree).
  Future<void> discardVariant(String abotName, String kuboName) async {
    await _api.post('/abots/${Uri.encodeComponent(abotName)}/discard', {
      'kubo': kuboName,
    });
    state = AsyncData(await listAbots());
    ref.invalidate(kuboServiceProvider);
  }

  /// Refresh the abots list.
  Future<void> refresh() async {
    state = AsyncData(await listAbots());
  }
}
