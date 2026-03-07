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
      await _api.put(
        '/sessions/${Uri.encodeComponent(_currentName)}',
        {'name': newName},
      );
      if (!mounted) return;
      setState(() {
        _currentName = newName;
        _renaming = false;
      });
      widget.onRenamed?.call(newName);
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
    String defaultFileName = '$_currentName.abot';
    if (_bundlePath != null) {
      final lastSlash = _bundlePath!.lastIndexOf('/');
      if (lastSlash >= 0 && lastSlash < _bundlePath!.length - 1) {
        defaultFileName = _bundlePath!.substring(lastSlash + 1);
      }
    }

    String? path;
    try {
      final data = await _api.post('/api/pick-save-location', {
        'defaultName': defaultFileName,
      }) as Map<String, dynamic>;
      path = data['path'] as String?;
    } catch (_) {
      return;
    }

    if (path == null || path.isEmpty || !mounted) return;
    // Ensure .abot extension
    if (!path.endsWith('.abot')) {
      path = '$path.abot';
    }
    // Reject saving inside another .abot bundle
    final segments = path.split('/');
    final abotParents =
        segments.sublist(0, segments.length - 1).where((s) => s.endsWith('.abot'));
    if (abotParents.isNotEmpty) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Cannot save inside another .abot bundle.')),
      );
      return;
    }

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

