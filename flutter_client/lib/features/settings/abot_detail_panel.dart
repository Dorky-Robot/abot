import 'package:flutter/material.dart';
import '../../core/network/abot_service.dart';
import '../../core/theme/abot_theme.dart';

/// Per-abot detail overlay — opened from Abots tab in sidebar.
class AbotDetailPanel extends StatelessWidget {
  final AbotInfo detail;
  final VoidCallback onClose;
  final VoidCallback? onRemove;
  final void Function(String kuboName)? onSwitchToKubo;

  const AbotDetailPanel({
    super.key,
    required this.detail,
    required this.onClose,
    this.onRemove,
    this.onSwitchToKubo,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    final activeBranches =
        detail.kuboBranches.where((b) => b.hasWorktree).toList();
    final pastBranches =
        detail.kuboBranches.where((b) => !b.hasWorktree).toList();

    return GestureDetector(
      onTap: onClose,
      child: Container(
        color: Colors.black54,
        child: Center(
          child: GestureDetector(
            onTap: () {},
            child: Container(
              width: 320,
              constraints: const BoxConstraints(maxHeight: 520),
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
                            detail.name,
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
                        // Identity
                        _SectionLabel(label: 'Identity'),
                        const SizedBox(height: AbotSpacing.sm),
                        if (detail.createdAt != null)
                          _InfoRow(
                              label: 'Created',
                              value: _formatDate(detail.createdAt!)),
                        _InfoRow(
                            label: 'Branch', value: detail.defaultBranch),
                        if (detail.path.isNotEmpty)
                          Padding(
                            padding:
                                const EdgeInsets.only(top: AbotSpacing.xs),
                            child: Text(
                              detail.path,
                              style: TextStyle(
                                fontSize: 10,
                                color: p.subtext0,
                                fontFamily: AbotFonts.mono,
                              ),
                            ),
                          ),
                        const SizedBox(height: AbotSpacing.lg),

                        // Active in (kubos with live worktrees)
                        if (activeBranches.isNotEmpty) ...[
                          _SectionLabel(label: 'Active in'),
                          const SizedBox(height: AbotSpacing.sm),
                          for (final branch in activeBranches)
                            _KuboBranchRow(
                              branch: branch,
                              isActive: true,
                              onTap: onSwitchToKubo != null
                                  ? () =>
                                      onSwitchToKubo!(branch.kuboName)
                                  : null,
                            ),
                          const SizedBox(height: AbotSpacing.lg),
                        ],

                        // Past work (kubo branches without worktrees)
                        if (pastBranches.isNotEmpty) ...[
                          _SectionLabel(label: 'Past work'),
                          const SizedBox(height: AbotSpacing.sm),
                          for (final branch in pastBranches)
                            _KuboBranchRow(
                              branch: branch,
                              isActive: false,
                            ),
                          const SizedBox(height: AbotSpacing.lg),
                        ],

                        // Remove
                        if (onRemove != null) ...[
                          _SectionLabel(label: 'Manage'),
                          const SizedBox(height: AbotSpacing.sm),
                          _buildRemoveButton(p),
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

  Widget _buildRemoveButton(CatPalette p) {
    return SizedBox(
      height: 32,
      child: TextButton(
        onPressed: onRemove,
        style: TextButton.styleFrom(
          backgroundColor: p.surface1,
          foregroundColor: p.red,
          padding:
              const EdgeInsets.symmetric(horizontal: AbotSpacing.md),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AbotRadius.sm),
          ),
          textStyle: const TextStyle(
            fontSize: 11,
            fontFamily: AbotFonts.mono,
            fontWeight: FontWeight.w600,
          ),
        ),
        child: const Text('Remove from list'),
      ),
    );
  }

  String _formatDate(String iso) {
    try {
      final dt = DateTime.parse(iso);
      return '${dt.year}-${dt.month.toString().padLeft(2, '0')}-${dt.day.toString().padLeft(2, '0')}';
    } catch (_) {
      return iso;
    }
  }
}

class _SectionLabel extends StatelessWidget {
  final String label;
  const _SectionLabel({required this.label});

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Text(
      label,
      style: TextStyle(
        fontSize: 10,
        color: p.subtext0,
        fontFamily: AbotFonts.mono,
        fontWeight: FontWeight.w600,
        letterSpacing: 0.5,
      ),
    );
  }
}

class _InfoRow extends StatelessWidget {
  final String label;
  final String value;
  const _InfoRow({required this.label, required this.value});

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Padding(
      padding: const EdgeInsets.only(bottom: AbotSpacing.xs),
      child: Row(
        children: [
          SizedBox(
            width: 70,
            child: Text(
              label,
              style: TextStyle(
                fontSize: 11,
                color: p.subtext0,
                fontFamily: AbotFonts.mono,
              ),
            ),
          ),
          Expanded(
            child: Text(
              value,
              style: TextStyle(
                fontSize: 11,
                color: p.text,
                fontFamily: AbotFonts.mono,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }
}

class _KuboBranchRow extends StatelessWidget {
  final KuboBranchInfo branch;
  final bool isActive;
  final VoidCallback? onTap;

  const _KuboBranchRow({
    required this.branch,
    required this.isActive,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Padding(
      padding: const EdgeInsets.only(bottom: AbotSpacing.xs),
      child: GestureDetector(
        onTap: onTap,
        behavior: HitTestBehavior.opaque,
        child: Row(
          children: [
            Container(
              width: 6,
              height: 6,
              decoration: BoxDecoration(
                color: isActive ? p.green : p.overlay0,
                shape: BoxShape.circle,
              ),
            ),
            const SizedBox(width: AbotSpacing.sm),
            Expanded(
              child: Text(
                branch.kuboName,
                style: TextStyle(
                  fontSize: 12,
                  color: onTap != null ? p.blue : p.text,
                  fontFamily: AbotFonts.mono,
                ),
                overflow: TextOverflow.ellipsis,
              ),
            ),
            if (branch.merged)
              Container(
                padding: const EdgeInsets.symmetric(
                    horizontal: 6, vertical: 1),
                decoration: BoxDecoration(
                  color: p.surface1,
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                ),
                child: Text(
                  'merged',
                  style: TextStyle(
                    fontSize: 9,
                    color: p.subtext0,
                    fontFamily: AbotFonts.mono,
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
