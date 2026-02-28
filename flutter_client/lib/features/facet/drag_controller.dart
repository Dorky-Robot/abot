import 'package:flutter/widgets.dart';

/// Preview state shown during a drag gesture.
enum DragPreview {
  none,
  moveDown,
  splitUp,
}

/// Tracks an active titlebar drag gesture and classifies direction.
///
/// Ported from client/lib/facet-manager.js global pointer handler.
/// Direction classification: |dy| > |dx| → vertical, else horizontal.
/// Thresholds: commit at 8px, preview at 40px, execute at 80px.
class DragController {
  static const double _commitThreshold = 8;
  static const double _previewThreshold = 40;
  static const double _executeThreshold = 80;
  static const int _swapCooldownMs = 100;

  /// Called when a horizontal drag swaps columns.
  /// Parameters: (draggedFacetId, targetFacetId)
  final void Function(String draggedId, String targetId)? onReorderColumns;

  /// Called when dragging down past threshold moves facet into target's column.
  /// Parameters: (facetId, targetFacetId)
  final void Function(String facetId, String targetFacetId)? onMoveFacetToColumn;

  /// Called when dragging up past threshold splits facet to own column.
  /// Parameters: (facetId)
  final void Function(String facetId)? onSplitFacetToOwnColumn;

  /// Called when preview state changes for a facet.
  final void Function(String facetId, DragPreview preview)? onPreviewChanged;

  /// Called when drag ends (for cleanup).
  final VoidCallback? onDragEnd;

  /// Hit-test callback: given a global position, return the facetId at that
  /// point (excluding the currently dragged facet), or null.
  final String? Function(Offset globalPosition, String excludeId)? hitTest;

  /// Check if a facet's column has multiple facets (needed for split-up).
  final bool Function(String facetId)? isInMultiFacetColumn;

  /// Check if there are multiple columns (needed for move-down).
  final bool Function()? hasMultipleColumns;

  DragController({
    this.onReorderColumns,
    this.onMoveFacetToColumn,
    this.onSplitFacetToOwnColumn,
    this.onPreviewChanged,
    this.onDragEnd,
    this.hitTest,
    this.isInMultiFacetColumn,
    this.hasMultipleColumns,
  });

  // Active drag state
  String? _activeFacetId;
  Offset? _startPosition;
  bool _committed = false;
  bool _moveTriggered = false;
  int _lastSwapTime = 0;
  DragPreview _currentPreview = DragPreview.none;

  bool get isDragging => _activeFacetId != null && _committed;
  String? get activeFacetId => _activeFacetId;

  void onDragStart(String facetId, Offset globalPosition) {
    _activeFacetId = facetId;
    _startPosition = globalPosition;
    _committed = false;
    _moveTriggered = false;
    _lastSwapTime = 0;
    _currentPreview = DragPreview.none;
  }

  void onDragUpdate(Offset globalPosition) {
    if (_activeFacetId == null || _startPosition == null) return;

    final dx = globalPosition.dx - _startPosition!.dx;
    final dy = globalPosition.dy - _startPosition!.dy;

    if (!_committed) {
      if (dx * dx + dy * dy < _commitThreshold * _commitThreshold) return;
      _committed = true;
    }

    final absDx = dx.abs();
    final absDy = dy.abs();
    final primarilyDown = dy > 0 && absDy > absDx;
    final primarilyUp = dy < 0 && absDy > absDx;

    if (primarilyDown && !_moveTriggered) {
      // Drag down: move facet into adjacent column (stack)
      if (hasMultipleColumns?.call() != true) return;

      if (dy > _executeThreshold) {
        final targetId = hitTest?.call(globalPosition, _activeFacetId!);
        if (targetId != null) {
          _moveTriggered = true;
          _setPreview(DragPreview.none);
          onMoveFacetToColumn?.call(_activeFacetId!, targetId);
          onDragEnd?.call();
          _reset();
          return;
        }
      } else if (dy > _previewThreshold) {
        _setPreview(DragPreview.moveDown);
      }
    } else if (primarilyUp && !_moveTriggered) {
      // Drag up: split facet out of stacked column
      if (isInMultiFacetColumn?.call(_activeFacetId!) != true) return;

      if (-dy > _executeThreshold) {
        _moveTriggered = true;
        _setPreview(DragPreview.none);
        onSplitFacetToOwnColumn?.call(_activeFacetId!);
        onDragEnd?.call();
        _reset();
        return;
      } else if (-dy > _previewThreshold) {
        _setPreview(DragPreview.splitUp);
      }
    } else if (!_moveTriggered) {
      // Horizontal: reorder columns
      _setPreview(DragPreview.none);

      final targetId = hitTest?.call(globalPosition, _activeFacetId!);
      if (targetId != null) {
        final now = DateTime.now().millisecondsSinceEpoch;
        if (now - _lastSwapTime > _swapCooldownMs) {
          _lastSwapTime = now;
          onReorderColumns?.call(_activeFacetId!, targetId);
        }
      }
    }
  }

  void onDragEnded() {
    if (_activeFacetId != null) {
      _setPreview(DragPreview.none);
      onDragEnd?.call();
    }
    _reset();
  }

  void _setPreview(DragPreview preview) {
    if (_currentPreview != preview && _activeFacetId != null) {
      _currentPreview = preview;
      onPreviewChanged?.call(_activeFacetId!, preview);
    }
  }

  void _reset() {
    _activeFacetId = null;
    _startPosition = null;
    _committed = false;
    _moveTriggered = false;
    _currentPreview = DragPreview.none;
  }
}
