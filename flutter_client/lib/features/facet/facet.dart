/// Facet type enum — currently only terminals, designed for future types.
enum FacetType {
  terminal,
  // Future: canvas, conversation, dashboard
}

/// A facet is the visual primitive — a translucent panel in the spatial UI.
/// Column structure is tracked by FacetManagerNotifier, not here.
class FacetData {
  final String id;
  final String sessionName;
  final FacetType type;

  const FacetData({
    required this.id,
    required this.sessionName,
    this.type = FacetType.terminal,
  });

  FacetData copyWith({
    String? sessionName,
    FacetType? type,
  }) =>
      FacetData(
        id: id,
        sessionName: sessionName ?? this.sessionName,
        type: type ?? this.type,
      );
}
