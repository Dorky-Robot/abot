import 'dart:js_interop';
import 'package:flutter/material.dart';
import '../../core/js_interop/xterm_interop.dart';
import '../../core/theme/abot_theme.dart';

/// Search overlay for a terminal facet.
class TerminalSearchBar extends StatefulWidget {
  final XtermSearchAddon searchAddon;
  final VoidCallback onClose;

  const TerminalSearchBar({
    super.key,
    required this.searchAddon,
    required this.onClose,
  });

  @override
  State<TerminalSearchBar> createState() => _TerminalSearchBarState();
}

class _TerminalSearchBarState extends State<TerminalSearchBar> {
  final _controller = TextEditingController();
  final _focusNode = FocusNode();

  @override
  void initState() {
    super.initState();
    _focusNode.requestFocus();
  }

  @override
  void dispose() {
    _controller.dispose();
    _focusNode.dispose();
    widget.searchAddon.clearDecorations();
    super.dispose();
  }

  void _findNext() {
    final term = _controller.text;
    if (term.isNotEmpty) {
      widget.searchAddon.findNext(term.toJS);
    }
  }

  void _findPrevious() {
    final term = _controller.text;
    if (term.isNotEmpty) {
      widget.searchAddon.findPrevious(term.toJS);
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    final bgColor = p.surface0;
    final textColor = p.text;
    final subtextColor = p.subtext0;
    final borderColor = p.surface1;
    final accentColor = p.mauve;

    return Container(
      height: 36,
      decoration: BoxDecoration(
        color: bgColor,
        border: Border(bottom: BorderSide(color: borderColor, width: 1)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: AbotSpacing.sm),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: _controller,
              focusNode: _focusNode,
              style: TextStyle(
                fontFamily: AbotFonts.mono,
                fontSize: 12,
                color: textColor,
              ),
              decoration: InputDecoration(
                hintText: 'Search...',
                hintStyle: TextStyle(
                  fontFamily: AbotFonts.mono,
                  fontSize: 12,
                  color: subtextColor.withValues(alpha: 0.5),
                ),
                isDense: true,
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: AbotSpacing.sm,
                  vertical: 6,
                ),
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  borderSide: BorderSide(color: borderColor),
                ),
                enabledBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  borderSide: BorderSide(color: borderColor),
                ),
                focusedBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                  borderSide: BorderSide(color: accentColor),
                ),
              ),
              onSubmitted: (_) => _findNext(),
              onChanged: (_) => _findNext(),
            ),
          ),
          const SizedBox(width: AbotSpacing.xs),
          _SearchButton(
            icon: Icons.keyboard_arrow_up,
            onPressed: _findPrevious,
            color: subtextColor,
          ),
          _SearchButton(
            icon: Icons.keyboard_arrow_down,
            onPressed: _findNext,
            color: subtextColor,
          ),
          _SearchButton(
            icon: Icons.close,
            onPressed: widget.onClose,
            color: subtextColor,
          ),
        ],
      ),
    );
  }
}

class _SearchButton extends StatelessWidget {
  final IconData icon;
  final VoidCallback onPressed;
  final Color color;

  const _SearchButton({
    required this.icon,
    required this.onPressed,
    required this.color,
  });

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onPressed,
      borderRadius: BorderRadius.circular(AbotRadius.sm),
      child: Padding(
        padding: const EdgeInsets.all(4),
        child: Icon(icon, size: 18, color: color),
      ),
    );
  }
}
