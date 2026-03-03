import 'package:flutter/material.dart';
import '../../core/theme/abot_theme.dart';

/// Reusable credential input widget — text field + save button.
/// Used by both global settings (AI tab) and per-session settings.
class CredentialInput extends StatelessWidget {
  final TextEditingController controller;
  final bool saving;
  final VoidCallback onSave;
  final String hintText;

  const CredentialInput({
    super.key,
    required this.controller,
    required this.saving,
    required this.onSave,
    this.hintText = 'Paste token or API key...',
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return Row(
      children: [
        Expanded(
          child: SizedBox(
            height: 32,
            child: TextField(
              controller: controller,
              obscureText: true,
              style: TextStyle(
                fontSize: 12,
                color: p.text,
                fontFamily: AbotFonts.mono,
              ),
              decoration: InputDecoration(
                hintText: hintText,
                hintStyle: TextStyle(
                  fontSize: 12,
                  color: p.overlay0,
                  fontFamily: AbotFonts.mono,
                ),
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: AbotSpacing.sm,
                ),
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  borderSide: BorderSide(color: p.surface1),
                ),
                enabledBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  borderSide: BorderSide(color: p.surface1),
                ),
                focusedBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  borderSide: BorderSide(color: p.mauve),
                ),
                filled: true,
                fillColor: p.surface0,
              ),
              onSubmitted: (_) => onSave(),
            ),
          ),
        ),
        const SizedBox(width: AbotSpacing.sm),
        SizedBox(
          height: 32,
          child: TextButton(
            onPressed: saving ? null : onSave,
            style: TextButton.styleFrom(
              backgroundColor: p.mauve,
              foregroundColor: p.base,
              padding: const EdgeInsets.symmetric(
                horizontal: AbotSpacing.md,
              ),
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(AbotRadius.sm),
              ),
              textStyle: const TextStyle(
                fontSize: 11,
                fontFamily: AbotFonts.mono,
                fontWeight: FontWeight.w600,
              ),
            ),
            child: saving
                ? SizedBox(
                    width: 14,
                    height: 14,
                    child: CircularProgressIndicator(
                      strokeWidth: 2,
                      color: p.base,
                    ),
                  )
                : const Text('Save'),
          ),
        ),
      ],
    );
  }
}

/// Connected status badge — shows green checkmark with message.
class CredentialConnectedBadge extends StatelessWidget {
  final String message;
  final String? subtitle;
  final VoidCallback onDisconnect;

  const CredentialConnectedBadge({
    super.key,
    required this.message,
    this.subtitle,
    required this.onDisconnect,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Container(
          padding: const EdgeInsets.all(AbotSpacing.md),
          decoration: BoxDecoration(
            color: p.surface0,
            borderRadius: BorderRadius.circular(AbotRadius.md),
            border: Border.all(color: p.green, width: 0.5),
          ),
          child: Row(
            children: [
              Icon(Icons.check_circle, size: 16, color: p.green),
              const SizedBox(width: AbotSpacing.sm),
              Expanded(
                child: Text(
                  message,
                  style: TextStyle(
                    fontSize: 11,
                    color: p.green,
                    fontFamily: AbotFonts.mono,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ],
          ),
        ),
        if (subtitle != null) ...[
          const SizedBox(height: AbotSpacing.sm),
          Text(
            subtitle!,
            style: TextStyle(
              fontSize: 11,
              color: p.subtext0,
              fontFamily: AbotFonts.mono,
            ),
          ),
        ],
        const SizedBox(height: AbotSpacing.md),
        SizedBox(
          height: 32,
          child: TextButton(
            onPressed: onDisconnect,
            style: TextButton.styleFrom(
              foregroundColor: p.red,
              textStyle: const TextStyle(
                fontSize: 11,
                fontFamily: AbotFonts.mono,
              ),
            ),
            child: const Text('Remove credentials'),
          ),
        ),
      ],
    );
  }
}
