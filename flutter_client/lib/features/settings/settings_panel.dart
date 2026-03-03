import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import '../../core/auth/device_utils.dart' show isLocalhost;
import '../../core/network/api_client.dart';
import '../../core/network/config_service.dart';
import '../../core/theme/abot_theme.dart';
import '../../core/theme/theme_provider.dart';
import 'token_manager.dart';

/// Settings panel overlay — slides in from the sidebar footer gear icon.
class SettingsPanel extends ConsumerStatefulWidget {
  final VoidCallback onClose;

  const SettingsPanel({super.key, required this.onClose});

  @override
  ConsumerState<SettingsPanel> createState() => _SettingsPanelState();
}

class _SettingsPanelState extends ConsumerState<SettingsPanel> {
  int _tabIndex = 0; // 0 = General, 1 = Remote
  final _nameController = TextEditingController();
  final _nameFocus = FocusNode();
  final _bundleDirController = TextEditingController();
  bool _nameInitialized = false;

  @override
  void initState() {
    super.initState();
    _nameFocus.addListener(_onNameFocusChange);
  }

  @override
  void dispose() {
    _nameController.dispose();
    _nameFocus.removeListener(_onNameFocusChange);
    _nameFocus.dispose();
    _bundleDirController.dispose();
    super.dispose();
  }

  void _onNameFocusChange() {
    if (!_nameFocus.hasFocus) {
      _saveInstanceName();
    }
  }

  void _saveInstanceName() {
    final name = _nameController.text.trim();
    if (name.isNotEmpty) {
      ref.read(configProvider.notifier).setInstanceName(name);
    }
  }

  void _saveBundleDir() {
    final dir = _bundleDirController.text.trim();
    ref.read(configProvider.notifier).setBundleDir(dir);
  }

  Future<void> _pickBundleDir() async {
    try {
      final data =
          await const ApiClient().post('/api/pick-directory', {}) as Map<String, dynamic>;
      if (!mounted) return;
      final path = data['path'] as String?;
      if (path != null && path.isNotEmpty) {
        _bundleDirController.text = path;
        _saveBundleDir();
        setState(() {});
      }
    } catch (_) {
      // User cancelled or picker unavailable
    }
  }

