import 'dart:convert';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import '../../core/network/kubo_service.dart';
import '../../core/network/websocket_service.dart';
import 'facet.dart';

/// State for the facet manager — Stage Manager model.
/// All facets live in a single ordered list. One is focused (expanded to
/// center stage); the rest appear as cards in the sidebar at their stable
/// positions.
class FacetManagerState {
  final Map<String, FacetData> facets;

  /// All facet IDs in their stable sidebar order.
  final List<String> order;
  final String? focusedId;

  const FacetManagerState({
    this.facets = const {},
    this.order = const [],
    this.focusedId,
  });

  FacetManagerState copyWith({
    Map<String, FacetData>? facets,
    List<String>? order,
    Object? focusedId = _sentinel,
  }) =>
      FacetManagerState(
        facets: facets ?? this.facets,
        order: order ?? this.order,
        focusedId: focusedId == _sentinel
            ? this.focusedId
            : focusedId as String?,
      );

  static const Object _sentinel = Object();

  FacetData? get focused => facets[focusedId];

  FacetData? getBySession(String sessionName) {
    for (final f in facets.values) {
      if (f.sessionName == sessionName) return f;
    }
    return null;
  }

  /// Non-focused facet IDs in order (for sidebar strip cards).
  List<String> get stripOrder =>
      order.where((id) => id != focusedId).toList();

  /// All facets in order (focused first, then the rest).
  List<FacetData> get orderedFacets {
    final result = <FacetData>[];
    if (focusedId != null) {
      final f = facets[focusedId];
      if (f != null) result.add(f);
    }
    for (final id in order) {
      if (id == focusedId) continue;
      final f = facets[id];
      if (f != null) result.add(f);
    }
    return result;
  }

  int get count => facets.length;
}

/// Facet manager provider
final facetManagerProvider =
    NotifierProvider<FacetManagerNotifier, FacetManagerState>(
        FacetManagerNotifier.new);

class FacetManagerNotifier extends Notifier<FacetManagerState> {
  int _nextId = 0;

  @override
  FacetManagerState build() => const FacetManagerState();

  /// Create a named abot in a kubo: canonical abot + worktree + session + facet.
  /// Returns the facet if a new session was created, null if the abot was only
  /// employed (worktree created) without a new session (e.g. abot already has
  /// a session in another kubo).
  Future<FacetData?> createAbotInKubo(String abotName, {required String kubo}) async {
    final result = await ref.read(kuboServiceProvider.notifier).addAbotToKubo(
      kubo,
      abotName,
      createSession: true,
    );

    // If no session was created (abot already has one in another kubo),
    // just return null — the worktree and manifest were still set up.
    final sessionName = result['session'] as String?;
    if (sessionName == null) return null;

    // Remove any existing facet for this session (backend killed the old one)
    final existing = state.getBySession(sessionName);
    if (existing != null) {
      ref.read(wsServiceProvider.notifier).detachSession(sessionName);
      remove(existing.id);
    }

    final facet = create(sessionName);
    ref.read(wsServiceProvider.notifier).attachSession(sessionName);
    return facet;
  }

  /// Detach a facet's session via WS and remove the facet (session stays alive).
  void minimizeSession(String facetId) {
    final facet = state.facets[facetId];
    if (facet == null) return;
    ref.read(wsServiceProvider.notifier).detachSession(facet.sessionName);
    remove(facetId);
  }

  /// Update session name in all facets referencing the old name.
  void renameSessionInFacets(String oldName, String newName) {
    final newFacets = Map<String, FacetData>.from(state.facets);
    var changed = false;
    for (final entry in newFacets.entries) {
      if (entry.value.sessionName == oldName) {
        newFacets[entry.key] = entry.value.copyWith(sessionName: newName);
        changed = true;
      }
    }
    if (changed) {
      state = state.copyWith(facets: newFacets);
    }
  }

