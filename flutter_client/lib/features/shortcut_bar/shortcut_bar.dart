import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/theme/abot_theme.dart';
import '../../core/network/websocket_service.dart';
import '../facet/facet.dart';

/// Bottom shortcut bar: [+ New] [tab1] [tab2] ... [spacer] [sessions] [Esc] [Tab]
class ShortcutBar extends ConsumerWidget {
  final List<FacetData> facets;
  final String? focusedId;
  final bool connected;
  final VoidCallback? onNewFacet;
  final void Function(String facetId)? onFocusFacet;

  const ShortcutBar({
    super.key,
    required this.facets,
    this.focusedId,
    this.connected = false,
    this.onNewFacet,
    this.onFocusFacet,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final p = context.palette;
    final bgColor = p.mantle;
    final borderColor = p.surface1;
    final textColor = p.subtext0;
    final activeColor = p.mauve;

    return Container(
      height: AbotSizes.barHeight,
      decoration: BoxDecoration(
        color: bgColor,
        border: Border(top: BorderSide(color: borderColor, width: 1)),
      ),
      child: Row(
        children: [
          const SizedBox(width: AbotSpacing.xs),

          // Connection indicator
          Container(
            width: 8,
            height: 8,
            margin: const EdgeInsets.symmetric(horizontal: AbotSpacing.xs),
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: connected ? p.green : p.red,
            ),
          ),

          // [+ New] button
          _BarButton(
            onTap: onNewFacet,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.add, size: 14, color: textColor),
                const SizedBox(width: 2),
                Text('New',
                    style: TextStyle(
                        fontSize: 12,
                        color: textColor,
                        fontFamily: AbotFonts.mono)),
              ],
            ),
          ),

          const SizedBox(width: AbotSpacing.xs),

          // Facet tabs
          Expanded(
            child: ListView(
              scrollDirection: Axis.horizontal,
              children: facets.map((facet) {
                final isActive = facet.id == focusedId;
                return Padding(
                  padding:
                      const EdgeInsets.only(right: AbotSpacing.xs),
                  child: _BarButton(
                    onTap: () => onFocusFacet?.call(facet.id),
                    isActive: isActive,
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(Icons.terminal,
                            size: 14,
                            color: isActive ? activeColor : textColor),
                        const SizedBox(width: 4),
                        Text(
                          facet.sessionName,
                          style: TextStyle(
                            fontSize: 12,
                            color: isActive ? activeColor : textColor,
                            fontFamily: AbotFonts.mono,
                            fontWeight: isActive
                                ? FontWeight.w500
                                : FontWeight.normal,
                          ),
                          overflow: TextOverflow.ellipsis,
                        ),
                      ],
                    ),
                  ),
                );
              }).toList(),
            ),
          ),

          // Pinned shortcuts
          _ShortcutButton(
            label: 'Esc',
            onTap: () {
              final wsService = ref.read(wsServiceProvider.notifier);
              wsService.sendInput('\x1b');
            },
          ),
          const SizedBox(width: AbotSpacing.xs),
          _ShortcutButton(
            label: 'Tab',
            onTap: () {
              final wsService = ref.read(wsServiceProvider.notifier);
              wsService.sendInput('\t');
            },
          ),

          const SizedBox(width: AbotSpacing.sm),
        ],
      ),
    );
  }
}

class _BarButton extends StatelessWidget {
  final VoidCallback? onTap;
  final Widget child;
  final bool isActive;

  const _BarButton({
    required this.child,
    this.onTap,
    this.isActive = false,
  });

  @override
  Widget build(BuildContext context) {
    final bg = isActive ? context.palette.surface0 : Colors.transparent;

    return Material(
      color: bg,
      borderRadius: BorderRadius.circular(AbotRadius.sm),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(AbotRadius.sm),
        child: Padding(
          padding: const EdgeInsets.symmetric(
            horizontal: AbotSpacing.sm,
            vertical: AbotSpacing.xs,
          ),
          child: child,
        ),
      ),
    );
  }
}

class _ShortcutButton extends StatelessWidget {
  final String label;
  final VoidCallback onTap;

  const _ShortcutButton({
    required this.label,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    final textColor = p.subtext0;
    final borderColor = p.surface1;

    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(AbotRadius.sm),
        child: Container(
          padding: const EdgeInsets.symmetric(
            horizontal: AbotSpacing.sm,
            vertical: AbotSpacing.xs,
          ),
          decoration: BoxDecoration(
            border: Border.all(color: borderColor, width: 1),
            borderRadius: BorderRadius.circular(AbotRadius.sm),
          ),
          child: Text(
            label,
            style: TextStyle(
              fontSize: 12,
              color: textColor,
              fontFamily: AbotFonts.mono,
            ),
          ),
        ),
      ),
    );
  }
}
