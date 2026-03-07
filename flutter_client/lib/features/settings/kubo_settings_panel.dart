import 'package:flutter/material.dart';
import '../../core/network/kubo_service.dart';
import '../../core/theme/abot_theme.dart';
import '../../core/theme/abot_widgets.dart';

/// Per-kubo settings overlay — opened from kubo gear icon in sidebar.
class KuboSettingsPanel extends StatelessWidget {
  final KuboInfo kubo;
  final VoidCallback onClose;
  final VoidCallback? onShutdown;
  final VoidCallback? onStart;

  const KuboSettingsPanel({
    super.key,
    required this.kubo,
    required this.onClose,
    this.onShutdown,
    this.onStart,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return GestureDetector(
      onTap: onClose,
      child: Container(
        color: Colors.black54,
        child: Center(
          child: GestureDetector(
            onTap: () {},
            child: Container(
              width: 320,
              constraints: const BoxConstraints(maxHeight: 480),
              decoration: BoxDecoration(
                color: p.base,
                borderRadius: BorderRadius.circular(AbotRadius.lg),
                border: Border.all(color: p.surface1, width: 0.5),
                boxShadow: [
                  BoxShadow(
                    color: Colors.black.withValues(alpha: 0.3),
                    blurRadius: 24,
                    offset: const Offset(4, 0),
                  ),
                ],
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  // Header
                  Padding(
                    padding: const EdgeInsets.fromLTRB(
                      AbotSpacing.lg, AbotSpacing.lg, AbotSpacing.sm, 0,
                    ),
                    child: Row(
                      children: [
                        Expanded(
                          child: Text(
                            kubo.name,
                            style: TextStyle(
                              fontSize: 14,
                              fontWeight: FontWeight.w600,
                              color: p.text,
                              fontFamily: AbotFonts.mono,
                            ),
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                        IconButton(
                          icon: Icon(Icons.close, size: 18, color: p.subtext0),
                          onPressed: onClose,
                          splashRadius: 16,
                        ),
                      ],
                    ),
                  ),

                  Divider(color: p.surface1, height: 1),

                  // Content
                  Flexible(
                    child: ListView(
                      shrinkWrap: true,
                      padding: const EdgeInsets.all(AbotSpacing.lg),
                      children: [
                        // Status
                        AbotSectionLabel(label: 'Status'),
                        const SizedBox(height: AbotSpacing.sm),
                        _buildStatusRow(p),
                        const SizedBox(height: AbotSpacing.lg),

                        // Path
                        if (kubo.path.isNotEmpty) ...[
                          AbotSectionLabel(label: 'Location'),
                          const SizedBox(height: AbotSpacing.sm),
                          Text(
                            kubo.path,
                            style: TextStyle(
                              fontSize: 10,
                              color: p.subtext0,
                              fontFamily: AbotFonts.mono,
                            ),
                          ),
                          const SizedBox(height: AbotSpacing.lg),
                        ],

                        // Abots
                        if (kubo.abots.isNotEmpty) ...[
                          AbotSectionLabel(label: 'Abots'),
                          const SizedBox(height: AbotSpacing.sm),
                          for (final abot in kubo.abots)
                            Padding(
                              padding: const EdgeInsets.only(bottom: AbotSpacing.xs),
                              child: Row(
                                children: [
                                  Container(
                                    width: 6,
                                    height: 6,
                                    decoration: BoxDecoration(
                                      color: p.overlay0,
                                      shape: BoxShape.circle,
                                    ),
                                  ),
                                  const SizedBox(width: AbotSpacing.sm),
                                  Text(
                                    abot,
                                    style: TextStyle(
                                      fontSize: 12,
                                      color: p.text,
                                      fontFamily: AbotFonts.mono,
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          const SizedBox(height: AbotSpacing.lg),
                        ],

                        // Start / Stop
                        if (!kubo.running && onStart != null) ...[
                          _buildActionButton(p, 'Start kubo', p.green, onStart!),
                        ],
                        if (kubo.running && onShutdown != null) ...[
                          _buildActionButton(p, 'Stop kubo', p.red, onShutdown!),
                        ],
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildStatusRow(CatPalette p) {
    return Row(
      children: [
        Container(
          width: 8,
          height: 8,
          decoration: BoxDecoration(
            color: kubo.running ? p.green : p.overlay0,
            shape: BoxShape.circle,
          ),
        ),
        const SizedBox(width: AbotSpacing.sm),
        Text(
          kubo.running ? 'Running' : 'Stopped',
          style: TextStyle(
            fontSize: 12,
            color: kubo.running ? p.green : p.overlay0,
            fontFamily: AbotFonts.mono,
          ),
        ),
        if (kubo.activeSessions > 0) ...[
          const SizedBox(width: AbotSpacing.md),
          Text(
            '${kubo.activeSessions} active session${kubo.activeSessions == 1 ? '' : 's'}',
            style: TextStyle(
              fontSize: 11,
              color: p.subtext0,
              fontFamily: AbotFonts.mono,
            ),
          ),
        ],
      ],
    );
  }

  Widget _buildActionButton(CatPalette p, String label, Color color, VoidCallback onTap) {
    return SizedBox(
      height: 32,
      child: TextButton(
        onPressed: onTap,
        style: TextButton.styleFrom(
          backgroundColor: p.surface1,
          foregroundColor: color,
          padding: const EdgeInsets.symmetric(horizontal: AbotSpacing.md),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AbotRadius.sm),
          ),
          textStyle: const TextStyle(
            fontSize: 11,
            fontFamily: AbotFonts.mono,
            fontWeight: FontWeight.w600,
          ),
        ),
        child: Text(label),
      ),
    );
  }
}

