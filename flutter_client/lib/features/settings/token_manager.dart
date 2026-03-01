import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../../core/network/api_client.dart';
import '../../core/theme/abot_theme.dart';

/// A credential linked to a setup token.
class DeviceCredential {
  final String id;
  final String? name;
  final int? createdAt;
  final int? lastUsedAt;
  final String? userAgent;

  const DeviceCredential({
    required this.id,
    this.name,
    this.createdAt,
    this.lastUsedAt,
    this.userAgent,
  });

  factory DeviceCredential.fromJson(Map<String, dynamic> json) =>
      DeviceCredential(
        id: json['id'] as String? ?? '',
        name: json['name'] as String?,
        createdAt: json['createdAt'] as int?,
        lastUsedAt: json['lastUsedAt'] as int?,
        userAgent: json['userAgent'] as String?,
      );
}

/// A setup token with optional linked credential.
class PairedDevice {
  final String id;
  final String name;
  final int createdAt;
  final int expiresAt;
  final DeviceCredential? credential;

  const PairedDevice({
    required this.id,
    required this.name,
    required this.createdAt,
    required this.expiresAt,
    this.credential,
  });

  factory PairedDevice.fromJson(Map<String, dynamic> json) => PairedDevice(
        id: json['id'] as String? ?? '',
        name: json['name'] as String? ?? '',
        createdAt: json['createdAt'] as int? ?? 0,
        expiresAt: json['expiresAt'] as int? ?? 0,
        credential: json['credential'] != null
            ? DeviceCredential.fromJson(
                json['credential'] as Map<String, dynamic>)
            : null,
      );

  bool get hasCredential => credential != null;
}

/// Token management widget with paired-device flow.
class TokenManager extends StatefulWidget {
  const TokenManager({super.key});

  @override
  State<TokenManager> createState() => _TokenManagerState();
}

class _TokenManagerState extends State<TokenManager> {
  final _api = const ApiClient();
  List<PairedDevice> _devices = [];
  List<DeviceCredential> _orphanedCredentials = [];
  String? _newTokenValue;
  bool _loading = true;
  bool _creating = false;
  final _nameController = TextEditingController();

  @override
  void initState() {
    super.initState();
    _loadTokens();
  }

  @override
  void dispose() {
    _nameController.dispose();
    super.dispose();
  }

  Future<void> _loadTokens() async {
    setState(() => _loading = true);
    try {
      final data =
          await _api.get('/auth/tokens') as Map<String, dynamic>;
      if (!mounted) return;
      final tokensList = (data['tokens'] as List?) ?? [];
      final orphanedList = (data['orphanedCredentials'] as List?) ?? [];
      setState(() {
        _devices = tokensList
            .map((e) => PairedDevice.fromJson(e as Map<String, dynamic>))
            .toList();
        _orphanedCredentials = orphanedList
            .map((e) => DeviceCredential.fromJson(e as Map<String, dynamic>))
            .toList();
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _loading = false);
    }
  }

  Future<void> _createToken() async {
    final name = _nameController.text.trim();
    if (name.isEmpty) return;

    setState(() => _creating = true);
    try {
      final data = await _api.post('/auth/tokens', {'name': name})
          as Map<String, dynamic>;
      if (!mounted) return;
      setState(() {
        _newTokenValue = data['token'] as String? ?? '';
        _creating = false;
      });
      _nameController.clear();
      await _loadTokens();
    } catch (e) {
      if (!mounted) return;
      setState(() => _creating = false);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Failed to create token: $e')),
      );
    }
  }

  Future<void> _revokeDevice(PairedDevice device) async {
    if (device.hasCredential) {
      final confirm = await showDialog<bool>(
        context: context,
        builder: (ctx) => AlertDialog(
          backgroundColor: context.palette.base,
          title: Text(
            'Revoke device?',
            style: TextStyle(
              fontSize: 14,
              fontFamily: AbotFonts.mono,
              color: context.palette.text,
            ),
          ),
          content: Text(
            'This will disconnect the device immediately and delete its credential.',
            style: TextStyle(
              fontSize: 12,
              fontFamily: AbotFonts.mono,
              color: context.palette.subtext0,
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(
                'Cancel',
                style: TextStyle(
                  fontFamily: AbotFonts.mono,
                  color: context.palette.subtext0,
                ),
              ),
            ),
            TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(
                'Revoke',
                style: TextStyle(
                  fontFamily: AbotFonts.mono,
                  color: context.palette.red,
                ),
              ),
            ),
          ],
        ),
      );
      if (confirm != true) return;
    }

