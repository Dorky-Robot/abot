import 'package:flutter/material.dart';
import 'package:web/web.dart' as web;
import '../../core/network/api_client.dart';
import '../../core/theme/abot_theme.dart';

/// Manages Anthropic OAuth connection state in Settings → AI tab.
class AnthropicOAuthManager extends StatefulWidget {
  const AnthropicOAuthManager({super.key});

  @override
  State<AnthropicOAuthManager> createState() => _AnthropicOAuthManagerState();
}

enum _OAuthState { loading, disconnected, awaitingCode, connected, expired }

class _AnthropicOAuthManagerState extends State<AnthropicOAuthManager> {
  final _api = const ApiClient();
  final _codeController = TextEditingController();

  _OAuthState _state = _OAuthState.loading;
  String? _authorizeUrl;
  int? _expiresAt;
  bool _exchanging = false;

  @override
  void initState() {
    super.initState();
    _loadStatus();
  }

  @override
  void dispose() {
    _codeController.dispose();
    super.dispose();
  }

  Future<void> _loadStatus() async {
    try {
      final data = await _api.get('/api/anthropic/oauth/status')
          as Map<String, dynamic>;
      if (!mounted) return;
      final status = data['status'] as String? ?? 'disconnected';
      setState(() {
        _expiresAt = data['expires_at'] as int?;
        switch (status) {
          case 'connected':
            _state = _OAuthState.connected;
          case 'expired':
            _state = _OAuthState.expired;
          default:
            _state = _OAuthState.disconnected;
        }
      });
    } catch (_) {
      if (!mounted) return;
      setState(() => _state = _OAuthState.disconnected);
    }
  }

