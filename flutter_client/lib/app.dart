import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import 'core/auth/auth_provider.dart';
import 'core/auth/device_utils.dart' show isLocalhost;
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
  bool _redirecting = false;

  @override
  void initState() {
    super.initState();
    // If not localhost, check auth status
    if (!isLocalhost()) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        ref.read(authProvider.notifier).checkStatus();
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    // Localhost bypass — show child directly
    if (isLocalhost()) {
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
      // Redirect to login (guard prevents duplicate redirects)
      if (!_redirecting) {
        _redirecting = true;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          web.window.location.href = '/login';
        });
      }
      return const SizedBox.shrink();
    }

    return widget.child;
  }
}
