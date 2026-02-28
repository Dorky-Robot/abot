import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'facet.dart';

/// State for the facet manager — column-major tiling model.
class FacetManagerState {
  final Map<String, FacetData> facets;
  final List<List<String>> columns;
  final String? focusedId;

  const FacetManagerState({
    this.facets = const {},
    this.columns = const [],
    this.focusedId,
  });

  FacetManagerState copyWith({
    Map<String, FacetData>? facets,
    List<List<String>>? columns,
    Object? focusedId = _sentinel,
  }) =>
      FacetManagerState(
        facets: facets ?? this.facets,
        columns: columns ?? this.columns,
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

  /// Flat list of all facet data in column-major order.
  List<FacetData> get orderedFacets {
    final result = <FacetData>[];
    for (final col in columns) {
      for (final id in col) {
        final f = facets[id];
        if (f != null) result.add(f);
      }
    }
    return result;
  }

  int get count => facets.length;

  /// Find which column a facet belongs to: returns (col, row) or null.
  ({int col, int row})? findFacet(String facetId) {
    for (int c = 0; c < columns.length; c++) {
      final r = columns[c].indexOf(facetId);
      if (r != -1) return (col: c, row: r);
    }
    return null;
  }
}

/// Facet manager provider
final facetManagerProvider =
    NotifierProvider<FacetManagerNotifier, FacetManagerState>(
        FacetManagerNotifier.new);

class FacetManagerNotifier extends Notifier<FacetManagerState> {
  int _nextId = 0;

  @override
  FacetManagerState build() => const FacetManagerState();

  /// Create a new facet for a session (always adds a new column).
  FacetData create(String sessionName) {
    final id = 'facet-${_nextId++}';
    final facet = FacetData(id: id, sessionName: sessionName);
    final newFacets = Map<String, FacetData>.from(state.facets);
    newFacets[id] = facet;
    final newColumns = [
      for (final col in state.columns) List<String>.from(col),
      [id],
    ];
    state = state.copyWith(
      facets: newFacets,
      columns: newColumns,
      focusedId: id,
    );
    return facet;
  }

  /// Remove a facet by ID (must have >1 facet).
  void remove(String facetId) {
    if (state.facets.length <= 1) return;

    final newFacets = Map<String, FacetData>.from(state.facets);
    newFacets.remove(facetId);

    final newColumns = <List<String>>[];
    for (final col in state.columns) {
      final filtered = col.where((id) => id != facetId).toList();
      if (filtered.isNotEmpty) newColumns.add(filtered);
    }

    String? newFocused = state.focusedId;
    if (newFocused == facetId) {
      // Focus the last facet in column-major order
      final allIds = newColumns.expand((col) => col).toList();
      newFocused = allIds.isNotEmpty ? allIds.last : null;
    }

    state = state.copyWith(
      facets: newFacets,
      columns: newColumns,
      focusedId: newFocused,
    );
  }

  /// Focus a specific facet.
  void focus(String facetId) {
    if (state.facets.containsKey(facetId)) {
      state = state.copyWith(focusedId: facetId);
    }
  }

  /// Cycle focus to the next facet (column-major order).
  void cycleFocus() {
    final ordered = state.orderedFacets;
    if (ordered.isEmpty) return;
    final currentIdx = ordered.indexWhere((f) => f.id == state.focusedId);
    final nextIdx = (currentIdx + 1) % ordered.length;
    state = state.copyWith(focusedId: ordered[nextIdx].id);
  }

  /// Swap the columns containing draggedId and targetId.
  void reorderColumns(String draggedId, String targetId) {
    final dragPos = state.findFacet(draggedId);
    final targetPos = state.findFacet(targetId);
    if (dragPos == null || targetPos == null) return;
    if (dragPos.col == targetPos.col) return;

    final newColumns = [
      for (final col in state.columns) List<String>.from(col),
    ];
    final temp = newColumns[dragPos.col];
    newColumns[dragPos.col] = newColumns[targetPos.col];
    newColumns[targetPos.col] = temp;

    state = state.copyWith(columns: newColumns);
  }

  /// Move a facet from its column into another column's stack (at the bottom).
  /// The target is identified by another facet's ID — the moved facet joins
  /// that facet's column.
  void moveFacetToColumn(String facetId, String targetFacetId) {
    final srcPos = state.findFacet(facetId);
    final targetPos = state.findFacet(targetFacetId);
    if (srcPos == null || targetPos == null) return;
    if (srcPos.col == targetPos.col) return;

    final newColumns = [
      for (final col in state.columns) List<String>.from(col),
    ];

    // Remove from source column
    newColumns[srcPos.col].removeAt(srcPos.row);

    // Calculate adjusted target column index after potential source removal
    int adjustedTargetCol = targetPos.col;
    if (newColumns[srcPos.col].isEmpty) {
      newColumns.removeAt(srcPos.col);
      if (targetPos.col > srcPos.col) adjustedTargetCol--;
    }

    // Add to target column
    newColumns[adjustedTargetCol].add(facetId);

    state = state.copyWith(columns: newColumns);
  }

  /// Split a facet out of a multi-facet column into its own new column
  /// (inserted to the right of the source column).
  void splitFacetToOwnColumn(String facetId) {
    final srcPos = state.findFacet(facetId);
    if (srcPos == null) return;
    if (state.columns[srcPos.col].length <= 1) return;

    final newColumns = [
      for (final col in state.columns) List<String>.from(col),
    ];

    newColumns[srcPos.col].removeAt(srcPos.row);
    newColumns.insert(srcPos.col + 1, [facetId]);

    state = state.copyWith(columns: newColumns);
  }
}
