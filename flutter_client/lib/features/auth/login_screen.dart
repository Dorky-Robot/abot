import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import '../../core/auth/auth_provider.dart';
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

  bool get _isLocalhost {
    final hostname = web.window.location.hostname;
    return hostname == 'localhost' ||
        hostname == '127.0.0.1' ||
        hostname == '::1';
  }

  Future<void> _handleRegister() async {
    final token = _tokenController.text.trim();
    if (token.isEmpty && !_isLocalhost) {
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
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final bgColor = isDark ? CatppuccinMocha.base : CatppuccinLatte.base;
    final cardColor =
        isDark ? CatppuccinMocha.surface0 : CatppuccinLatte.surface0;
    final textColor = isDark ? CatppuccinMocha.text : CatppuccinLatte.text;
    final subtextColor =
        isDark ? CatppuccinMocha.subtext0 : CatppuccinLatte.subtext0;
    final accentColor = isDark ? CatppuccinMocha.mauve : CatppuccinLatte.mauve;
    final errorColor = isDark ? CatppuccinMocha.red : CatppuccinLatte.red;

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
                        cardColor: cardColor,
                        textColor: textColor,
                        subtextColor: subtextColor,
                        accentColor: accentColor,
                        errorColor: errorColor,
                        error: auth.error,
                      )
                    : _buildSetupView(
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
            fontFamily: 'JetBrains Mono',
            fontSize: 14,
            color: subtextColor,
          ),
        ),
      ],
    );
  }

  Widget _buildSetupView({
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
              fontFamily: 'JetBrains Mono',
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
              fontFamily: 'JetBrains Mono',
              fontSize: 14,
              color: subtextColor,
            ),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: AbotSpacing.xl),
          if (!_isLocalhost) ...[
            _buildTokenField(textColor, subtextColor, cardColor),
            const SizedBox(height: AbotSpacing.md),
          ],
          _buildButton(
            label: 'Register Passkey',
            onPressed: _isLoading ? null : _handleRegister,
            accentColor: accentColor,
            textColor: textColor,
          ),
          if (error != null) ...[
            const SizedBox(height: AbotSpacing.md),
            Text(
              error,
              style: TextStyle(
                fontFamily: 'JetBrains Mono',
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
              fontFamily: 'JetBrains Mono',
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
              fontFamily: 'JetBrains Mono',
              fontSize: 14,
              color: subtextColor,
            ),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: AbotSpacing.xl),
          _buildButton(
            label: 'Sign In',
            onPressed: _isLoading ? null : _handleLogin,
            accentColor: accentColor,
            textColor: textColor,
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
                      fontFamily: 'JetBrains Mono',
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
            _buildTokenField(textColor, subtextColor, cardColor),
            const SizedBox(height: AbotSpacing.md),
            _buildButton(
              label: 'Register',
              onPressed: _isLoading ? null : _handleRegister,
              accentColor: accentColor,
              textColor: textColor,
            ),
          ],
          if (error != null) ...[
            const SizedBox(height: AbotSpacing.md),
            Text(
              error,
              style: TextStyle(
                fontFamily: 'JetBrains Mono',
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
      Color textColor, Color subtextColor, Color cardColor) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final fieldBg = isDark ? CatppuccinMocha.mantle : CatppuccinLatte.mantle;
    final borderColor =
        isDark ? CatppuccinMocha.surface1 : CatppuccinLatte.surface1;

    return TextField(
      controller: _tokenController,
      style: TextStyle(
        fontFamily: 'JetBrains Mono',
        fontSize: 14,
        color: textColor,
      ),
      decoration: InputDecoration(
        hintText: 'Setup token',
        hintStyle: TextStyle(
          fontFamily: 'JetBrains Mono',
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
          borderSide: BorderSide(
              color: isDark ? CatppuccinMocha.mauve : CatppuccinLatte.mauve),
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
    required Color accentColor,
    required Color textColor,
  }) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final onAccent = isDark ? CatppuccinMocha.base : CatppuccinLatte.base;

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
            fontFamily: 'JetBrains Mono',
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
