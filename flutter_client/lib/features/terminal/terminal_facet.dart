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

/// A single terminal facet backed by xterm.js via HtmlElementView
class TerminalFacet extends ConsumerStatefulWidget {
  final String facetId;
  final String sessionName;
  final bool isFocused;
  final VoidCallback? onFocused;
  final VoidCallback? onClose;
  final bool showTitleBar;

  const TerminalFacet({
    super.key,
    required this.facetId,
    required this.sessionName,
    this.isFocused = false,
    this.onFocused,
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
  bool _registered = false;
  Timer? _fitDebounce;

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
      fontFamily:
          "'JetBrains Mono', 'SF Mono', Monaco, 'Cascadia Code', 'Roboto Mono', Consolas, 'Courier New', monospace",
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

  /// Get terminal dimensions
  ({int cols, int rows})? get dimensions {
    if (_terminal == null) return null;
    return (cols: _terminal!.cols, rows: _terminal!.rows);
  }

  /// Focus the underlying xterm terminal
  void focusTerminal() {
    _terminal?.focus();
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
    final isDark = Theme.of(context).brightness == Brightness.dark;

    return GestureDetector(
      onTap: () {
        widget.onFocused?.call();
        _terminal?.focus();
      },
      child: Column(
        children: [
          // Title bar (hidden when single facet)
          if (widget.showTitleBar)
            _TitleBar(
              sessionName: widget.sessionName,
              isFocused: widget.isFocused,
              isDark: isDark,
              onClose: widget.onClose,
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
  final bool isDark;
  final VoidCallback? onClose;

  const _TitleBar({
    required this.sessionName,
    required this.isFocused,
    required this.isDark,
    this.onClose,
  });

  @override
  Widget build(BuildContext context) {
    final bg = isDark
        ? (isFocused
            ? CatppuccinMocha.surface0
            : CatppuccinMocha.mantle)
        : (isFocused
            ? CatppuccinLatte.surface0
            : CatppuccinLatte.mantle);
    final textColor = isDark
        ? CatppuccinMocha.subtext0
        : CatppuccinLatte.subtext0;

    return Container(
      height: AbotSizes.titleBarHeight,
      color: bg,
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
                fontFamily: 'JetBrains Mono',
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
}

/// Global registry so the WS message handler can route output to the right terminal
class TerminalRegistry {
  TerminalRegistry._();
  static final instance = TerminalRegistry._();

  final Map<String, TerminalSink> _terminals = {};

  void register(String facetId, TerminalSink sink) {
    _terminals[facetId] = sink;
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
}