  /// Focus an existing facet for a session, or create one and attach.
  void openOrFocusSession(String sessionName) {
    final existing = state.getBySession(sessionName);
    if (existing != null) {
      focus(existing.id);
    } else {
      create(sessionName);
      ref.read(wsServiceProvider.notifier).attachSession(sessionName);
    }
  }

  /// Create a new facet for a session — becomes focused, appended to end
  /// of the order list.
  FacetData create(String sessionName) {
    final id = 'facet-${_nextId++}';
    final facet = FacetData(id: id, sessionName: sessionName);
    final newFacets = Map<String, FacetData>.from(state.facets);
    newFacets[id] = facet;

    final newOrder = List<String>.from(state.order);
    newOrder.add(id);

    state = state.copyWith(
      facets: newFacets,
      order: newOrder,
      focusedId: id,
    );
    _persistOrder();
    _persistFocused();
    return facet;
  }

  /// Remove a facet by ID.
  void remove(String facetId) {
    final newFacets = Map<String, FacetData>.from(state.facets);
    newFacets.remove(facetId);

    final newOrder = List<String>.from(state.order);
    newOrder.remove(facetId);

    String? newFocused = state.focusedId;
    if (newFocused == facetId) {
      // Promote the next item in order, or the previous one
      newFocused = newOrder.isNotEmpty ? newOrder.first : null;
    }

    state = state.copyWith(
      facets: newFacets,
      order: newOrder,
      focusedId: newFocused,
    );
    _persistOrder();
    _persistFocused();
  }

  /// Focus a specific facet — just changes focusedId, order is stable.
  void focus(String facetId) {
    if (!state.facets.containsKey(facetId)) return;
    if (facetId == state.focusedId) return;
    state = state.copyWith(focusedId: facetId);
    _persistFocused();
  }

  /// Clear focus — no facet is focused, shows landing page.
  void unfocus() {
    if (state.focusedId == null) return;
    state = state.copyWith(focusedId: null);
    _persistFocused();
  }

  /// Persist current facet order as session names to localStorage.
  void _persistOrder() {
    final names = state.order
        .map((id) => state.facets[id]?.sessionName)
        .whereType<String>()
        .toList();
    web.window.localStorage
        .setItem('abot_facet_order', jsonEncode(names));
  }

  /// Persist the focused session name to localStorage.
  void _persistFocused() {
    final name = state.focusedId != null
        ? state.facets[state.focusedId]?.sessionName
        : null;
    if (name != null) {
      web.window.localStorage.setItem('abot_focused_session', name);
    } else {
      web.window.localStorage.removeItem('abot_focused_session');
    }
  }

  /// Restore persisted sidebar order after facets have been created.
  void loadPersistedOrder() {
    final raw = web.window.localStorage.getItem('abot_facet_order');
    if (raw == null) return;

    final List<String> savedNames;
    try {
      savedNames = (jsonDecode(raw) as List).cast<String>();
    } catch (_) {
      return;
    }

    // Build a name→id lookup from current facets.
    final nameToId = <String, String>{};
    for (final entry in state.facets.entries) {
      nameToId[entry.value.sessionName] = entry.key;
    }

    // Saved sessions first (in saved order), then any new sessions appended.
    final newOrder = <String>[];
    final placed = <String>{};
    for (final name in savedNames) {
      final id = nameToId[name];
      if (id != null && !placed.contains(id)) {
        newOrder.add(id);
        placed.add(id);
      }
    }
    for (final id in state.order) {
      if (!placed.contains(id)) {
        newOrder.add(id);
      }
    }

    state = state.copyWith(order: newOrder);
    _persistOrder();

    // Restore focused session.
    final focusedName =
        web.window.localStorage.getItem('abot_focused_session');
    if (focusedName != null) {
      final focusedId = nameToId[focusedName];
      if (focusedId != null) {
        state = state.copyWith(focusedId: focusedId);
      }
    }
  }
}
