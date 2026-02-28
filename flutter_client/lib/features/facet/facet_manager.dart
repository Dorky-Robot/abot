import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'facet.dart';

/// State for the facet manager — Stage Manager model.
/// One focused facet takes center stage; the rest live in a side strip.
class FacetManagerState {
  final Map<String, FacetData> facets;

  /// Non-focused facet IDs in most-recent-first order (for the side strip).
  final List<String> stripOrder;
  final String? focusedId;

  const FacetManagerState({
    this.facets = const {},
    this.stripOrder = const [],
    this.focusedId,
  });

  FacetManagerState copyWith({
    Map<String, FacetData>? facets,
    List<String>? stripOrder,
    Object? focusedId = _sentinel,
  }) =>
      FacetManagerState(
        facets: facets ?? this.facets,
        stripOrder: stripOrder ?? this.stripOrder,
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

  /// Flat list: focused first, then strip order (for shortcut bar tabs).
  List<FacetData> get orderedFacets {
    final result = <FacetData>[];
    if (focusedId != null) {
      final f = facets[focusedId];
      if (f != null) result.add(f);
    }
    for (final id in stripOrder) {
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

  /// Create a new facet for a session — becomes focused, previous focused
  /// moves to the top of the strip.
  FacetData create(String sessionName) {
    final id = 'facet-${_nextId++}';
    final facet = FacetData(id: id, sessionName: sessionName);
    final newFacets = Map<String, FacetData>.from(state.facets);
    newFacets[id] = facet;

    // Push previously-focused to top of strip
    final newStrip = List<String>.from(state.stripOrder);
    if (state.focusedId != null) {
      newStrip.insert(0, state.focusedId!);
    }

    state = state.copyWith(
      facets: newFacets,
      stripOrder: newStrip,
      focusedId: id,
    );
    return facet;
  }

  /// Remove a facet by ID (must have >1 facet).
  void remove(String facetId) {
    if (state.facets.length <= 1) return;

    final newFacets = Map<String, FacetData>.from(state.facets);
    newFacets.remove(facetId);

    final newStrip = List<String>.from(state.stripOrder);
    newStrip.remove(facetId);

    String? newFocused = state.focusedId;
    if (newFocused == facetId) {
      // Promote first strip item
      newFocused = newStrip.isNotEmpty ? newStrip.removeAt(0) : null;
    }

    state = state.copyWith(
      facets: newFacets,
      stripOrder: newStrip,
      focusedId: newFocused,
    );
  }

  /// Focus a specific facet — swap it with the current focused.
  void focus(String facetId) {
    if (!state.facets.containsKey(facetId)) return;
    if (facetId == state.focusedId) return;

    final newStrip = List<String>.from(state.stripOrder);
    final idx = newStrip.indexOf(facetId);

    // Remove the newly focused from strip
    if (idx != -1) {
      newStrip.removeAt(idx);
    }

    // Push current focused into strip at the position the new focus was at
    if (state.focusedId != null) {
      final insertAt = idx != -1 ? idx : 0;
      newStrip.insert(insertAt, state.focusedId!);
    }

    state = state.copyWith(
      stripOrder: newStrip,
      focusedId: facetId,
    );
  }

  /// Cycle focus: current focused goes to end of strip,
  /// first strip item becomes focused.
  void cycleFocus() {
    if (state.stripOrder.isEmpty) return;

    final newStrip = List<String>.from(state.stripOrder);
    final newFocused = newStrip.removeAt(0);

    if (state.focusedId != null) {
      newStrip.add(state.focusedId!);
    }

    state = state.copyWith(
      stripOrder: newStrip,
      focusedId: newFocused,
    );
  }
}
