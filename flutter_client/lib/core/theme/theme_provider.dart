import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import 'abot_theme.dart';

/// Theme preference: auto, light, or dark
enum ThemePreference { auto, light, dark }

/// Provider for the current theme mode
final themeModeProvider =
    NotifierProvider<ThemeModeNotifier, ThemeMode>(ThemeModeNotifier.new);

/// Provider for the xterm theme colors (derived from effective theme)
final xtermThemeProvider = Provider<XtermThemeColors>((ref) {
  final mode = ref.watch(themeModeProvider);
  if (mode == ThemeMode.light) return xtermLightTheme;
  if (mode == ThemeMode.dark) return xtermDarkTheme;

  // Auto: check system preference
  final isDark = web.window.matchMedia('(prefers-color-scheme: dark)').matches;
  return isDark ? xtermDarkTheme : xtermLightTheme;
});

class ThemeModeNotifier extends Notifier<ThemeMode> {
  @override
  ThemeMode build() {
    final stored = web.window.localStorage.getItem('theme') ?? 'auto';
    return _prefToMode(stored);
  }

  static ThemeMode _prefToMode(String pref) {
    return switch (pref) {
      'light' => ThemeMode.light,
      'dark' => ThemeMode.dark,
      _ => ThemeMode.system,
    };
  }

  void setPreference(ThemePreference pref) {
    final key = switch (pref) {
      ThemePreference.auto => 'auto',
      ThemePreference.light => 'light',
      ThemePreference.dark => 'dark',
    };
    web.window.localStorage.setItem('theme', key);
    state = _prefToMode(key);
  }
}
