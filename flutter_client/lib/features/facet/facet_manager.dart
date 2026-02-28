import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'facet.dart';

/// State for the facet manager
class FacetManagerState {
  final List<FacetData> facets;
  final String? focusedId;

  const FacetManagerState({
    this.facets = const [],
    this.focusedId,
  });

  FacetManagerState copyWith({
    List<FacetData>? facets,
    String? focusedId,
  }) =>
      FacetManagerState(
        facets: facets ?? this.facets,
        focusedId: focusedId ?? this.focusedId,
      );

  FacetData? get focused =>
      facets.where((f) => f.id == focusedId).firstOrNull;

  FacetData? getBySession(String sessionName) =>
      facets.where((f) => f.sessionName == sessionName).firstOrNull;

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

  /// Create a new facet for a session
  FacetData create(String sessionName) {
    final id = 'facet-${_nextId++}';
    final facet = FacetData(
      id: id,
      sessionName: sessionName,
      columnIndex: state.facets.length,
    );
    state = state.copyWith(
      facets: [...state.facets, facet],
      focusedId: id,
    );
    return facet;
  }

  /// Remove a facet by ID (must have >1 facet)
  void remove(String facetId) {
    if (state.facets.length <= 1) return;
    final newFacets = state.facets.where((f) => f.id != facetId).toList();
    String? newFocused = state.focusedId;
    if (newFocused == facetId) {
      newFocused = newFacets.isNotEmpty ? newFacets.last.id : null;
    }
    state = state.copyWith(facets: newFacets, focusedId: newFocused);
  }

  /// Focus a specific facet
  void focus(String facetId) {
    if (state.facets.any((f) => f.id == facetId)) {
      state = state.copyWith(focusedId: facetId);
    }
  }

  /// Cycle focus to the next facet
  void cycleFocus() {
    if (state.facets.isEmpty) return;
    final currentIdx =
        state.facets.indexWhere((f) => f.id == state.focusedId);
    final nextIdx = (currentIdx + 1) % state.facets.length;
    state = state.copyWith(focusedId: state.facets[nextIdx].id);
  }
}
