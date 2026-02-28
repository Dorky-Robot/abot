import 'dart:async';
import 'dart:js_interop';
import 'dart:js_interop_unsafe';
import 'dart:ui_web' as ui_web;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import '../../core/js_interop/xterm_interop.dart';
import '../../core/theme/abot_theme.dart';
import '../../core/theme/theme_provider.dart';
import '../../core/network/websocket_service.dart';
import 'search_bar.dart';

/// A single terminal facet backed by xterm.js via HtmlElementView
class TerminalFacet extends ConsumerStatefulWidget {
  final String facetId;
  final String sessionName;
  final bool isFocused;
  final VoidCallback? onClose;
  final bool showTitleBar;

  const TerminalFacet({
    super.key,
    required this.facetId,
    required this.sessionName,
    this.isFocused = false,
    this.onClose,
    this.showTitleBar = true,
  });

  @override
  ConsumerState<TerminalFacet> createState() => _TerminalFacetState();
}

class _TerminalFacetState extends ConsumerState<TerminalFacet>
    implements TerminalSink {
  @override
  String get sessionName => widget.sessionName;

  late final String _viewId;
  XtermTerminal? _terminal;
  XtermFitAddon? _fitAddon;
  XtermSearchAddon? _searchAddon;
  web.ResizeObserver? _resizeObserver;
  web.HTMLElement? _container;
  bool _registered = false;
  Timer? _fitDebounce;
  bool _showSearch = false;

  @override
  void initState() {
    super.initState();
    _viewId = 'xterm-${widget.facetId}';
    _registerView();
  }

  void _registerView() {
    if (_registered) return;
    _registered = true;

    ui_web.platformViewRegistry.registerViewFactory(_viewId, (int viewId) {
      final container = web.document.createElement('div') as web.HTMLDivElement;
      container.className = 'xterm-container';
      container.style.width = '100%';
      container.style.height = '100%';

      // Defer terminal creation to after the element is in the DOM
      Future.microtask(() => _initTerminal(container));

      return container;
    });
  }

  void _initTerminal(web.HTMLElement container) {
    if (!mounted) return;
    _container = container;
    final xtermTheme = ref.read(xtermThemeProvider);
    final themeJs = createXtermThemeJs(
      background: xtermTheme.background,
      foreground: xtermTheme.foreground,
      cursor: xtermTheme.cursor,
      selectionBackground: xtermTheme.selectionBackground,
      black: xtermTheme.black,
      brightBlack: xtermTheme.brightBlack,
      red: xtermTheme.red,
      brightRed: xtermTheme.brightRed,
      green: xtermTheme.green,
      brightGreen: xtermTheme.brightGreen,
      yellow: xtermTheme.yellow,
      brightYellow: xtermTheme.brightYellow,
      blue: xtermTheme.blue,
      brightBlue: xtermTheme.brightBlue,
      magenta: xtermTheme.magenta,
      brightMagenta: xtermTheme.brightMagenta,
      cyan: xtermTheme.cyan,
      brightCyan: xtermTheme.brightCyan,
      white: xtermTheme.white,
      brightWhite: xtermTheme.brightWhite,
    );

    final options = createXtermOptions(
      fontSize: 14,
      fontFamily: AbotFonts.xtermStack,
      cursorBlink: true,
      scrollback: 10000,
      convertEol: true,
      macOptionIsMeta: true,
      minimumContrastRatio: 4.5,
      cursorInactiveStyle: 'outline',
      rightClickSelectsWord: true,
      rescaleOverlappingGlyphs: true,
      theme: themeJs,
    );

    _terminal = XtermTerminal(options);

    // Load addons
    _fitAddon = XtermFitAddon();
    _terminal!.loadAddon(_fitAddon!);

    _searchAddon = XtermSearchAddon();
    _terminal!.loadAddon(_searchAddon!);

    final webLinksAddon = XtermWebLinksAddon();
    _terminal!.loadAddon(webLinksAddon);

    // Open terminal in container
    _terminal!.open(container);

    // Intercept app-level shortcuts so xterm doesn't consume them.
    // Return false to block xterm from processing, true to let through.
    _terminal!.attachCustomKeyEventHandler(((web.KeyboardEvent event) {
      // Ctrl+Tab / Ctrl+` — cycle focus
      if (event.ctrlKey && (event.key == 'Tab' || event.key == '`')) {
        return false.toJS;
      }
      // Ctrl+N / Cmd+N — new session
      if ((event.ctrlKey || event.metaKey) && event.key == 'n') {
        return false.toJS;
      }
      // Ctrl+W / Cmd+W — close facet
      if ((event.ctrlKey || event.metaKey) && event.key == 'w') {
        return false.toJS;
      }
      // Ctrl+Shift+F / Cmd+Shift+F — search
      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key == 'F') {
        return false.toJS;
      }

      // macOS: translate Cmd+key → Ctrl+key for terminal use.
      // Native terminal emulators treat Cmd as Ctrl for most keys.
      // Skip browser-reserved combos (copy/paste/select-all/etc).
      if (event.metaKey &&
          !event.ctrlKey &&
          !event.shiftKey &&
          event.type == 'keydown') {
        final key = event.key.toLowerCase();
        const browserReserved = {
          'c', 'v', 'a', 'x', 'z', // clipboard / undo
          'r', 'l', 't', 'q', // browser navigation
        };
        if (key.length == 1 && !browserReserved.contains(key)) {
          final code = key.codeUnitAt(0);
          if (code >= 97 && code <= 122) {
            // Send Ctrl+letter (ASCII 1-26)
            final wsService = ref.read(wsServiceProvider.notifier);
            wsService.sendInput(
              String.fromCharCode(code - 96),
              session: widget.sessionName,
            );
            event.preventDefault();
            return false.toJS;
          }
        }
        // Cmd+Backspace → Ctrl+U (kill line)
        if (event.key == 'Backspace') {
          final wsService = ref.read(wsServiceProvider.notifier);
          wsService.sendInput('\x15', session: widget.sessionName);
          event.preventDefault();
          return false.toJS;
        }
        // Cmd+Delete → Ctrl+K (kill to end of line)
        if (event.key == 'Delete') {
          final wsService = ref.read(wsServiceProvider.notifier);
          wsService.sendInput('\x0b', session: widget.sessionName);
          event.preventDefault();
          return false.toJS;
        }
      }

      return true.toJS;
    }).toJS);

    // Wire up data handler -> send input to server
    _terminal!.onData(((JSString data) {
      final wsService = ref.read(wsServiceProvider.notifier);
      wsService.sendInput(data.toDart, session: widget.sessionName);
    }).toJS);

    // Wire up resize handler -> notify server
    _terminal!.onResize(((JSObject event) {
      final cols = (event['cols'] as JSNumber).toDartInt;
      final rows = (event['rows'] as JSNumber).toDartInt;
      final wsService = ref.read(wsServiceProvider.notifier);
      wsService.resizeSession(widget.sessionName, cols, rows);
    }).toJS);

    // Observe container size changes for fit
    _resizeObserver = web.ResizeObserver(
        ((JSArray<web.ResizeObserverEntry> entries,
            web.ResizeObserver observer) {
      _debouncedFit();
    }).toJS);
    _resizeObserver!.observe(container);

    // Initial fit
    Future.delayed(const Duration(milliseconds: 50), () {
      _fitAddon?.fit();
    });

    // Register this terminal with the facet registry
    TerminalRegistry.instance.register(widget.facetId, this);
  }

  void _debouncedFit() {
    _fitDebounce?.cancel();
    _fitDebounce = Timer(const Duration(milliseconds: 50), () {
      _fitAddon?.fit();
    });
  }

  /// Write data to this terminal (called by the facet manager on output)
  @override
  void writeData(String data) {
    _terminal?.write(data.toJS);
  }

  /// Toggle the search bar overlay.
  @override
  void toggleSearch() {
    setState(() => _showSearch = !_showSearch);
  }

  /// Apply a CSS transform to the xterm container for GPU-accelerated animation.
  /// When [animate] is true, a CSS transition smoothly interpolates the transform.
  @override
  void setGenieTransform(String transform, {bool animate = true}) {
    if (_container == null) return;
    _container!.style.transformOrigin = '0 0';
    _container!.style.transition = animate
        ? 'transform 400ms cubic-bezier(0.4, 0, 0.2, 1)'
        : 'none';
    _container!.style.transform = transform;
    _container!.style.pointerEvents = 'none';
    _setAncestorOverflow(true);
  }

  /// Clear CSS transform (restore full-size rendering).
  @override
  void clearGenieTransform({bool animate = true}) {
    if (_container == null) return;
    _container!.style.transition = animate
        ? 'transform 400ms cubic-bezier(0.4, 0, 0.2, 1)'
        : 'none';
    _container!.style.transform = '';
    _container!.style.transformOrigin = '';
    _container!.style.pointerEvents = '';
    _setAncestorOverflow(false);
  }

  /// Allow (or restore) CSS overflow on ancestor DOM elements so that
  /// CSS-transformed content can render outside the platform view bounds.
  /// Max DOM ancestor depth to walk when toggling overflow.
  /// Must be deep enough to escape Flutter's platform view wrappers.
  static const _ancestorOverflowDepth = 8;

  void _setAncestorOverflow(bool allowOverflow) {
    web.Element? el = _container?.parentElement;
    for (var i = 0; i < _ancestorOverflowDepth && el != null; i++) {
      if (el is web.HTMLElement) {
        el.style.overflow = allowOverflow ? 'visible' : '';
      }
      el = el.parentElement;
    }
  }

  @override
  void didUpdateWidget(TerminalFacet oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.isFocused && !oldWidget.isFocused) {
      _terminal?.focus();
    }
  }

  @override
  void dispose() {
    TerminalRegistry.instance.unregister(widget.facetId);
    _fitDebounce?.cancel();
    _resizeObserver?.disconnect();
    _terminal?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: () => _terminal?.focus(),
      child: Column(
        children: [
          // Title bar (hidden when single facet)
          if (widget.showTitleBar)
            _TitleBar(
              sessionName: widget.sessionName,
              isFocused: widget.isFocused,
              onClose: widget.onClose,
            ),
          // Search bar overlay
          if (_showSearch && _searchAddon != null)
            TerminalSearchBar(
              searchAddon: _searchAddon!,
              onClose: () => setState(() => _showSearch = false),
            ),
          // Terminal content
          Expanded(
            child: HtmlElementView(viewType: _viewId),
          ),
        ],
      ),
    );
  }
}

