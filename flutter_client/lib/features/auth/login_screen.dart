import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/auth/auth_provider.dart';
import '../../core/auth/device_utils.dart' show isLocalhost;
import '../../core/theme/abot_theme.dart';

/// Login/setup screen — matches the vanilla client's login.html flow.
class LoginScreen extends ConsumerStatefulWidget {
  const LoginScreen({super.key});

  @override
  ConsumerState<LoginScreen> createState() => _LoginScreenState();
}

class _LoginScreenState extends ConsumerState<LoginScreen> {
  final _tokenController = TextEditingController();
  bool _showRegisterFields = false;
  bool _isLoading = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      ref.read(authProvider.notifier).checkStatus();
    });
  }

  @override
  void dispose() {
    _tokenController.dispose();
    super.dispose();
  }

  Future<void> _handleRegister() async {
    final token = _tokenController.text.trim();
    if (token.isEmpty && !isLocalhost()) {
      ref.read(authProvider.notifier).setError(
          'Setup token is required for remote registration.');
      return;
    }
    setState(() => _isLoading = true);
    await ref.read(authProvider.notifier).register(token);
    if (mounted) setState(() => _isLoading = false);
  }

  Future<void> _handleLogin() async {
    setState(() => _isLoading = true);
    await ref.read(authProvider.notifier).login();
    if (mounted) setState(() => _isLoading = false);
  }

  @override
  Widget build(BuildContext context) {
    final auth = ref.watch(authProvider);
    final p = context.palette;
    final bgColor = p.base;
    final cardColor = p.surface0;
    final textColor = p.text;
    final subtextColor = p.subtext0;
    final accentColor = p.mauve;
    final errorColor = p.red;

    return Scaffold(
      backgroundColor: bgColor,
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 400),
          child: Padding(
            padding: const EdgeInsets.all(AbotSpacing.xl),
            child: auth.isChecking
                ? _buildLoading(subtextColor)
                : auth.isSetup
                    ? _buildLoginView(
                        isDark: p.isDark,
                        cardColor: cardColor,
                        textColor: textColor,
                        subtextColor: subtextColor,
                        accentColor: accentColor,
                        errorColor: errorColor,
                        error: auth.error,
                      )
                    : _buildSetupView(
                        isDark: p.isDark,
                        cardColor: cardColor,
                        textColor: textColor,
                        subtextColor: subtextColor,
                        accentColor: accentColor,
                        errorColor: errorColor,
                        error: auth.error,
                      ),
          ),
        ),
      ),
    );
  }

  Widget _buildLoading(Color subtextColor) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        CircularProgressIndicator(color: subtextColor),
        const SizedBox(height: AbotSpacing.lg),
        Text(
          'Checking authentication...',
          style: TextStyle(
            fontFamily: AbotFonts.mono,
            fontSize: 14,
            color: subtextColor,
          ),
        ),
      ],
    );
  }

  Widget _buildSetupView({
    required bool isDark,
    required Color cardColor,
    required Color textColor,
    required Color subtextColor,
    required Color accentColor,
    required Color errorColor,
    String? error,
  }) {
    return Container(
      padding: const EdgeInsets.all(AbotSpacing.xl),
      decoration: BoxDecoration(
        color: cardColor,
        borderRadius: BorderRadius.circular(AbotRadius.lg),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            'abot',
            style: TextStyle(
              fontFamily: AbotFonts.mono,
              fontSize: 24,
              fontWeight: FontWeight.bold,
              color: accentColor,
            ),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: AbotSpacing.sm),
          Text(
            'Register your first passkey',
            style: TextStyle(
              fontFamily: AbotFonts.mono,
              fontSize: 14,
              color: subtextColor,
            ),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: AbotSpacing.xl),
          if (!isLocalhost()) ...[
            _buildTokenField(isDark: isDark, textColor: textColor, subtextColor: subtextColor),
            const SizedBox(height: AbotSpacing.md),
          ],
          _buildButton(
            label: 'Register Passkey',
            onPressed: _isLoading ? null : _handleRegister,
            isDark: isDark,
            accentColor: accentColor,
          ),
          if (error != null) ...[
            const SizedBox(height: AbotSpacing.md),
            Text(
              error,
              style: TextStyle(
                fontFamily: AbotFonts.mono,
                fontSize: 12,
                color: errorColor,
              ),
              textAlign: TextAlign.center,
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildLoginView({
    required bool isDark,
    required Color cardColor,
    required Color textColor,
    required Color subtextColor,
    required Color accentColor,
    required Color errorColor,
    String? error,
  }) {
    return Container(
      padding: const EdgeInsets.all(AbotSpacing.xl),
      decoration: BoxDecoration(
        color: cardColor,
        borderRadius: BorderRadius.circular(AbotRadius.lg),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            'abot',
            style: TextStyle(
              fontFamily: AbotFonts.mono,
              fontSize: 24,
              fontWeight: FontWeight.bold,
              color: accentColor,
            ),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: AbotSpacing.sm),
          Text(
            'Sign in with passkey',
            style: TextStyle(
              fontFamily: AbotFonts.mono,
              fontSize: 14,
              color: subtextColor,
            ),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: AbotSpacing.xl),
          _buildButton(
            label: 'Sign In',
            onPressed: _isLoading ? null : _handleLogin,
            isDark: isDark,
            accentColor: accentColor,
          ),
          const SizedBox(height: AbotSpacing.lg),
          // Register new passkey section
          InkWell(
            onTap: () =>
                setState(() => _showRegisterFields = !_showRegisterFields),
            borderRadius: BorderRadius.circular(AbotRadius.sm),
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: AbotSpacing.xs),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(
                    _showRegisterFields
                        ? Icons.expand_less
                        : Icons.expand_more,
                    size: 16,
                    color: subtextColor,
                  ),
                  const SizedBox(width: AbotSpacing.xs),
                  Text(
                    'Register New Passkey',
                    style: TextStyle(
                      fontFamily: AbotFonts.mono,
                      fontSize: 12,
                      color: subtextColor,
                    ),
                  ),
                ],
              ),
            ),
          ),
          if (_showRegisterFields) ...[
            const SizedBox(height: AbotSpacing.md),
            _buildTokenField(isDark: isDark, textColor: textColor, subtextColor: subtextColor),
            const SizedBox(height: AbotSpacing.md),
            _buildButton(
              label: 'Register',
              onPressed: _isLoading ? null : _handleRegister,
              isDark: isDark,
              accentColor: accentColor,
            ),
          ],
          if (error != null) ...[
            const SizedBox(height: AbotSpacing.md),
            Text(
              error,
              style: TextStyle(
                fontFamily: AbotFonts.mono,
                fontSize: 12,
                color: errorColor,
              ),
              textAlign: TextAlign.center,
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildTokenField(
      {required bool isDark,
      required Color textColor,
      required Color subtextColor}) {
    final p = CatPalette(isDark);
    final fieldBg = p.mantle;
    final borderColor = p.surface1;

    return TextField(
      controller: _tokenController,
      style: TextStyle(
        fontFamily: AbotFonts.mono,
        fontSize: 14,
        color: textColor,
      ),
      decoration: InputDecoration(
        hintText: 'Setup token',
        hintStyle: TextStyle(
          fontFamily: AbotFonts.mono,
          fontSize: 14,
          color: subtextColor.withValues(alpha: 0.5),
        ),
        filled: true,
        fillColor: fieldBg,
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
          borderSide: BorderSide(color: p.mauve),
        ),
        contentPadding: const EdgeInsets.symmetric(
          horizontal: AbotSpacing.md,
          vertical: AbotSpacing.sm,
        ),
      ),
    );
  }

  Widget _buildButton({
    required String label,
    required VoidCallback? onPressed,
    required bool isDark,
    required Color accentColor,
  }) {
    final onAccent = CatPalette(isDark).base;

    return SizedBox(
      height: AbotSizes.buttonMdHeight,
      child: ElevatedButton(
        onPressed: onPressed,
        style: ElevatedButton.styleFrom(
          backgroundColor: accentColor,
          foregroundColor: onAccent,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AbotRadius.sm),
          ),
          textStyle: const TextStyle(
            fontFamily: AbotFonts.mono,
            fontSize: 14,
            fontWeight: FontWeight.w500,
          ),
        ),
        child: _isLoading
            ? SizedBox(
                width: 20,
                height: 20,
                child: CircularProgressIndicator(
                  strokeWidth: 2,
                  color: onAccent,
                ),
              )
            : Text(label),
      ),
    );
  }
}
