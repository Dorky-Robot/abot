import 'package:flutter/material.dart';

/// Catppuccin color palettes
class CatppuccinMocha {
  CatppuccinMocha._();
  static const base = Color(0xFF1E1E2E);
  static const mantle = Color(0xFF181825);
  static const crust = Color(0xFF11111B);
  static const surface0 = Color(0xFF313244);
  static const surface1 = Color(0xFF45475A);
  static const surface2 = Color(0xFF585B70);
  static const overlay0 = Color(0xFF6C7086);
  static const overlay1 = Color(0xFF7F849C);
  static const overlay2 = Color(0xFF9399B2);
  static const subtext0 = Color(0xFFA6ADC8);
  static const subtext1 = Color(0xFFBAC2DE);
  static const text = Color(0xFFCDD6F4);
  static const rosewater = Color(0xFFF5E0DC);
  static const flamingo = Color(0xFFF2CDCD);
  static const pink = Color(0xFFF5C2E7);
  static const mauve = Color(0xFFCBA6F7);
  static const red = Color(0xFFF38BA8);
  static const maroon = Color(0xFFEBA0AC);
  static const peach = Color(0xFFFAB387);
  static const yellow = Color(0xFFF9E2AF);
  static const green = Color(0xFFA6E3A1);
  static const teal = Color(0xFF94E2D5);
  static const sky = Color(0xFF89DCEB);
  static const sapphire = Color(0xFF74C7EC);
  static const blue = Color(0xFF89B4FA);
  static const lavender = Color(0xFFB4BEFE);
}

class CatppuccinLatte {
  CatppuccinLatte._();
  static const base = Color(0xFFEFF1F5);
  static const mantle = Color(0xFFE6E9EF);
  static const crust = Color(0xFFDCE0E8);
  static const surface0 = Color(0xFFCCD0DA);
  static const surface1 = Color(0xFFBCC0CC);
  static const surface2 = Color(0xFFACB0BE);
  static const overlay0 = Color(0xFF9CA0B0);
  static const overlay1 = Color(0xFF8C8FA1);
  static const overlay2 = Color(0xFF7C7F93);
  static const subtext0 = Color(0xFF6C6F85);
  static const subtext1 = Color(0xFF5C5F77);
  static const text = Color(0xFF4C4F69);
  static const rosewater = Color(0xFFDC8A78);
  static const flamingo = Color(0xFFDD7878);
  static const pink = Color(0xFFEA76CB);
  static const mauve = Color(0xFF8839EF);
  static const red = Color(0xFFD20F39);
  static const maroon = Color(0xFFE64553);
  static const peach = Color(0xFFFE640B);
  static const yellow = Color(0xFFDF8E1D);
  static const green = Color(0xFF40A02B);
  static const teal = Color(0xFF179299);
  static const sky = Color(0xFF04A5E5);
  static const sapphire = Color(0xFF209FB5);
  static const blue = Color(0xFF1E66F5);
  static const lavender = Color(0xFF7287FD);
}

/// Design tokens matching client/design-tokens.css
class AbotSpacing {
  AbotSpacing._();
  static const double xs = 4;
  static const double sm = 8;
  static const double md = 12;
  static const double lg = 16;
  static const double xl = 24;
  static const double xxl = 32;
}

class AbotRadius {
  AbotRadius._();
  static const double sm = 6;
  static const double md = 8;
  static const double lg = 12;
}

class AbotSizes {
  AbotSizes._();
  static const double barHeight = 44; // matches --height-bar
  static const double buttonSmHeight = 32;
  static const double buttonMdHeight = 44; // iOS touch target
  static const double titleBarHeight = 32;
  static const double sidebarExpandedWidth = 200;
  static const double sidebarCollapsedWidth = 44;
}

class AbotFonts {
  AbotFonts._();
  static const String mono = 'JetBrains Mono';
  static const String xtermStack =
      "'JetBrains Mono', 'SF Mono', Monaco, 'Cascadia Code', 'Roboto Mono', Consolas, 'Courier New', monospace";
}

/// Resolved Catppuccin palette — picks Mocha or Latte based on brightness.
class CatPalette {
  final bool isDark;
  const CatPalette(this.isDark);

  Color get base => isDark ? CatppuccinMocha.base : CatppuccinLatte.base;
  Color get mantle => isDark ? CatppuccinMocha.mantle : CatppuccinLatte.mantle;
  Color get surface0 => isDark ? CatppuccinMocha.surface0 : CatppuccinLatte.surface0;
  Color get surface1 => isDark ? CatppuccinMocha.surface1 : CatppuccinLatte.surface1;
  Color get subtext0 => isDark ? CatppuccinMocha.subtext0 : CatppuccinLatte.subtext0;
  Color get text => isDark ? CatppuccinMocha.text : CatppuccinLatte.text;
  Color get blue => isDark ? CatppuccinMocha.blue : CatppuccinLatte.blue;
  Color get red => isDark ? CatppuccinMocha.red : CatppuccinLatte.red;
  Color get green => isDark ? CatppuccinMocha.green : CatppuccinLatte.green;
  Color get yellow => isDark ? CatppuccinMocha.yellow : CatppuccinLatte.yellow;
  Color get mauve => isDark ? CatppuccinMocha.mauve : CatppuccinLatte.mauve;
  Color get overlay0 => isDark ? CatppuccinMocha.overlay0 : CatppuccinLatte.overlay0;
  Color get overlay1 => isDark ? CatppuccinMocha.overlay1 : CatppuccinLatte.overlay1;
}

