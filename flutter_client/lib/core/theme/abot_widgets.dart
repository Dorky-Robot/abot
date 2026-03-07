import 'package:flutter/material.dart';

import 'abot_theme.dart';

class AbotSectionLabel extends StatelessWidget {
  final String label;
  const AbotSectionLabel({super.key, required this.label});

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
