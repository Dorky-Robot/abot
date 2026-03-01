import 'dart:js_interop';
import 'dart:js_interop_unsafe';
import 'package:web/web.dart' as web;

/// JS interop bindings for xterm.js loaded via globalThis.xtermJs
///
/// The xterm.js ESM modules are loaded in web/index.html and exposed as:
///   globalThis.xtermJs = { Terminal, FitAddon, WebLinksAddon, SearchAddon }

JSObject get _xtermJs => globalContext['xtermJs'] as JSObject;

/// Wrapper for xterm.js Terminal instance
extension type XtermTerminal._(JSObject _) implements JSObject {
  /// Create a new Terminal with options
  factory XtermTerminal(JSObject options) {
    final ctor = (_xtermJs['Terminal'] as JSFunction);
    return ctor.callAsConstructor<XtermTerminal>(options);
  }

  external void open(web.HTMLElement container);
  external void write(JSString data);
  external void dispose();
  external void focus();
  external void blur();
  external void clear();
  external void scrollToBottom();
  external int get cols;
  external int get rows;
  external XtermBuffer get buffer;

  /// Register event listener: onData, onResize, etc.
  external JSObject onData(JSFunction callback);
  external JSObject onResize(JSFunction callback);

  /// Load an addon
  external void loadAddon(JSObject addon);

  /// Attach a custom key event handler. Return false to prevent xterm from
  /// processing the event, true to let it through.
  external void attachCustomKeyEventHandler(JSFunction handler);

  /// Set terminal options
  void setOption(String key, JSAny? value) {
    final options = this['options'] as JSObject;
    options[key] = value;
  }
}

extension type XtermBuffer._(JSObject _) implements JSObject {
  external XtermBufferState get active;
}

extension type XtermBufferState._(JSObject _) implements JSObject {
  external int get cursorY;
  external int get viewportY;
  external int get baseY;
}

/// FitAddon wrapper
extension type XtermFitAddon._(JSObject _) implements JSObject {
  factory XtermFitAddon() {
    final ctor = (_xtermJs['FitAddon'] as JSFunction);
    return ctor.callAsConstructor<XtermFitAddon>();
  }

  external void fit();
  external JSObject? proposeDimensions();
}

/// WebLinksAddon wrapper
extension type XtermWebLinksAddon._(JSObject _) implements JSObject {
  factory XtermWebLinksAddon() {
    final ctor = (_xtermJs['WebLinksAddon'] as JSFunction);
    return ctor.callAsConstructor<XtermWebLinksAddon>();
  }
}

/// SearchAddon wrapper
extension type XtermSearchAddon._(JSObject _) implements JSObject {
  factory XtermSearchAddon() {
    final ctor = (_xtermJs['SearchAddon'] as JSFunction);
    return ctor.callAsConstructor<XtermSearchAddon>();
  }

  external JSBoolean findNext(JSString term);
  external JSBoolean findPrevious(JSString term);
  external void clearDecorations();
}

/// Helper to create xterm options JSObject
JSObject createXtermOptions({
  required int fontSize,
  required String fontFamily,
  required bool cursorBlink,
  required int scrollback,
  required bool convertEol,
  required bool macOptionIsMeta,
  required double minimumContrastRatio,
  required String cursorInactiveStyle,
  required bool rightClickSelectsWord,
  required bool rescaleOverlappingGlyphs,
  JSObject? theme,
}) {
  final obj = JSObject();
  obj['fontSize'] = fontSize.toJS;
  obj['fontFamily'] = fontFamily.toJS;
  obj['cursorBlink'] = cursorBlink.toJS;
  obj['scrollback'] = scrollback.toJS;
  obj['convertEol'] = convertEol.toJS;
  obj['macOptionIsMeta'] = macOptionIsMeta.toJS;
  obj['minimumContrastRatio'] = minimumContrastRatio.toJS;
  obj['cursorInactiveStyle'] = cursorInactiveStyle.toJS;
  obj['rightClickSelectsWord'] = rightClickSelectsWord.toJS;
  obj['rescaleOverlappingGlyphs'] = rescaleOverlappingGlyphs.toJS;
  if (theme != null) obj['theme'] = theme;
  return obj;
}

/// Helper to create xterm theme JSObject from our theme colors
JSObject createXtermThemeJs({
  required String background,
  required String foreground,
  required String cursor,
  required String selectionBackground,
  required String black,
  required String brightBlack,
  required String red,
  required String brightRed,
  required String green,
  required String brightGreen,
  required String yellow,
  required String brightYellow,
  required String blue,
  required String brightBlue,
  required String magenta,
  required String brightMagenta,
  required String cyan,
  required String brightCyan,
  required String white,
  required String brightWhite,
}) {
  final obj = JSObject();
  obj['background'] = background.toJS;
  obj['foreground'] = foreground.toJS;
  obj['cursor'] = cursor.toJS;
  obj['selectionBackground'] = selectionBackground.toJS;
  obj['black'] = black.toJS;
  obj['brightBlack'] = brightBlack.toJS;
  obj['red'] = red.toJS;
  obj['brightRed'] = brightRed.toJS;
  obj['green'] = green.toJS;
  obj['brightGreen'] = brightGreen.toJS;
  obj['yellow'] = yellow.toJS;
  obj['brightYellow'] = brightYellow.toJS;
  obj['blue'] = blue.toJS;
  obj['brightBlue'] = brightBlue.toJS;
  obj['magenta'] = magenta.toJS;
  obj['brightMagenta'] = brightMagenta.toJS;
  obj['cyan'] = cyan.toJS;
  obj['brightCyan'] = brightCyan.toJS;
  obj['white'] = white.toJS;
  obj['brightWhite'] = brightWhite.toJS;
  return obj;
}
