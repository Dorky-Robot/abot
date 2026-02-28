import 'package:flutter_riverpod/flutter_riverpod.dart';
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
  }

  /// Reorder within the strip (non-focused items only).
  /// Converts strip indices to full order indices.
  void reorderStrip(int oldStripIndex, int newStripIndex) {
    final strip = state.stripOrder;
    if (oldStripIndex < 0 || oldStripIndex >= strip.length) return;
    if (newStripIndex < 0 || newStripIndex > strip.length) return;

    final movedId = strip[oldStripIndex];
    final targetId = newStripIndex < strip.length
        ? strip[newStripIndex > oldStripIndex ? newStripIndex : newStripIndex]
        : null;

    final newOrder = List<String>.from(state.order);
    newOrder.remove(movedId);

    if (targetId != null) {
      final targetIdx = newOrder.indexOf(targetId);
      if (newStripIndex > oldStripIndex) {
        newOrder.insert(targetIdx + 1, movedId);
      } else {
        newOrder.insert(targetIdx, movedId);
      }
    } else {
      newOrder.add(movedId);
    }

    state = state.copyWith(order: newOrder);
  }

  /// Cycle focus to the next facet in order.
  void cycleFocus() {
    if (state.order.length <= 1) return;
    final currentIdx = state.order.indexOf(state.focusedId ?? '');
    final nextIdx = (currentIdx + 1) % state.order.length;
    state = state.copyWith(focusedId: state.order[nextIdx]);
  }
}