class _TitleBar extends StatelessWidget {
  final String sessionName;
  final bool isFocused;
  final VoidCallback? onClose;

  const _TitleBar({
    required this.sessionName,
    required this.isFocused,
    this.onClose,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    final bg = isFocused ? p.surface0 : p.mantle;
    final textColor = p.subtext0;

    return Container(
      height: AbotSizes.titleBarHeight,
      decoration: BoxDecoration(color: bg),
      padding: const EdgeInsets.symmetric(horizontal: AbotSpacing.sm),
      child: Row(
        children: [
          Icon(Icons.terminal, size: 14, color: textColor),
          const SizedBox(width: AbotSpacing.xs),
          Expanded(
            child: Text(
              sessionName,
              style: TextStyle(
                fontSize: 12,
                color: textColor,
                fontFamily: AbotFonts.mono,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          if (onClose != null)
            InkWell(
              onTap: onClose,
              borderRadius: BorderRadius.circular(AbotRadius.sm),
              child: Padding(
                padding: const EdgeInsets.all(4),
                child: Icon(Icons.close, size: 14, color: textColor),
              ),
            ),
        ],
      ),
    );
  }
}

/// A terminal that can receive data and report its session name.
abstract interface class TerminalSink {
  String get sessionName;
  void writeData(String data);
  void toggleSearch();
  void setGenieTransform(String transform, {bool animate});
  void clearGenieTransform({bool animate});
}

/// Global registry so the WS message handler can route output to the right terminal
class TerminalRegistry {
  TerminalRegistry._();
  static final instance = TerminalRegistry._();

  final Map<String, TerminalSink> _terminals = {};

  /// Called when a new terminal finishes initializing and registers itself.
  VoidCallback? onRegistered;

  void register(String facetId, TerminalSink sink) {
    _terminals[facetId] = sink;
    onRegistered?.call();
  }

  void unregister(String facetId) {
    _terminals.remove(facetId);
  }

  /// Write data to a terminal by its session name
  void writeToSession(String sessionName, String data) {
    for (final sink in _terminals.values) {
      if (sink.sessionName == sessionName) {
        sink.writeData(data);
        return;
      }
    }
  }

  /// Write data to all terminals (fallback)
  void writeToAll(String data) {
    for (final sink in _terminals.values) {
      sink.writeData(data);
    }
  }

  /// Toggle search on a specific facet
  void toggleSearchOnFacet(String facetId) {
    _terminals[facetId]?.toggleSearch();
  }

  /// Apply a CSS transform to a facet's terminal container (GPU-accelerated).
  void setGenieTransform(String facetId, String transform,
      {bool animate = true}) {
    _terminals[facetId]?.setGenieTransform(transform, animate: animate);
  }

  /// Clear CSS transform on a facet's terminal container.
  void clearGenieTransform(String facetId, {bool animate = true}) {
    _terminals[facetId]?.clearGenieTransform(animate: animate);
  }
}
