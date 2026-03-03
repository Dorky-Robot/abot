import 'package:flutter/material.dart';
import '../../core/network/api_client.dart';
import '../../core/theme/abot_theme.dart';

enum FileBrowserMode { open, save, selectDirectory }

/// Entry returned from GET /api/browse.
class _BrowseEntry {
  final String name;
  final bool isDir;
  final int size;
  final int modified;

  const _BrowseEntry({
    required this.name,
    required this.isDir,
    required this.size,
    required this.modified,
  });

  factory _BrowseEntry.fromJson(Map<String, dynamic> json) => _BrowseEntry(
        name: json['name'] as String,
        isDir: json['isDir'] as bool? ?? false,
        size: (json['size'] as num?)?.toInt() ?? 0,
        modified: (json['modified'] as num?)?.toInt() ?? 0,
      );

  bool get isAbot => isDir && name.endsWith('.abot');
}

/// Server-side file browser dialog for opening / saving .abot bundles.
///
/// Usage:
/// ```dart
/// final path = await FileBrowserDialog.show(context, mode: FileBrowserMode.open);
/// ```
class FileBrowserDialog extends StatefulWidget {
  final FileBrowserMode mode;
  final String? initialPath;
  final String? defaultFileName;

  const FileBrowserDialog({
    super.key,
    required this.mode,
    this.initialPath,
    this.defaultFileName,
  });

  /// Convenience launcher — returns the selected path or null.
  static Future<String?> show(
    BuildContext context, {
    required FileBrowserMode mode,
    String? initialPath,
    String? defaultFileName,
  }) {
    return showDialog<String>(
      context: context,
      builder: (_) => FileBrowserDialog(
        mode: mode,
        initialPath: initialPath,
        defaultFileName: defaultFileName,
      ),
    );
  }

  @override
  State<FileBrowserDialog> createState() => _FileBrowserDialogState();
}

class _FileBrowserDialogState extends State<FileBrowserDialog> {
  final _api = const ApiClient();
  final _fileNameController = TextEditingController();

