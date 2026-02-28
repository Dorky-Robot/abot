import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/network/session_service.dart';
import '../../core/theme/abot_theme.dart';

/// Drawer for managing sessions: list, create, rename, delete.
class SessionDrawer extends ConsumerStatefulWidget {
  final void Function(String sessionName)? onSessionTap;

  const SessionDrawer({super.key, this.onSessionTap});

  @override
  ConsumerState<SessionDrawer> createState() => _SessionDrawerState();
}

class _SessionDrawerState extends ConsumerState<SessionDrawer> {
  final _newSessionController = TextEditingController();

  @override
  void initState() {
    super.initState();
    // Refresh session list when drawer opens
    ref.listenManual(sessionServiceProvider, (_, _) {});
    WidgetsBinding.instance.addPostFrameCallback((_) {
      ref.read(sessionServiceProvider.notifier).refresh();
    });
  }

  @override
  void dispose() {
    _newSessionController.dispose();
    super.dispose();
  }

  Future<void> _createSession() async {
    final name = _newSessionController.text.trim();
    if (name.isEmpty) return;
    try {
      await ref.read(sessionServiceProvider.notifier).createSession(name);
      _newSessionController.clear();
      if (mounted) {
        widget.onSessionTap?.call(name);
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to create session: $e')),
        );
      }
    }
  }

  Future<void> _deleteSession(String name) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete Session'),
        content: Text('Delete session "$name"?'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirmed == true) {
      try {
        await ref.read(sessionServiceProvider.notifier).deleteSession(name);
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text('Failed to delete session: $e')),
          );
        }
      }
    }
  }

  Future<void> _renameSession(String oldName) async {
    final controller = TextEditingController(text: oldName);
    final newName = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Rename Session'),
        content: TextField(
          controller: controller,
          autofocus: true,
          decoration: const InputDecoration(hintText: 'New name'),
          onSubmitted: (value) => Navigator.pop(context, value.trim()),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, controller.text.trim()),
            child: const Text('Rename'),
          ),
        ],
      ),
    );
    controller.dispose();
    if (newName != null && newName.isNotEmpty && newName != oldName) {
      try {
        await ref
            .read(sessionServiceProvider.notifier)
            .renameSession(oldName, newName);
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text('Failed to rename session: $e')),
          );
        }
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final sessions = ref.watch(sessionServiceProvider);
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final bgColor = isDark ? CatppuccinMocha.mantle : CatppuccinLatte.mantle;
    final textColor = isDark ? CatppuccinMocha.text : CatppuccinLatte.text;
    final subtextColor =
        isDark ? CatppuccinMocha.subtext0 : CatppuccinLatte.subtext0;
    final accentColor = isDark ? CatppuccinMocha.mauve : CatppuccinLatte.mauve;
    final surfaceColor =
        isDark ? CatppuccinMocha.surface0 : CatppuccinLatte.surface0;
    final borderColor =
        isDark ? CatppuccinMocha.surface1 : CatppuccinLatte.surface1;

    return Drawer(
      backgroundColor: bgColor,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Header
          Container(
            padding: EdgeInsets.only(
              top: MediaQuery.of(context).padding.top + AbotSpacing.lg,
              left: AbotSpacing.lg,
              right: AbotSpacing.lg,
              bottom: AbotSpacing.lg,
            ),
            decoration: BoxDecoration(
              color: surfaceColor,
              border: Border(bottom: BorderSide(color: borderColor)),
            ),
            child: Text(
              'Sessions',
              style: TextStyle(
                fontFamily: 'JetBrains Mono',
                fontSize: 16,
                fontWeight: FontWeight.bold,
                color: textColor,
              ),
            ),
          ),

          // Create session input
          Padding(
            padding: const EdgeInsets.all(AbotSpacing.md),
            child: Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: _newSessionController,
                    style: TextStyle(
                      fontFamily: 'JetBrains Mono',
                      fontSize: 13,
                      color: textColor,
                    ),
                    decoration: InputDecoration(
                      hintText: 'New session name',
                      hintStyle: TextStyle(
                        fontFamily: 'JetBrains Mono',
                        fontSize: 13,
                        color: subtextColor.withValues(alpha: 0.5),
                      ),
                      isDense: true,
                      contentPadding: const EdgeInsets.symmetric(
                        horizontal: AbotSpacing.sm,
                        vertical: AbotSpacing.sm,
                      ),
                      border: OutlineInputBorder(
                        borderRadius: BorderRadius.circular(AbotRadius.sm),
                        borderSide: BorderSide(color: borderColor),
                      ),
                      enabledBorder: OutlineInputBorder(
                        borderRadius: BorderRadius.circular(AbotRadius.sm),
                        borderSide: BorderSide(color: borderColor),
                      ),
                      focusedBorder: OutlineInputBorder(
                        borderRadius: BorderRadius.circular(AbotRadius.sm),
                        borderSide: BorderSide(color: accentColor),
                      ),
                    ),
                    onSubmitted: (_) => _createSession(),
                  ),
                ),
                const SizedBox(width: AbotSpacing.xs),
                IconButton(
                  icon: Icon(Icons.add, color: accentColor, size: 20),
                  onPressed: _createSession,
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(
                    minWidth: 32,
                    minHeight: 32,
                  ),
                ),
              ],
            ),
          ),

          // Session list
          Expanded(
            child: sessions.when(
              loading: () => Center(
                child: CircularProgressIndicator(color: subtextColor),
              ),
              error: (error, _) => Center(
                child: Text(
                  'Error: $error',
                  style: TextStyle(
                    fontFamily: 'JetBrains Mono',
                    fontSize: 12,
                    color: subtextColor,
                  ),
                ),
              ),
              data: (sessionList) {
                if (sessionList.isEmpty) {
                  return Center(
                    child: Text(
                      'No sessions',
                      style: TextStyle(
                        fontFamily: 'JetBrains Mono',
                        fontSize: 13,
                        color: subtextColor,
                      ),
                    ),
                  );
                }
                return ListView.builder(
                  itemCount: sessionList.length,
                  itemBuilder: (context, index) {
                    final session = sessionList[index];
                    return _SessionTile(
                      session: session,
                      textColor: textColor,
                      subtextColor: subtextColor,
                      accentColor: accentColor,
                      onTap: () {
                        widget.onSessionTap?.call(session.name);
                        Navigator.pop(context);
                      },
                      onDelete: () => _deleteSession(session.name),
                      onRename: () => _renameSession(session.name),
                    );
                  },
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}

class _SessionTile extends StatelessWidget {
  final SessionInfo session;
  final Color textColor;
  final Color subtextColor;
  final Color accentColor;
  final VoidCallback? onTap;
  final VoidCallback? onDelete;
  final VoidCallback? onRename;

  const _SessionTile({
    required this.session,
    required this.textColor,
    required this.subtextColor,
    required this.accentColor,
    this.onTap,
    this.onDelete,
    this.onRename,
  });

  @override
  Widget build(BuildContext context) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final greenColor = isDark ? CatppuccinMocha.green : CatppuccinLatte.green;

    return ListTile(
      leading: Icon(Icons.terminal, size: 18, color: subtextColor),
      title: Text(
        session.name,
        style: TextStyle(
          fontFamily: 'JetBrains Mono',
          fontSize: 13,
          color: textColor,
        ),
      ),
      subtitle: Text(
        session.status,
        style: TextStyle(
          fontFamily: 'JetBrains Mono',
          fontSize: 11,
          color: session.status == 'running' ? greenColor : subtextColor,
        ),
      ),
      trailing: IconButton(
        icon: Icon(Icons.delete_outline, size: 18, color: subtextColor),
        onPressed: onDelete,
      ),
      onTap: onTap,
      onLongPress: onRename,
      dense: true,
    );
  }
}
