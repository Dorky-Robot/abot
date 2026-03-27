import 'package:flutter/material.dart';
import '../../core/network/api_client.dart';
import '../../core/network/session_service.dart';
import '../../core/theme/abot_theme.dart';
import '../../core/theme/abot_widgets.dart';

/// Per-session settings overlay — opened from session gear icon.
/// Shows session info, rename, and document save/save-as.
class SessionSettingsPanel extends StatefulWidget {
  final String sessionName;
  final VoidCallback onClose;
  final ValueChanged<String>? onRenamed;

  const SessionSettingsPanel({
    super.key,
    required this.sessionName,
    required this.onClose,
    this.onRenamed,
  });

  @override
  State<SessionSettingsPanel> createState() => _SessionSettingsPanelState();
}

class _SessionSettingsPanelState extends State<SessionSettingsPanel> {
  final _api = const ApiClient();
  final _renameController = TextEditingController();
  final _renameFocus = FocusNode();

  bool _savingBundle = false;
  String? _bundlePath;
  bool _dirty = false;
  String? _savedMessage;
  bool _renaming = false;
  late String _currentName;

  @override
  void initState() {
    super.initState();
    _currentName = widget.sessionName;
    _loadSessionInfo();
  }

  @override
  void dispose() {
    _renameController.dispose();
    _renameFocus.dispose();
    super.dispose();
  }

  Future<void> _loadSessionInfo() async {
    final url = '/sessions/${Uri.encodeComponent(_currentName)}';
    try {
      final data = await _api.get(url) as Map<String, dynamic>;
      if (!mounted) return;
      final info = SessionInfo.fromJson(data);
      setState(() {
        _bundlePath = info.bundlePath;
        _dirty = info.dirty;
      });
    } catch (_) {
      // Session info not critical — just leave defaults
    }
  }

  void _startRename() {
    _renameController.text = _currentName;
    setState(() => _renaming = true);
    Future.microtask(() {
      _renameFocus.requestFocus();
      _renameController.selection = TextSelection(
        baseOffset: 0,
        extentOffset: _renameController.text.length,
      );
    });
  }

