import 'package:flutter/material.dart';
import 'package:web/web.dart' as web;
import '../../core/network/api_client.dart';
import '../../core/theme/abot_theme.dart';

/// Manages Anthropic API key in Settings → AI tab.
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
        SnackBar(content: Text('Failed to save key: $e')),
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
        SnackBar(content: Text('Failed to remove key: $e')),
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
      _KeyState.connected => _buildConnected(p),
    };
  }

  Widget _buildDisconnected(CatPalette p) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Paste your Anthropic API key to enable Claude in all containers.',
          style: TextStyle(
            fontSize: 11,
            color: p.subtext0,
            fontFamily: AbotFonts.mono,
          ),
        ),
        const SizedBox(height: AbotSpacing.sm),
        GestureDetector(
          onTap: () => web.window.open(
            'https://console.anthropic.com/settings/keys',
            '_blank',
          ),
          child: Text(
            'Get an API key from console.anthropic.com',
            style: TextStyle(
              fontSize: 11,
              color: p.blue,
              fontFamily: AbotFonts.mono,
              decoration: TextDecoration.underline,
            ),
          ),
        ),
        const SizedBox(height: AbotSpacing.md),
        Row(
          children: [
            Expanded(
              child: SizedBox(
                height: 32,
                child: TextField(
                  controller: _keyController,
                  obscureText: true,
                  style: TextStyle(
                    fontSize: 12,
                    color: p.text,
                    fontFamily: AbotFonts.mono,
                  ),
                  decoration: InputDecoration(
                    hintText: 'sk-ant-...',
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
                  onSubmitted: (_) => _saveKey(),
                ),
              ),
            ),
            const SizedBox(width: AbotSpacing.sm),
            SizedBox(
              height: 32,
              child: TextButton(
                onPressed: _saving ? null : _saveKey,
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
                child: _saving
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
        ),
      ],
    );
  }

  Widget _buildConnected(CatPalette p) {
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
                  'API key configured',
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
        Text(
          'Claude will be available in new containers automatically.',
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
            onPressed: _disconnect,
            style: TextButton.styleFrom(
              foregroundColor: p.red,
              textStyle: const TextStyle(
                fontSize: 11,
                fontFamily: AbotFonts.mono,
              ),
            ),
            child: const Text('Remove key'),
          ),
        ),
      ],
    );
  }
}
