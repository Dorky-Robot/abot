import 'package:flutter/material.dart';
import '../../core/network/api_client.dart';
import '../../core/theme/abot_theme.dart';
import 'credential_input.dart';

/// Manages Anthropic credentials in Settings → AI tab (default/global credentials).
/// Supports both setup tokens (Max subscription) and API keys.
class AnthropicOAuthManager extends StatefulWidget {
  const AnthropicOAuthManager({super.key});

  @override
  State<AnthropicOAuthManager> createState() => _AnthropicOAuthManagerState();
}

enum _KeyState { loading, disconnected, connected }

class _AnthropicOAuthManagerState extends State<AnthropicOAuthManager> {
  final _api = const ApiClient();
  final _keyController = TextEditingController();

  _KeyState _state = _KeyState.loading;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    _loadStatus();
  }

  @override
  void dispose() {
    _keyController.dispose();
    super.dispose();
  }

  Future<void> _loadStatus() async {
    try {
      final data = await _api.get('/api/anthropic/key/status')
          as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _state = data['status'] == 'connected'
            ? _KeyState.connected
            : _KeyState.disconnected;
      });
    } catch (_) {
      if (!mounted) return;
      setState(() => _state = _KeyState.disconnected);
    }
  }

  Future<void> _saveKey() async {
    final key = _keyController.text.trim();
    if (key.isEmpty) return;

    setState(() => _saving = true);
    try {
      await _api.post('/api/anthropic/key', {'api_key': key});
      if (!mounted) return;
      setState(() {
        _state = _KeyState.connected;
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

  Future<void> _disconnect() async {
    try {
      await _api.delete('/api/anthropic/key');
      if (!mounted) return;
      setState(() => _state = _KeyState.disconnected);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to remove: $e')),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return switch (_state) {
      _KeyState.loading => Center(
          child: Padding(
            padding: const EdgeInsets.all(AbotSpacing.lg),
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
      _KeyState.disconnected => _buildDisconnected(p),
      _KeyState.connected => CredentialConnectedBadge(
          message: 'Default credentials configured',
          subtitle: 'Applied to new sessions without their own credentials.',
          onDisconnect: _disconnect,
        ),
    };
  }

  Widget _buildDisconnected(CatPalette p) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'To use your Max subscription, run this on the host:',
          style: TextStyle(
            fontSize: 11,
            color: p.subtext0,
            fontFamily: AbotFonts.mono,
          ),
        ),
        const SizedBox(height: AbotSpacing.sm),
        Container(
          padding: const EdgeInsets.all(AbotSpacing.sm),
          decoration: BoxDecoration(
            color: p.surface0,
            borderRadius: BorderRadius.circular(AbotRadius.sm),
          ),
          child: SelectableText(
            'claude setup-token',
            style: TextStyle(
              fontSize: 12,
              color: p.text,
              fontFamily: AbotFonts.mono,
            ),
          ),
        ),
        const SizedBox(height: AbotSpacing.sm),
        Text(
          'Then paste the token below. An API key (sk-ant-...) also works.',
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
          onSave: _saveKey,
        ),
      ],
    );
  }
}