    try {
      await _api.delete('/auth/tokens/${device.id}');
      if (!mounted) return;
      await _loadTokens();
    } catch (e) {
      if (!mounted) return;
      final msg = e is ApiException && e.statusCode == 403
          ? 'Cannot revoke the last credential'
          : 'Failed to revoke: $e';
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(msg)),
      );
    }
  }

  Future<void> _deleteOrphanedCredential(DeviceCredential cred) async {
    try {
      await _api.delete('/api/credentials/${cred.id}');
      if (!mounted) return;
      await _loadTokens();
    } catch (e) {
      if (!mounted) return;
      final msg = e is ApiException && e.statusCode == 403
          ? 'Cannot delete the last credential'
          : 'Failed to delete: $e';
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(msg)),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // New token display
        if (_newTokenValue != null && _newTokenValue!.isNotEmpty) ...[
          Container(
            padding: const EdgeInsets.all(AbotSpacing.md),
            decoration: BoxDecoration(
              color: p.surface0,
              borderRadius: BorderRadius.circular(AbotRadius.md),
              border: Border.all(color: p.green, width: 0.5),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Save this token now — you won\'t see it again!',
                  style: TextStyle(
                    fontSize: 11,
                    color: p.yellow,
                    fontFamily: AbotFonts.mono,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(height: AbotSpacing.sm),
                Row(
                  children: [
                    Expanded(
                      child: SelectableText(
                        _newTokenValue!,
                        style: TextStyle(
                          fontSize: 11,
                          color: p.text,
                          fontFamily: AbotFonts.mono,
                        ),
                      ),
                    ),
                    IconButton(
                      icon: Icon(Icons.copy, size: 16, color: p.subtext0),
                      onPressed: () {
                        Clipboard.setData(
                            ClipboardData(text: _newTokenValue!));
                        ScaffoldMessenger.of(context).showSnackBar(
                          const SnackBar(
                            content: Text('Token copied'),
                            duration: Duration(seconds: 1),
                          ),
                        );
                      },
                      splashRadius: 14,
                    ),
                  ],
                ),
                const SizedBox(height: AbotSpacing.xs),
                Align(
                  alignment: Alignment.centerRight,
                  child: TextButton(
                    onPressed: () =>
                        setState(() => _newTokenValue = null),
                    child: Text(
                      'Dismiss',
                      style: TextStyle(
                        fontSize: 11,
                        color: p.subtext0,
                        fontFamily: AbotFonts.mono,
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: AbotSpacing.md),
        ],

        // Create form
        Row(
          children: [
            Expanded(
              child: SizedBox(
                height: 32,
                child: TextField(
                  controller: _nameController,
                  style: TextStyle(
                    fontSize: 12,
                    color: p.text,
                    fontFamily: AbotFonts.mono,
                  ),
                  decoration: InputDecoration(
                    hintText: 'Device name...',
                    hintStyle: TextStyle(
                      fontSize: 12,
                      color: p.overlay0,
                      fontFamily: AbotFonts.mono,
                    ),
                    contentPadding: const EdgeInsets.symmetric(
                      horizontal: AbotSpacing.sm,
                    ),
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
                    filled: true,
                    fillColor: p.surface0,
                  ),
                  onSubmitted: (_) => _createToken(),
                ),
              ),
            ),
            const SizedBox(width: AbotSpacing.sm),
            SizedBox(
              height: 32,
              child: TextButton(
                onPressed: _creating ? null : _createToken,
                style: TextButton.styleFrom(
                  backgroundColor: p.mauve,
                  foregroundColor: p.base,
                  padding: const EdgeInsets.symmetric(
                    horizontal: AbotSpacing.md,
                  ),
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(AbotRadius.sm),
                  ),
                  textStyle: const TextStyle(
                    fontSize: 11,
                    fontFamily: AbotFonts.mono,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                child: _creating
                    ? SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: p.base,
                        ),
                      )
                    : const Text('Generate'),
              ),
            ),
          ],
        ),
        const SizedBox(height: AbotSpacing.md),

        // Device list
        if (_loading)
          Center(
            child: Padding(
              padding: const EdgeInsets.all(AbotSpacing.lg),
              child: SizedBox(
                width: 18,
                height: 18,
                child: CircularProgressIndicator(
                  strokeWidth: 2,
                  color: p.overlay0,
                ),
              ),
            ),
          )
        else if (_devices.isEmpty && _orphanedCredentials.isEmpty)
          Padding(
            padding: const EdgeInsets.all(AbotSpacing.md),
            child: Text(
              'No paired devices',
              style: TextStyle(
                fontSize: 11,
                color: p.overlay0,
                fontFamily: AbotFonts.mono,
              ),
            ),
          )
        else ...[
          for (final device in _devices)
            _DeviceTile(
              device: device,
              onRevoke: () => _revokeDevice(device),
            ),
          // Orphaned credentials
          if (_orphanedCredentials.isNotEmpty) ...[
            const SizedBox(height: AbotSpacing.md),
            Text(
              'Other devices',
              style: TextStyle(
                fontSize: 10,
                color: p.subtext0,
                fontFamily: AbotFonts.mono,
                fontWeight: FontWeight.w600,
                letterSpacing: 0.5,
              ),
            ),
            const SizedBox(height: AbotSpacing.xs),
            for (final cred in _orphanedCredentials)
              _OrphanedCredentialTile(
                credential: cred,
                onDelete: () => _deleteOrphanedCredential(cred),
              ),
          ],
        ],
      ],
    );
  }
}

class _DeviceTile extends StatelessWidget {
  final PairedDevice device;
  final VoidCallback onRevoke;

  const _DeviceTile({required this.device, required this.onRevoke});

  String _formatTime(int? timestamp) {
    if (timestamp == null || timestamp == 0) return '';
    final dt = DateTime.fromMillisecondsSinceEpoch(timestamp * 1000);
    final now = DateTime.now();
    final diff = now.difference(dt);
    if (diff.inMinutes < 1) return 'just now';
    if (diff.inHours < 1) return '${diff.inMinutes}m ago';
    if (diff.inDays < 1) return '${diff.inHours}h ago';
    if (diff.inDays < 30) return '${diff.inDays}d ago';
    return '${dt.month}/${dt.day}/${dt.year}';
  }

  String _formatExpiry(int expiresAt) {
    final dt = DateTime.fromMillisecondsSinceEpoch(expiresAt * 1000);
    final now = DateTime.now();
    if (dt.isBefore(now)) return 'Expired';
    final diff = dt.difference(now);
    if (diff.inHours < 1) return 'Expires in ${diff.inMinutes}m';
    if (diff.inDays < 1) return 'Expires in ${diff.inHours}h';
    return 'Expires in ${diff.inDays}d';
  }

  @override
  Widget build(BuildContext context) {
    final p = context.palette;
    final cred = device.credential;

    return Container(
      margin: const EdgeInsets.only(bottom: AbotSpacing.xs),
      padding: const EdgeInsets.symmetric(
        horizontal: AbotSpacing.md,
        vertical: AbotSpacing.sm,
      ),
      decoration: BoxDecoration(
        color: p.surface0,
        borderRadius: BorderRadius.circular(AbotRadius.sm),
      ),
      child: Row(
        children: [
          Icon(
            device.hasCredential ? Icons.phone_android : Icons.vpn_key,
            size: 14,
            color: device.hasCredential ? p.green : p.subtext0,
          ),
          const SizedBox(width: AbotSpacing.sm),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  cred?.name ?? device.name,
                  style: TextStyle(
                    fontSize: 11,
                    color: p.text,
                    fontFamily: AbotFonts.mono,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
                if (device.hasCredential) ...[
                  Text(
                    'Active device',
                    style: TextStyle(
                      fontSize: 9,
                      color: p.green,
                      fontFamily: AbotFonts.mono,
                    ),
                  ),
                  if (cred?.lastUsedAt != null)
                    Text(
                      'Last used ${_formatTime(cred!.lastUsedAt)}',
                      style: TextStyle(
                        fontSize: 9,
                        color: p.overlay0,
                        fontFamily: AbotFonts.mono,
                      ),
                    ),
                ] else ...[
                  Text(
                    'Unused · ${_formatExpiry(device.expiresAt)}',
                    style: TextStyle(
                      fontSize: 9,
                      color: p.overlay0,
                      fontFamily: AbotFonts.mono,
                    ),
                  ),
                ],
              ],
            ),
          ),
          IconButton(
            icon: Icon(Icons.delete_outline, size: 14, color: p.subtext0),
            onPressed: onRevoke,
            splashRadius: 14,
            constraints: const BoxConstraints(
              minWidth: 28,
              minHeight: 28,
            ),
          ),
        ],
      ),
    );
  }
}