extension AbotColors on BuildContext {
  bool get isDark => Theme.of(this).brightness == Brightness.dark;
  CatPalette get palette => CatPalette(isDark);
}

/// xterm.js theme objects for JS interop
class XtermThemeColors {
  final String background;
  final String foreground;
  final String cursor;
  final String selectionBackground;
  final String black;
  final String brightBlack;
  final String red;
  final String brightRed;
  final String green;
  final String brightGreen;
  final String yellow;
  final String brightYellow;
  final String blue;
  final String brightBlue;
  final String magenta;
  final String brightMagenta;
  final String cyan;
  final String brightCyan;
  final String white;
  final String brightWhite;

  const XtermThemeColors({
    required this.background,
    required this.foreground,
    required this.cursor,
    required this.selectionBackground,
    required this.black,
    required this.brightBlack,
    required this.red,
    required this.brightRed,
    required this.green,
    required this.brightGreen,
    required this.yellow,
    required this.brightYellow,
    required this.blue,
    required this.brightBlue,
    required this.magenta,
    required this.brightMagenta,
    required this.cyan,
    required this.brightCyan,
    required this.white,
    required this.brightWhite,
  });
}

const xtermDarkTheme = XtermThemeColors(
  background: '#1e1e2e',
  foreground: '#cdd6f4',
  cursor: '#f5e0dc',
  selectionBackground: 'rgba(137,180,250,0.3)',
  black: '#45475a',
  brightBlack: '#585b70',
  red: '#f38ba8',
  brightRed: '#f38ba8',
  green: '#a6e3a1',
  brightGreen: '#a6e3a1',
  yellow: '#f9e2af',
  brightYellow: '#f9e2af',
  blue: '#89b4fa',
  brightBlue: '#89b4fa',
  magenta: '#f5c2e7',
  brightMagenta: '#f5c2e7',
  cyan: '#94e2d5',
  brightCyan: '#94e2d5',
  white: '#bac2de',
  brightWhite: '#a6adc8',
);

const xtermLightTheme = XtermThemeColors(
  background: '#eff1f5',
  foreground: '#4c4f69',
  cursor: '#dc8a78',
  selectionBackground: 'rgba(30,102,245,0.2)',
  black: '#5c5f77',
  brightBlack: '#6c6f85',
  red: '#d20f39',
  brightRed: '#d20f39',
  green: '#40a02b',
  brightGreen: '#40a02b',
  yellow: '#df8e1d',
  brightYellow: '#df8e1d',
  blue: '#1e66f5',
  brightBlue: '#1e66f5',
  magenta: '#ea76cb',
  brightMagenta: '#ea76cb',
  cyan: '#179299',
  brightCyan: '#179299',
  white: '#acb0be',
  brightWhite: '#bcc0cc',
);

/// Flutter ThemeData for the app chrome (non-terminal UI)
class AbotTheme {
  AbotTheme._();

  static ThemeData get dark => ThemeData(
        brightness: Brightness.dark,
        scaffoldBackgroundColor: CatppuccinMocha.base,
        colorScheme: const ColorScheme.dark(
          surface: CatppuccinMocha.base,
          primary: CatppuccinMocha.blue,
          secondary: CatppuccinMocha.mauve,
          error: CatppuccinMocha.red,
          onSurface: CatppuccinMocha.text,
          onPrimary: CatppuccinMocha.base,
        ),
        textTheme: const TextTheme(
          bodyMedium: TextStyle(
            fontFamily: AbotFonts.mono,
            color: CatppuccinMocha.text,
          ),
        ),
        iconTheme: const IconThemeData(color: CatppuccinMocha.subtext0),
        dividerColor: CatppuccinMocha.surface1,
      );

  static ThemeData get light => ThemeData(
        brightness: Brightness.light,
        scaffoldBackgroundColor: CatppuccinLatte.base,
        colorScheme: const ColorScheme.light(
          surface: CatppuccinLatte.base,
          primary: CatppuccinLatte.blue,
          secondary: CatppuccinLatte.mauve,
          error: CatppuccinLatte.red,
          onSurface: CatppuccinLatte.text,
          onPrimary: CatppuccinLatte.base,
        ),
        textTheme: const TextTheme(
          bodyMedium: TextStyle(
            fontFamily: AbotFonts.mono,
            color: CatppuccinLatte.text,
          ),
        ),
        iconTheme: const IconThemeData(color: CatppuccinLatte.subtext0),
        dividerColor: CatppuccinLatte.surface1,
      );
}
