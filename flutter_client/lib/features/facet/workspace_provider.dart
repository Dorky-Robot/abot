import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;

/// Workspace state — which kubos are open and which is active.
/// Persisted to localStorage so it survives page reloads.
class WorkspaceState {
  final String? activeKubo;
  final Set<String> openKubos;

  const WorkspaceState({this.activeKubo, this.openKubos = const {}});

  WorkspaceState copyWith({
    Object? activeKubo = _sentinel,
    Set<String>? openKubos,
  }) =>
      WorkspaceState(
        activeKubo:
            activeKubo == _sentinel ? this.activeKubo : activeKubo as String?,
        openKubos: openKubos ?? this.openKubos,
      );

  static const Object _sentinel = Object();
}

final workspaceProvider =
    NotifierProvider<WorkspaceNotifier, WorkspaceState>(WorkspaceNotifier.new);

class WorkspaceNotifier extends Notifier<WorkspaceState> {
  static const _activeKuboKey = 'abot_active_kubo';
  static const _openKubosKey = 'abot_open_kubos';

  @override
  WorkspaceState build() {
    final stored = web.window.localStorage.getItem(_activeKuboKey);
    final openJson = web.window.localStorage.getItem(_openKubosKey);

    Set<String> openKubos = {};
    if (openJson != null) {
      try {
        openKubos = (jsonDecode(openJson) as List).cast<String>().toSet();
      } catch (e) {
        debugPrint('[WorkspaceProvider] Failed to parse open kubos: $e');
      }
    }

    return WorkspaceState(
      activeKubo: (stored != null && stored.isNotEmpty) ? stored : null,
      openKubos: openKubos,
    );
  }

  void setActiveKubo(String? kubo) {
    state = state.copyWith(activeKubo: kubo);
    if (kubo != null) {
      web.window.localStorage.setItem(_activeKuboKey, kubo);
    } else {
      web.window.localStorage.removeItem(_activeKuboKey);
    }
  }

  void openKubo(String name) {
    final newOpen = {...state.openKubos, name};
    state = state.copyWith(openKubos: newOpen);
    _persistOpen(newOpen);
    setActiveKubo(name);
  }

  void pruneStale(Set<String> serverKuboNames) {
    final stale = state.openKubos.difference(serverKuboNames);
    if (stale.isEmpty) return;

    final newOpen = state.openKubos.difference(stale);
    final newActive = (state.activeKubo != null &&
            serverKuboNames.contains(state.activeKubo))
        ? state.activeKubo
        : (newOpen.isNotEmpty ? newOpen.first : null);

    state = state.copyWith(openKubos: newOpen);
    _persistOpen(newOpen);
    setActiveKubo(newActive);
  }

  void _persistOpen(Set<String> kubos) {
    web.window.localStorage
        .setItem(_openKubosKey, jsonEncode(kubos.toList()));
  }
}