class _OrphanedCredentialTile extends StatelessWidget {
  final DeviceCredential credential;
  final VoidCallback onDelete;

  const _OrphanedCredentialTile({
    required this.credential,
    required this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    final p = context.palette;

    return Container(
      margin: const EdgeInsets.only(bottom: AbotSpacing.xs),
      padding: const EdgeInsets.symmetric(
        horizontal: AbotSpacing.md,
        vertical: AbotSpacing.sm,
      ),
      decoration: BoxDecoration(
        color: p.surface0,
        borderRadius: BorderRadius.circular(AbotRadius.sm),
      ),
      child: Row(
        children: [
          Icon(Icons.devices, size: 14, color: p.subtext0),
          const SizedBox(width: AbotSpacing.sm),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  credential.name ?? 'Unknown device',
                  style: TextStyle(
                    fontSize: 11,
                    color: p.text,
                    fontFamily: AbotFonts.mono,
                  ),
                  overflow: TextOverflow.ellipsis,
                ),
                if (credential.userAgent != null)
                  Text(
                    credential.userAgent!,
                    style: TextStyle(
                      fontSize: 9,
                      color: p.overlay0,
                      fontFamily: AbotFonts.mono,
                    ),
                    overflow: TextOverflow.ellipsis,
                    maxLines: 1,
                  ),
              ],
            ),
          ),
          IconButton(
            icon: Icon(Icons.delete_outline, size: 14, color: p.subtext0),
            onPressed: onDelete,
            splashRadius: 14,
            constraints: const BoxConstraints(
              minWidth: 28,
              minHeight: 28,
            ),
          ),
        ],
      ),
    );
  }
}
