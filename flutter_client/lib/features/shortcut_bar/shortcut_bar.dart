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
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final bgColor =
        isDark ? CatppuccinMocha.mantle : CatppuccinLatte.mantle;
    final borderColor =
        isDark ? CatppuccinMocha.surface1 : CatppuccinLatte.surface1;
    final textColor =
        isDark ? CatppuccinMocha.subtext0 : CatppuccinLatte.subtext0;
    final activeColor =
        isDark ? CatppuccinMocha.mauve : CatppuccinLatte.mauve;

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
              color: connected
                  ? (isDark
                      ? CatppuccinMocha.green
                      : CatppuccinLatte.green)
                  : (isDark
                      ? CatppuccinMocha.red
                      : CatppuccinLatte.red),
            ),
          ),

          // [+ New] button
          _BarButton(
            onTap: onNewFacet,
            isDark: isDark,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.add, size: 14, color: textColor),
                const SizedBox(width: 2),
                Text('New',
                    style: TextStyle(
                        fontSize: 12,
                        color: textColor,
                        fontFamily: 'JetBrains Mono')),
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
                    isDark: isDark,
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
                            fontFamily: 'JetBrains Mono',
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

          // Sessions drawer button
          _BarButton(
            onTap: () {
              Scaffold.of(context).openEndDrawer();
            },
            isDark: isDark,
            child: Icon(Icons.dashboard_outlined, size: 16, color: textColor),
          ),
          const SizedBox(width: AbotSpacing.xs),

          // Pinned shortcuts
          _ShortcutButton(
            label: 'Esc',
            isDark: isDark,
            onTap: () {
              final wsService = ref.read(wsServiceProvider.notifier);
              wsService.sendInput('\x1b');
            },
          ),
          const SizedBox(width: AbotSpacing.xs),
          _ShortcutButton(
            label: 'Tab',
            isDark: isDark,
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
  final bool isDark;
  final bool isActive;

  const _BarButton({
    required this.child,
    required this.isDark,
    this.onTap,
    this.isActive = false,
  });

  @override
  Widget build(BuildContext context) {
    final bg = isActive
        ? (isDark ? CatppuccinMocha.surface0 : CatppuccinLatte.surface0)
        : Colors.transparent;

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
  final bool isDark;
  final VoidCallback onTap;

  const _ShortcutButton({
    required this.label,
    required this.isDark,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final textColor =
        isDark ? CatppuccinMocha.subtext0 : CatppuccinLatte.subtext0;
    final borderColor =
        isDark ? CatppuccinMocha.surface1 : CatppuccinLatte.surface1;

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
              fontFamily: 'JetBrains Mono',
            ),
          ),
        ),
      ),
    );
  }
}
