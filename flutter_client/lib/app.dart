import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import 'core/auth/auth_provider.dart';
import 'core/theme/abot_theme.dart';
import 'core/theme/theme_provider.dart';
import 'features/auth/login_screen.dart';
import 'features/facet/facet_shell.dart';

class AbotApp extends ConsumerWidget {
  const AbotApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final themeMode = ref.watch(themeModeProvider);

    return MaterialApp(
      title: 'abot',
      debugShowCheckedModeBanner: false,
      theme: AbotTheme.light,
      darkTheme: AbotTheme.dark,
      themeMode: themeMode,
      initialRoute: Uri.base.path == '/login' ? '/login' : '/',
      onGenerateRoute: (settings) {
        if (settings.name == '/login') {
          return MaterialPageRoute(builder: (_) => const LoginScreen());
        }
        return MaterialPageRoute(
          builder: (_) => const AuthGuard(child: FacetShell()),
        );
      },
    );
  }
}

/// Auth guard that checks localhost bypass or redirects to login.
class AuthGuard extends ConsumerStatefulWidget {
  final Widget child;
  const AuthGuard({super.key, required this.child});

  @override
  ConsumerState<AuthGuard> createState() => _AuthGuardState();
}

class _AuthGuardState extends ConsumerState<AuthGuard> {
  @override
  void initState() {
    super.initState();
    // If not localhost, check auth status
    if (!_isLocalhost) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        ref.read(authProvider.notifier).checkStatus();
      });
    }
  }

  bool get _isLocalhost {
    final hostname = web.window.location.hostname;
    return hostname == 'localhost' ||
        hostname == '127.0.0.1' ||
        hostname == '::1';
  }

  @override
  Widget build(BuildContext context) {
    // Localhost bypass — show child directly
    if (_isLocalhost) {
      return widget.child;
    }

    final auth = ref.watch(authProvider);

    if (auth.isChecking) {
      final isDark = Theme.of(context).brightness == Brightness.dark;
      final subtextColor =
          isDark ? CatppuccinMocha.subtext0 : CatppuccinLatte.subtext0;
      return Scaffold(
        body: Center(
          child: CircularProgressIndicator(color: subtextColor),
        ),
      );
    }

    if (!auth.isAuthenticated) {
      // Redirect to login
      WidgetsBinding.instance.addPostFrameCallback((_) {
        web.window.location.href = '/login';
      });
      return const SizedBox.shrink();
    }

    return widget.child;
  }
}
