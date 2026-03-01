import 'dart:convert';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
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
    return facet;
  }

  /// Remove a facet by ID (must have >1 facet).
  void remove(String facetId) {
    if (state.facets.length <= 1) return;

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
  }

  /// Focus a specific facet — just changes focusedId, order is stable.
  void focus(String facetId) {
    if (!state.facets.containsKey(facetId)) return;
    if (facetId == state.focusedId) return;
    state = state.copyWith(focusedId: facetId);
  }

  /// Reorder: move facet from [oldIndex] to [newIndex] in the full order list.
  void reorder(int oldIndex, int newIndex) {
    final newOrder = List<String>.from(state.order);
    if (oldIndex < 0 || oldIndex >= newOrder.length) return;
    if (newIndex < 0 || newIndex > newOrder.length) return;

    final id = newOrder.removeAt(oldIndex);
    final insertAt = newIndex > oldIndex ? newIndex - 1 : newIndex;
    newOrder.insert(insertAt, id);

    state = state.copyWith(order: newOrder);
    _persistOrder();
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
  }
}
