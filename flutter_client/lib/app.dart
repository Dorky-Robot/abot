import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'core/theme/abot_theme.dart';
import 'core/theme/theme_provider.dart';
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
      home: const FacetShell(),
    );
  }
}