  Future<void> _initOAuth() async {
    try {
      final data = await _api.post('/api/anthropic/oauth/init')
          as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _authorizeUrl = data['authorize_url'] as String?;
        _state = _OAuthState.awaitingCode;
      });
      // Open authorize URL in new tab
      if (_authorizeUrl != null) {
        web.window.open(_authorizeUrl!, '_blank');
      }
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to start OAuth: $e')),
      );
    }
  }

  Future<void> _exchangeCode() async {
    final code = _codeController.text.trim();
    if (code.isEmpty) return;

    setState(() => _exchanging = true);
    try {
      final data = await _api.post(
        '/api/anthropic/oauth/exchange',
        {'code': code},
      ) as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _expiresAt = data['expires_at'] as int?;
        _state = _OAuthState.connected;
        _exchanging = false;
        _authorizeUrl = null;
      });
      _codeController.clear();
    } catch (e) {
      if (!mounted) return;
      setState(() => _exchanging = false);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to exchange code: $e')),
      );
    }
  }

  Future<void> _disconnect() async {
    try {
      await _api.delete('/api/anthropic/oauth');
      if (!mounted) return;
      setState(() {
        _state = _OAuthState.disconnected;
        _expiresAt = null;
        _authorizeUrl = null;
      });
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to disconnect: $e')),
      );
    }
  }

  String _formatExpiry(int expiresAt) {
    final dt = DateTime.fromMillisecondsSinceEpoch(expiresAt * 1000);
    final now = DateTime.now();
    if (dt.isBefore(now)) return 'Expired';
    final diff = dt.difference(now);
    if (diff.inMinutes < 60) return 'Expires in ${diff.inMinutes}m';
    if (diff.inHours < 24) return 'Expires in ${diff.inHours}h';
    return 'Expires in ${diff.inDays}d';
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return switch (_state) {
      _OAuthState.loading => Center(
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
      _OAuthState.disconnected => _buildDisconnected(p),
      _OAuthState.awaitingCode => _buildAwaitingCode(p),
      _OAuthState.connected => _buildConnected(p),
      _OAuthState.expired => _buildExpired(p),
    };
  }

  Widget _buildDisconnected(CatPalette p) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Connect your Anthropic account to enable Claude Code in all containers without manual login.',
          style: TextStyle(
            fontSize: 11,
            color: p.subtext0,
            fontFamily: AbotFonts.mono,
          ),
        ),
        const SizedBox(height: AbotSpacing.md),
        SizedBox(
          height: 32,
          width: double.infinity,
          child: TextButton(
            onPressed: _initOAuth,
            style: TextButton.styleFrom(
              backgroundColor: p.mauve,
              foregroundColor: p.base,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(AbotRadius.sm),
              ),
              textStyle: const TextStyle(
                fontSize: 11,
                fontFamily: AbotFonts.mono,
                fontWeight: FontWeight.w600,
              ),
            ),
            child: const Text('Connect to Anthropic'),
          ),
        ),
      ],
    );
  }

  Widget _buildAwaitingCode(CatPalette p) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Authorize in the browser tab that opened, then paste the code below.',
          style: TextStyle(
            fontSize: 11,
            color: p.subtext0,
            fontFamily: AbotFonts.mono,
          ),
        ),
        if (_authorizeUrl != null) ...[
          const SizedBox(height: AbotSpacing.sm),
          GestureDetector(
            onTap: () => web.window.open(_authorizeUrl!, '_blank'),
            child: Text(
              'Open authorize page',
              style: TextStyle(
                fontSize: 11,
                color: p.blue,
                fontFamily: AbotFonts.mono,
                decoration: TextDecoration.underline,
              ),
            ),
          ),
        ],
        const SizedBox(height: AbotSpacing.md),
        Row(
          children: [
            Expanded(
              child: SizedBox(
                height: 32,
                child: TextField(
                  controller: _codeController,
                  style: TextStyle(
                    fontSize: 12,
                    color: p.text,
                    fontFamily: AbotFonts.mono,
                  ),
                  decoration: InputDecoration(
                    hintText: 'Paste code here...',
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
                  onSubmitted: (_) => _exchangeCode(),
                ),
              ),
            ),
            const SizedBox(width: AbotSpacing.sm),
            SizedBox(
              height: 32,
              child: TextButton(
                onPressed: _exchanging ? null : _exchangeCode,
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
                child: _exchanging
                    ? SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: p.base,
                        ),
                      )
                    : const Text('Submit'),
              ),
            ),
          ],
        ),
        const SizedBox(height: AbotSpacing.sm),
        GestureDetector(
          onTap: () => setState(() {
            _state = _OAuthState.disconnected;
            _authorizeUrl = null;
          }),
          child: Text(
            'Cancel',
            style: TextStyle(
              fontSize: 11,
              color: p.subtext0,
              fontFamily: AbotFonts.mono,
            ),
          ),
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
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Connected to Anthropic',
                      style: TextStyle(
                        fontSize: 11,
                        color: p.green,
                        fontFamily: AbotFonts.mono,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    if (_expiresAt != null)
                      Text(
                        _formatExpiry(_expiresAt!),
                        style: TextStyle(
                          fontSize: 10,
                          color: p.overlay0,
                          fontFamily: AbotFonts.mono,
                        ),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: AbotSpacing.sm),
        Text(
          'Claude Code in new containers will authenticate automatically.',
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
            child: const Text('Disconnect'),
          ),
        ),
      ],
    );
  }

  Widget _buildExpired(CatPalette p) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Container(
          padding: const EdgeInsets.all(AbotSpacing.md),
          decoration: BoxDecoration(
            color: p.surface0,
            borderRadius: BorderRadius.circular(AbotRadius.md),
            border: Border.all(color: p.yellow, width: 0.5),
          ),
          child: Row(
            children: [
              Icon(Icons.warning_amber, size: 16, color: p.yellow),
              const SizedBox(width: AbotSpacing.sm),
              Expanded(
                child: Text(
                  'Token expired — reconnect to continue.',
                  style: TextStyle(
                    fontSize: 11,
                    color: p.yellow,
                    fontFamily: AbotFonts.mono,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: AbotSpacing.md),
        SizedBox(
          height: 32,
          width: double.infinity,
          child: TextButton(
            onPressed: _initOAuth,
            style: TextButton.styleFrom(
              backgroundColor: p.mauve,
              foregroundColor: p.base,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(AbotRadius.sm),
              ),
              textStyle: const TextStyle(
                fontSize: 11,
                fontFamily: AbotFonts.mono,
                fontWeight: FontWeight.w600,
              ),
            ),
            child: const Text('Reconnect'),
          ),
        ),
        const SizedBox(height: AbotSpacing.sm),
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
            child: const Text('Disconnect'),
          ),
        ),
      ],
    );
  }
}
