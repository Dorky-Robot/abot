import 'package:flutter/material.dart';
import '../../core/network/api_client.dart';
import '../../core/theme/abot_theme.dart';
import 'credential_input.dart';

/// Per-session settings overlay — opened from session gear icon.
/// Shows session info, per-session credentials, and export button.
class SessionSettingsPanel extends StatefulWidget {
  final String sessionName;
  final VoidCallback onClose;

  const SessionSettingsPanel({
    super.key,
    required this.sessionName,
    required this.onClose,
  });

  @override
  State<SessionSettingsPanel> createState() => _SessionSettingsPanelState();
}

enum _CredState { loading, disconnected, connected }

class _SessionSettingsPanelState extends State<SessionSettingsPanel> {
  final _api = const ApiClient();
  final _keyController = TextEditingController();

  _CredState _credState = _CredState.loading;
  bool _saving = false;
  bool _exporting = false;
  String? _exportPath;

  @override
  void initState() {
    super.initState();
    _loadCredentialStatus();
  }

  @override
  void dispose() {
    _keyController.dispose();
    super.dispose();
  }

  Future<void> _loadCredentialStatus() async {
    final url =
        '/sessions/${Uri.encodeComponent(widget.sessionName)}/credentials/status';
    try {
      final data = await _api.get(url) as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _credState = data['status'] == 'connected'
            ? _CredState.connected
            : _CredState.disconnected;
      });
    } catch (_) {
      if (!mounted) return;
      setState(() => _credState = _CredState.disconnected);
    }
  }

  Future<void> _saveCredential() async {
    final key = _keyController.text.trim();
    if (key.isEmpty) return;

    setState(() => _saving = true);
    final url =
        '/sessions/${Uri.encodeComponent(widget.sessionName)}/credentials';
    try {
      await _api.post(url, {'api_key': key});
      if (!mounted) return;
      setState(() {
        _credState = _CredState.connected;
        _saving = false;
      });
      _keyController.clear();
    } catch (e) {
      if (!mounted) return;
      setState(() => _saving = false);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to save: $e')),
      );
    }
  }

  Future<void> _removeCredential() async {
    final url =
        '/sessions/${Uri.encodeComponent(widget.sessionName)}/credentials';
    try {
      await _api.delete(url);
      if (!mounted) return;
      setState(() => _credState = _CredState.disconnected);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to remove: $e')),
      );
    }
  }

  Future<void> _exportSession() async {
    setState(() => _exporting = true);
    final url =
        '/sessions/${Uri.encodeComponent(widget.sessionName)}/export';
    try {
      final data = await _api.post(url, {}) as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _exporting = false;
        _exportPath = data['path'] as String?;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _exporting = false);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Export failed: $e')),
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
              constraints: const BoxConstraints(maxHeight: 440),
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
                          child: Text(
                            widget.sessionName,
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
                        // Credentials section
                        _SectionLabel(label: 'Session Credentials'),
                        const SizedBox(height: AbotSpacing.sm),
                        _buildCredentialSection(p),

                        const SizedBox(height: AbotSpacing.xl),

                        // Export section
                        _SectionLabel(label: 'Export'),
                        const SizedBox(height: AbotSpacing.sm),
                        _buildExportSection(p),
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

  Widget _buildCredentialSection(CatPalette p) {
    return switch (_credState) {
      _CredState.loading => Padding(
          padding: const EdgeInsets.all(AbotSpacing.md),
          child: Center(
            child: SizedBox(
              width: 18,
              height: 18,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                color: p.overlay0,
              ),
            ),
          ),
        ),
      _CredState.disconnected => Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Override the default credentials for this session only.',
              style: TextStyle(
                fontSize: 11,
                color: p.subtext0,
                fontFamily: AbotFonts.mono,
              ),
            ),
            const SizedBox(height: AbotSpacing.md),
            CredentialInput(
              controller: _keyController,
              saving: _saving,
              onSave: _saveCredential,
            ),
          ],
        ),
      _CredState.connected => CredentialConnectedBadge(
          message: 'Session credentials set',
          subtitle: 'This session uses its own credentials.',
          onDisconnect: _removeCredential,
        ),
    };
  }

  Widget _buildExportSection(CatPalette p) {
    if (_exportPath != null) {
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
                    'Exported',
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
          const SizedBox(height: AbotSpacing.sm),
          SelectableText(
            _exportPath!,
            style: TextStyle(
              fontSize: 10,
              color: p.subtext0,
              fontFamily: AbotFonts.mono,
            ),
          ),
        ],
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Save this session as a portable .abot bundle.',
          style: TextStyle(
            fontSize: 11,
            color: p.subtext0,
            fontFamily: AbotFonts.mono,
          ),
        ),
        const SizedBox(height: AbotSpacing.md),
        SizedBox(
          height: 32,
          child: TextButton(
            onPressed: _exporting ? null : _exportSession,
            style: TextButton.styleFrom(
              backgroundColor: p.surface1,
              foregroundColor: p.text,
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
            child: _exporting
                ? SizedBox(
                    width: 14,
                    height: 14,
                    child: CircularProgressIndicator(
                      strokeWidth: 2,
                      color: p.text,
                    ),
                  )
                : const Text('Export as .abot'),
          ),
        ),
      ],
    );
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
