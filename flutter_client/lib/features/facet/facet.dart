/// Facet type enum — currently only terminals, designed for future types.
enum FacetType {
  terminal,
  // Future: canvas, conversation, dashboard
}

/// A facet is the visual primitive — a translucent panel in the spatial UI.
class FacetData {
  final String id;
  final String sessionName;
  final FacetType type;
  final int columnIndex;

  const FacetData({
    required this.id,
    required this.sessionName,
    this.type = FacetType.terminal,
    this.columnIndex = 0,
  });

  FacetData copyWith({
    String? sessionName,
    FacetType? type,
    int? columnIndex,
  }) =>
      FacetData(
        id: id,
        sessionName: sessionName ?? this.sessionName,
        type: type ?? this.type,
        columnIndex: columnIndex ?? this.columnIndex,
      );
}