  void _resetBundleDir() {
    _bundleDirController.text = '';
    ref.read(configProvider.notifier).setBundleDir('');
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    final configAsync = ref.watch(configProvider);
    final isLocal = isLocalhost();

    // Initialize controllers from config once loaded
    configAsync.whenData((config) {
      if (!_nameInitialized) {
        _nameController.text = config.instanceName;
        _bundleDirController.text = config.bundleDir;
        _nameInitialized = true;
      }
    });

    return GestureDetector(
      onTap: widget.onClose,
      child: Container(
        color: Colors.black54,
        child: Center(
          child: GestureDetector(
            onTap: () {}, // absorb taps on the panel
            child: Container(
              width: 320,
              constraints: const BoxConstraints(maxHeight: 480),
              decoration: BoxDecoration(
                color: p.base,
                borderRadius: BorderRadius.circular(AbotRadius.lg),
                border: Border.all(color: p.surface1, width: 0.5),
                boxShadow: [
                  BoxShadow(
                    color: Colors.black.withValues(alpha: 0.3),
                    blurRadius: 24,
                    offset: const Offset(4, 0),
                  ),
                ],
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  // Header
                  Padding(
                    padding: const EdgeInsets.fromLTRB(
                      AbotSpacing.lg,
                      AbotSpacing.lg,
                      AbotSpacing.sm,
                      0,
                    ),
                    child: Row(
                      children: [
                        Text(
                          'Settings',
                          style: TextStyle(
                            fontSize: 14,
                            fontWeight: FontWeight.w600,
                            color: p.text,
                            fontFamily: AbotFonts.mono,
                          ),
                        ),
                        const Spacer(),
                        IconButton(
                          icon: Icon(Icons.close, size: 18, color: p.subtext0),
                          onPressed: widget.onClose,
                          splashRadius: 16,
                        ),
                      ],
                    ),
                  ),

                  // Tabs
                  Padding(
                      padding: const EdgeInsets.symmetric(
                        horizontal: AbotSpacing.lg,
                      ),
                      child: Row(
                        children: [
                          _TabButton(
                            label: 'General',
                            isActive: _tabIndex == 0,
                            onTap: () => setState(() => _tabIndex = 0),
                          ),
                          const SizedBox(width: AbotSpacing.md),
                          _TabButton(
                            label: 'Remote',
                            isActive: _tabIndex == 1,
                            onTap: () => setState(() => _tabIndex = 1),
                          ),
                        ],
                      ),
                    ),

                  const SizedBox(height: AbotSpacing.md),
                  Divider(color: p.surface1, height: 1),

                  // Tab content
                  Expanded(
                    child: configAsync.when(
                      data: (config) {
                        if (_tabIndex == 0) {
                          return _buildGeneralTab(p);
                        }
                        return _buildRemoteTab(p);
                      },
                      loading: () => Center(
                        child: SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            color: p.overlay0,
                          ),
                        ),
                      ),
                      error: (e, _) => Center(
                        child: Text(
                          'Failed to load config',
                          style: TextStyle(
                            fontSize: 11,
                            color: p.red,
                            fontFamily: AbotFonts.mono,
                          ),
                        ),
                      ),
                    ),
                  ),

                  // Footer
                  Divider(color: p.surface1, height: 1),
                  _buildFooter(p, isLocal),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildGeneralTab(CatPalette p) {
    return ListView(
      padding: const EdgeInsets.all(AbotSpacing.lg),
      children: [
        // Instance name
        _SectionLabel(label: 'Instance Name'),
        const SizedBox(height: AbotSpacing.xs),
        _buildTextField(
          controller: _nameController,
          focusNode: _nameFocus,
          onSubmitted: (_) => _saveInstanceName(),
          palette: p,
        ),
        const SizedBox(height: AbotSpacing.lg),

        // Bundles location
        _SectionLabel(label: 'Bundles Location'),
        const SizedBox(height: AbotSpacing.xs),
        GestureDetector(
          onTap: _pickBundleDir,
          child: Container(
            height: 32,
            padding: const EdgeInsets.symmetric(horizontal: AbotSpacing.sm),
            decoration: BoxDecoration(
              color: p.surface0,
              borderRadius: BorderRadius.circular(AbotRadius.sm),
              border: Border.all(color: p.surface1),
            ),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    _bundleDirController.text.isNotEmpty
                        ? _bundleDirController.text
                        : '~/.abot/bundles',
                    style: TextStyle(
                      fontSize: 12,
                      color: _bundleDirController.text.isNotEmpty
                          ? p.text
                          : p.overlay0,
                      fontFamily: AbotFonts.mono,
                    ),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                Icon(Icons.folder_open, size: 14, color: p.overlay0),
              ],
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.only(top: AbotSpacing.xs),
          child: Text.rich(
            TextSpan(
              children: [
                TextSpan(
                  text: 'Default directory for .abot bundles.',
                  style: TextStyle(color: p.overlay0),
                ),
                if (_bundleDirController.text.isNotEmpty) ...[
                  const TextSpan(text: ' '),
                  WidgetSpan(
                    alignment: PlaceholderAlignment.baseline,
                    baseline: TextBaseline.alphabetic,
                    child: GestureDetector(
                      onTap: _resetBundleDir,
                      child: Text(
                        'Reset',
                        style: TextStyle(
                          fontSize: 10,
                          color: p.red,
                          fontFamily: AbotFonts.mono,
                        ),
                      ),
                    ),
                  ),
                ],
              ],
            ),
            style: TextStyle(
              fontSize: 10,
              fontFamily: AbotFonts.mono,
            ),
          ),
        ),
        const SizedBox(height: AbotSpacing.lg),

        // Theme toggle
        _SectionLabel(label: 'Appearance'),
        const SizedBox(height: AbotSpacing.xs),
        _ThemeToggle(),
      ],
    );
  }

  Widget _buildTextField({
    required TextEditingController controller,
    required FocusNode focusNode,
    required ValueChanged<String> onSubmitted,
    required CatPalette palette,
    String? hintText,
  }) {
    return SizedBox(
      height: 32,
      child: TextField(
        controller: controller,
        focusNode: focusNode,
        style: TextStyle(
          fontSize: 12,
          color: palette.text,
          fontFamily: AbotFonts.mono,
        ),
        decoration: InputDecoration(
          contentPadding: const EdgeInsets.symmetric(
            horizontal: AbotSpacing.sm,
          ),
          hintText: hintText,
          hintStyle: TextStyle(
            fontSize: 12,
            color: palette.overlay0,
            fontFamily: AbotFonts.mono,
          ),
          border: OutlineInputBorder(
            borderRadius: BorderRadius.circular(AbotRadius.sm),
            borderSide: BorderSide(color: palette.surface1),
          ),
          enabledBorder: OutlineInputBorder(
            borderRadius: BorderRadius.circular(AbotRadius.sm),
            borderSide: BorderSide(color: palette.surface1),
          ),
          focusedBorder: OutlineInputBorder(
            borderRadius: BorderRadius.circular(AbotRadius.sm),
            borderSide: BorderSide(color: palette.mauve),
          ),
          filled: true,
          fillColor: palette.surface0,
        ),
        onSubmitted: onSubmitted,
      ),
    );
  }

  Widget _buildRemoteTab(CatPalette p) {
    return ListView(
      padding: const EdgeInsets.all(AbotSpacing.lg),
      children: [
        _SectionLabel(label: 'Paired Devices'),
        const SizedBox(height: AbotSpacing.sm),
        const TokenManager(),
      ],
    );
  }

  Widget _buildFooter(CatPalette p, bool isLocal) {
    return Padding(
      padding: const EdgeInsets.all(AbotSpacing.lg),
      child: Row(
        children: [
          if (!isLocal)
            TextButton(
              onPressed: _logout,
              style: TextButton.styleFrom(
                foregroundColor: p.red,
                textStyle: const TextStyle(
                  fontSize: 11,
                  fontFamily: AbotFonts.mono,
                ),
              ),
              child: const Text('Logout'),
            ),
          const Spacer(),
          Text(
            'abot',
            style: TextStyle(
              fontSize: 10,
              color: p.overlay0,
              fontFamily: AbotFonts.mono,
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _logout() async {
    try {
      await const ApiClient().post('/auth/logout');
    } catch (_) {
      // Best-effort logout
    }
    if (!mounted) return;
    web.window.location.href = '/login';
  }
}

class _TabButton extends StatelessWidget {
  final String label;
  final bool isActive;
  final VoidCallback onTap;

  const _TabButton({
    required this.label,
    required this.isActive,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(
          horizontal: AbotSpacing.md,
          vertical: AbotSpacing.xs,
        ),
        decoration: BoxDecoration(
          border: Border(
            bottom: BorderSide(
              color: isActive ? p.mauve : Colors.transparent,
              width: 2,
            ),
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 12,
            color: isActive ? p.text : p.subtext0,
            fontWeight: isActive ? FontWeight.w600 : FontWeight.normal,
            fontFamily: AbotFonts.mono,
          ),
        ),
      ),
    );
  }
}

class _SectionLabel extends StatelessWidget {
  final String label;
  const _SectionLabel({required this.label});

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    return Text(
      label,
      style: TextStyle(
        fontSize: 10,
        color: p.subtext0,
        fontFamily: AbotFonts.mono,
        fontWeight: FontWeight.w600,
        letterSpacing: 0.5,
      ),
    );
  }
}

class _ThemeToggle extends ConsumerWidget {
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final p = context.palette;
    final mode = ref.watch(themeModeProvider);

    final currentPref = switch (mode) {
      ThemeMode.system => ThemePreference.auto,
      ThemeMode.light => ThemePreference.light,
      ThemeMode.dark => ThemePreference.dark,
    };

    return Container(
      height: 32,
      decoration: BoxDecoration(
        color: p.surface0,
        borderRadius: BorderRadius.circular(AbotRadius.sm),
        border: Border.all(color: p.surface1),
      ),
      child: Row(
        children: [
          for (final pref in ThemePreference.values)
            Expanded(
              child: GestureDetector(
                onTap: () =>
                    ref.read(themeModeProvider.notifier).setPreference(pref),
                child: Container(
                  alignment: Alignment.center,
                  decoration: BoxDecoration(
                    color: currentPref == pref ? p.surface1 : Colors.transparent,
                    borderRadius: BorderRadius.circular(AbotRadius.sm - 1),
                  ),
                  child: Text(
                    switch (pref) {
                      ThemePreference.auto => 'Auto',
                      ThemePreference.light => 'Light',
                      ThemePreference.dark => 'Dark',
                    },
                    style: TextStyle(
                      fontSize: 11,
                      color: currentPref == pref ? p.text : p.subtext0,
                      fontWeight:
                          currentPref == pref ? FontWeight.w600 : FontWeight.normal,
                      fontFamily: AbotFonts.mono,
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