  String _currentPath = '';
  String? _parentPath;
  List<_BrowseEntry> _entries = [];
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    if (widget.defaultFileName != null) {
      _fileNameController.text = widget.defaultFileName!;
    }
    _browse(widget.initialPath);
  }

  @override
  void dispose() {
    _fileNameController.dispose();
    super.dispose();
  }

  Future<void> _browse([String? path]) async {
    setState(() {
      _loading = true;
      _error = null;
    });

    try {
      final queryPath = path != null ? '?path=${Uri.encodeQueryComponent(path)}' : '';
      final data = await _api.get('/api/browse$queryPath') as Map<String, dynamic>;
      if (!mounted) return;

      final entries = (data['entries'] as List? ?? [])
          .map((e) => _BrowseEntry.fromJson(e as Map<String, dynamic>))
          .toList();

      setState(() {
        _currentPath = data['path'] as String? ?? '';
        _parentPath = data['parent'] as String?;
        _entries = entries;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.toString();
        _loading = false;
      });
    }
  }

  void _onEntryTap(_BrowseEntry entry) {
    if (entry.isAbot && widget.mode == FileBrowserMode.open) {
      // In open mode, selecting a .abot bundle returns its path immediately
      Navigator.pop(context, '$_currentPath/${entry.name}');
      return;
    }
    if (entry.isDir) {
      _browse('$_currentPath/${entry.name}');
    }
  }

  void _goUp() {
    if (_parentPath != null) {
      _browse(_parentPath);
    }
  }

  void _onSave() {
    var name = _fileNameController.text.trim();
    if (name.isEmpty) return;
    if (!name.endsWith('.abot')) {
      name = '$name.abot';
    }
    Navigator.pop(context, '$_currentPath/$name');
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    final screenSize = MediaQuery.of(context).size;
    final dialogWidth = (screenSize.width - 32).clamp(0.0, 420.0);
    final dialogHeight = (screenSize.height - 64).clamp(0.0, 520.0);

    return Dialog(
      backgroundColor: p.base,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AbotRadius.lg),
        side: BorderSide(color: p.surface1, width: 0.5),
      ),
      child: SizedBox(
        width: dialogWidth,
        height: dialogHeight,
        child: Column(
          children: [
            // Header: path + close
            _buildHeader(p),
            Divider(color: p.surface1, height: 1),
            // Directory listing
            Expanded(child: _buildListing(p)),
            // Footer: save field or select button
            if (widget.mode == FileBrowserMode.save) ...[
              Divider(color: p.surface1, height: 1),
              _buildSaveFooter(p),
            ],
            if (widget.mode == FileBrowserMode.selectDirectory) ...[
              Divider(color: p.surface1, height: 1),
              _buildSelectDirFooter(p),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildHeader(CatPalette p) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(
        AbotSpacing.lg, AbotSpacing.md, AbotSpacing.sm, AbotSpacing.md,
      ),
      child: Row(
        children: [
          Expanded(
            child: Text(
              _currentPath,
              style: TextStyle(
                fontSize: 11,
                color: p.subtext0,
                fontFamily: AbotFonts.mono,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          IconButton(
            icon: Icon(Icons.close, size: 18, color: p.subtext0),
            onPressed: () => Navigator.pop(context),
            splashRadius: 16,
          ),
        ],
      ),
    );
  }

  Widget _buildListing(CatPalette p) {
    if (_loading) {
      return Center(
        child: SizedBox(
          width: 20,
          height: 20,
          child: CircularProgressIndicator(strokeWidth: 2, color: p.overlay0),
        ),
      );
    }

    if (_error != null) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(AbotSpacing.lg),
          child: Text(
            _error!,
            style: TextStyle(
              fontSize: 11,
              color: p.red,
              fontFamily: AbotFonts.mono,
            ),
            textAlign: TextAlign.center,
          ),
        ),
      );
    }

    return ListView.builder(
      padding: const EdgeInsets.symmetric(vertical: AbotSpacing.xs),
      itemCount: _entries.length + (_parentPath != null ? 1 : 0),
      itemBuilder: (context, index) {
        // "Go up" row
        if (_parentPath != null && index == 0) {
          return _EntryRow(
            icon: Icons.arrow_upward,
            iconColor: p.subtext0,
            name: '..',
            nameColor: p.subtext0,
            onTap: _goUp,
          );
        }

        final entry = _entries[_parentPath != null ? index - 1 : index];
        final Color iconColor;
        final IconData icon;
        final Color nameColor;
        final bool tappable;

        if (entry.isAbot) {
          icon = Icons.folder;
          iconColor = p.mauve;
          nameColor = p.mauve;
          // In selectDirectory mode, .abot bundles are not navigable
          tappable = widget.mode != FileBrowserMode.selectDirectory;
        } else if (entry.isDir) {
          // Regular directories — blue, tappable to navigate
          icon = Icons.folder_outlined;
          iconColor = p.blue;
          nameColor = p.text;
          tappable = true;
        } else {
          // Files — dimmed, not interactive
          icon = Icons.insert_drive_file_outlined;
          iconColor = p.overlay0;
          nameColor = p.overlay0;
          tappable = false;
        }

        return _EntryRow(
          icon: icon,
          iconColor: iconColor,
          name: entry.name,
          nameColor: nameColor,
          onTap: tappable ? () => _onEntryTap(entry) : null,
        );
      },
    );
  }

  Widget _buildSaveFooter(CatPalette p) {
    return Padding(
      padding: const EdgeInsets.all(AbotSpacing.md),
      child: Row(
        children: [
          Expanded(
            child: SizedBox(
              height: 36,
              child: TextField(
                controller: _fileNameController,
                style: TextStyle(
                  fontSize: 12,
                  color: p.text,
                  fontFamily: AbotFonts.mono,
                ),
                decoration: InputDecoration(
                  hintText: 'filename.abot',
                  hintStyle: TextStyle(
                    fontSize: 12,
                    color: p.overlay0,
                    fontFamily: AbotFonts.mono,
                  ),
                  contentPadding: const EdgeInsets.symmetric(
                    horizontal: AbotSpacing.md,
                    vertical: AbotSpacing.sm,
                  ),
                  filled: true,
                  fillColor: p.surface0,
                  border: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(AbotRadius.sm),
                    borderSide: BorderSide(color: p.surface1),
                  ),
                  enabledBorder: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(AbotRadius.sm),
                    borderSide: BorderSide(color: p.surface1),
                  ),
                  focusedBorder: OutlineInputBorder(
                    borderRadius: BorderRadius.circular(AbotRadius.sm),
                    borderSide: BorderSide(color: p.mauve),
                  ),
                ),
                onSubmitted: (_) => _onSave(),
              ),
            ),
          ),
          const SizedBox(width: AbotSpacing.sm),
          SizedBox(
            height: 36,
            child: TextButton(
              onPressed: _onSave,
              style: TextButton.styleFrom(
                backgroundColor: p.mauve,
                foregroundColor: p.base,
                padding: const EdgeInsets.symmetric(
                  horizontal: AbotSpacing.lg,
                ),
                shape: RoundedRectangleBorder(
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                ),
                textStyle: const TextStyle(
                  fontSize: 12,
                  fontFamily: AbotFonts.mono,
                  fontWeight: FontWeight.w600,
                ),
              ),
              child: const Text('Save'),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildSelectDirFooter(CatPalette p) {
    return Padding(
      padding: const EdgeInsets.all(AbotSpacing.md),
      child: Row(
        children: [
          Expanded(
            child: Text(
              _currentPath,
              style: TextStyle(
                fontSize: 11,
                color: p.subtext0,
                fontFamily: AbotFonts.mono,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          const SizedBox(width: AbotSpacing.sm),
          SizedBox(
            height: 36,
            child: TextButton(
              onPressed: () => Navigator.pop(context, _currentPath),
              style: TextButton.styleFrom(
                backgroundColor: p.mauve,
                foregroundColor: p.base,
                padding: const EdgeInsets.symmetric(
                  horizontal: AbotSpacing.lg,
                ),
                shape: RoundedRectangleBorder(
                  borderRadius: BorderRadius.circular(AbotRadius.sm),
                ),
                textStyle: const TextStyle(
                  fontSize: 12,
                  fontFamily: AbotFonts.mono,
                  fontWeight: FontWeight.w600,
                ),
              ),
              child: const Text('Select'),
            ),
          ),
        ],
      ),
    );
  }
}

/// A single row in the file listing.
class _EntryRow extends StatefulWidget {
  final IconData icon;
  final Color iconColor;
  final String name;
  final Color nameColor;
  final VoidCallback? onTap;

  const _EntryRow({
    required this.icon,
    required this.iconColor,
    required this.name,
    required this.nameColor,
    this.onTap,
  });

  @override
  State<_EntryRow> createState() => _EntryRowState();
}

class _EntryRowState extends State<_EntryRow> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return MouseRegion(
      onEnter: widget.onTap != null ? (_) => setState(() => _hovered = true) : null,
      onExit: widget.onTap != null ? (_) => setState(() => _hovered = false) : null,
      cursor: widget.onTap != null ? SystemMouseCursors.click : SystemMouseCursors.basic,
      child: GestureDetector(
        onTap: widget.onTap,
        behavior: HitTestBehavior.opaque,
        child: Container(
          color: _hovered ? p.surface0 : Colors.transparent,
          padding: const EdgeInsets.symmetric(
            horizontal: AbotSpacing.lg,
            vertical: AbotSpacing.sm,
          ),
          child: Row(
            children: [
              Icon(widget.icon, size: 16, color: widget.iconColor),
              const SizedBox(width: AbotSpacing.sm),
              Expanded(
                child: Text(
                  widget.name,
                  style: TextStyle(
                    fontSize: 12,
                    color: widget.nameColor,
                    fontFamily: AbotFonts.mono,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