  Future<void> _submitRename() async {
    final newName = _renameController.text.trim();
    if (newName.isEmpty || newName == _currentName) {
      setState(() => _renaming = false);
      return;
    }
    try {
      final resp = await _api.put(
        '/sessions/${Uri.encodeComponent(_currentName)}',
        {'name': newName},
      );
      if (!mounted) return;
      // Server returns the qualified name (e.g. "bob@default")
      final qualifiedName = (resp is Map && resp['newName'] != null)
          ? resp['newName'] as String
          : newName;
      setState(() {
        _currentName = qualifiedName;
        _renaming = false;
      });
      widget.onRenamed?.call(qualifiedName);
    } catch (e) {
      if (!mounted) return;
      setState(() => _renaming = false);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Rename failed: $e')),
      );
    }
  }

  Future<void> _saveSession() async {
    setState(() => _savingBundle = true);
    final url =
        '/sessions/${Uri.encodeComponent(_currentName)}/save';
    try {
      final data = await _api.post(url, {}) as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _savingBundle = false;
        _dirty = false;
        _bundlePath = data['path'] as String? ?? _bundlePath;
        _savedMessage = 'Saved';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _savingBundle = false);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Save failed: $e')),
      );
    }
  }

  Future<void> _saveSessionAs() async {
    String defaultName = '$_currentName.abot';
    if (_bundlePath != null) {
      final lastSlash = _bundlePath!.lastIndexOf('/');
      if (lastSlash >= 0 && lastSlash < _bundlePath!.length - 1) {
        defaultName = _bundlePath!.substring(lastSlash + 1);
      }
    }

    // Prompt user for a name — the path is always ~/.abot/abots/<name>.abot
    final controller = TextEditingController(text: defaultName.replaceAll('.abot', ''));
    final name = await showDialog<String>(
      context: context,
      builder: (ctx) {
        final p = ctx.palette;
        return AlertDialog(
          backgroundColor: p.base,
          title: Text('Save As',
              style: TextStyle(
                  color: p.text, fontFamily: AbotFonts.mono, fontSize: 14)),
          content: TextField(
            controller: controller,
            autofocus: true,
            style: TextStyle(
                color: p.text, fontFamily: AbotFonts.mono, fontSize: 13),
            decoration: InputDecoration(
              hintText: 'abot name',
              suffixText: '.abot',
              hintStyle: TextStyle(color: p.overlay0, fontFamily: AbotFonts.mono),
              suffixStyle: TextStyle(color: p.overlay0, fontFamily: AbotFonts.mono, fontSize: 12),
              enabledBorder: UnderlineInputBorder(
                  borderSide: BorderSide(color: p.surface1)),
              focusedBorder: UnderlineInputBorder(
                  borderSide: BorderSide(color: p.mauve)),
            ),
            onSubmitted: (v) => Navigator.pop(ctx, v.trim()),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx),
              child: Text('Cancel',
                  style: TextStyle(color: p.subtext0, fontFamily: AbotFonts.mono)),
            ),
            TextButton(
              onPressed: () => Navigator.pop(ctx, controller.text.trim()),
              child: Text('Save',
                  style: TextStyle(color: p.mauve, fontFamily: AbotFonts.mono)),
            ),
          ],
        );
      },
    );
    controller.dispose();

    if (name == null || name.isEmpty || !mounted) return;

    // Build path in the standard location
    final path = '~/.abot/abots/$name.abot';

    setState(() => _savingBundle = true);
    final url =
        '/sessions/${Uri.encodeComponent(_currentName)}/save-as';
    try {
      final data =
          await _api.post(url, {'path': path}) as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _savingBundle = false;
        _dirty = false;
        _bundlePath = data['path'] as String? ?? path;
        _savedMessage = 'Saved';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _savingBundle = false);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Save As failed: $e')),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return GestureDetector(
      onTap: widget.onClose,
      child: Container(
        color: Colors.black54,
        child: Center(
          child: GestureDetector(
            onTap: () {}, // absorb taps on the panel
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
                      AbotSpacing.lg,
                      AbotSpacing.lg,
                      AbotSpacing.sm,
                      0,
                    ),
                    child: Row(
                      children: [
                        Expanded(
                          child: Row(
                            children: [
                              Flexible(
                                child: _renaming
                                    ? SizedBox(
                                        height: 28,
                                        child: TextField(
                                          controller: _renameController,
                                          focusNode: _renameFocus,
                                          style: TextStyle(
                                            fontSize: 14,
                                            fontWeight: FontWeight.w600,
                                            color: p.text,
                                            fontFamily: AbotFonts.mono,
                                          ),
                                          decoration: InputDecoration(
                                            isDense: true,
                                            contentPadding:
                                                const EdgeInsets.symmetric(
                                              horizontal: AbotSpacing.xs,
                                              vertical: AbotSpacing.xs,
                                            ),
                                            border: OutlineInputBorder(
                                              borderRadius:
                                                  BorderRadius.circular(
                                                      AbotRadius.sm),
                                              borderSide: BorderSide(
                                                  color: p.mauve),
                                            ),
                                            focusedBorder: OutlineInputBorder(
                                              borderRadius:
                                                  BorderRadius.circular(
                                                      AbotRadius.sm),
                                              borderSide: BorderSide(
                                                  color: p.mauve),
                                            ),
                                            filled: true,
                                            fillColor: p.surface0,
                                          ),
                                          onSubmitted: (_) => _submitRename(),
                                        ),
                                      )
                                    : GestureDetector(
                                        onDoubleTap: _startRename,
                                        child: Text(
                                          _currentName,
                                          style: TextStyle(
                                            fontSize: 14,
                                            fontWeight: FontWeight.w600,
                                            color: p.text,
                                            fontFamily: AbotFonts.mono,
                                          ),
                                          overflow: TextOverflow.ellipsis,
                                        ),
                                      ),
                              ),
                              if (_dirty)
                                Padding(
                                  padding: const EdgeInsets.only(
                                      left: AbotSpacing.xs),
                                  child: Container(
                                    width: 6,
                                    height: 6,
                                    decoration: BoxDecoration(
                                      color: p.yellow,
                                      shape: BoxShape.circle,
                                    ),
                                  ),
                                ),
                            ],
                          ),
                        ),
                        IconButton(
                          icon: Icon(Icons.close, size: 18, color: p.subtext0),
                          onPressed: widget.onClose,
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
                        // Document section
                        AbotSectionLabel(label: 'Document'),
                        const SizedBox(height: AbotSpacing.sm),
                        _buildDocumentSection(p),
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

  Widget _buildDocumentSection(CatPalette p) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Show bundle path or "Unsaved"
        Text(
          _bundlePath ?? 'Unsaved',
          style: TextStyle(
            fontSize: 10,
            color: _bundlePath != null ? p.subtext0 : p.yellow,
            fontFamily: AbotFonts.mono,
          ),
        ),

        if (_savedMessage != null) ...[
          const SizedBox(height: AbotSpacing.sm),
          Container(
            padding: const EdgeInsets.all(AbotSpacing.sm),
            decoration: BoxDecoration(
              color: p.surface0,
              borderRadius: BorderRadius.circular(AbotRadius.md),
              border: Border.all(color: p.green, width: 0.5),
            ),
            child: Row(
              children: [
                Icon(Icons.check_circle, size: 16, color: p.green),
                const SizedBox(width: AbotSpacing.sm),
                Text(
                  _savedMessage!,
                  style: TextStyle(
                    fontSize: 11,
                    color: p.green,
                    fontFamily: AbotFonts.mono,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ],
            ),
          ),
        ],

        const SizedBox(height: AbotSpacing.md),
        Row(
          children: [
            // Save button (only enabled when dirty + has path)
            SizedBox(
              height: 32,
              child: TextButton(
                onPressed: (_dirty && _bundlePath != null && !_savingBundle)
                    ? _saveSession
                    : null,
                style: TextButton.styleFrom(
                  backgroundColor: p.surface1,
                  foregroundColor: p.text,
                  disabledForegroundColor: p.overlay0,
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
                child: _savingBundle
                    ? SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: p.text,
                        ),
                      )
                    : const Text('Save'),
              ),
            ),
            const SizedBox(width: AbotSpacing.sm),
            // Save As button (always available)
            SizedBox(
              height: 32,
              child: TextButton(
                onPressed: _savingBundle ? null : _saveSessionAs,
                style: TextButton.styleFrom(
                  backgroundColor: p.surface1,
                  foregroundColor: p.text,
                  disabledForegroundColor: p.overlay0,
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
                child: const Text('Save As'),
              ),
            ),
          ],
        ),
      ],
    );
  }

}

