import 'package:web/web.dart' as web;

/// Get or create a stable device ID persisted in localStorage.
String getOrCreateDeviceId() {
  var deviceId = web.window.localStorage.getItem('abot_device_id');
  if (deviceId == null || deviceId.isEmpty) {
    deviceId = _generateUuid();
    web.window.localStorage.setItem('abot_device_id', deviceId);
  }
  return deviceId;
}

/// Generate a human-readable device name from the user agent.
String generateDeviceName() {
  final ua = web.window.navigator.userAgent;

  // Mobile devices
  if (RegExp(r'iPhone', caseSensitive: false).hasMatch(ua)) {
    final match = RegExp(r'iPhone OS (\d+)').firstMatch(ua);
    return match != null ? 'iPhone (iOS ${match.group(1)})' : 'iPhone';
  }
  if (RegExp(r'iPad', caseSensitive: false).hasMatch(ua)) {
    return 'iPad';
  }
  if (RegExp(r'Android', caseSensitive: false).hasMatch(ua)) {
    final match = RegExp(r'Android (\d+)').firstMatch(ua);
    return match != null ? 'Android ${match.group(1)}' : 'Android';
  }

  // Desktop browsers
  if (RegExp(r'Mac OS X', caseSensitive: false).hasMatch(ua)) {
    if (RegExp(r'Chrome', caseSensitive: false).hasMatch(ua)) {
      return 'Chrome on Mac';
    }
    if (RegExp(r'Firefox', caseSensitive: false).hasMatch(ua)) {
      return 'Firefox on Mac';
    }
    if (RegExp(r'Safari', caseSensitive: false).hasMatch(ua) &&
        !RegExp(r'Chrome', caseSensitive: false).hasMatch(ua)) {
      return 'Safari on Mac';
    }
    return 'Mac';
  }
  if (RegExp(r'Windows', caseSensitive: false).hasMatch(ua)) {
    if (RegExp(r'Chrome', caseSensitive: false).hasMatch(ua)) {
      return 'Chrome on Windows';
    }
    if (RegExp(r'Firefox', caseSensitive: false).hasMatch(ua)) {
      return 'Firefox on Windows';
    }
    if (RegExp(r'Edge', caseSensitive: false).hasMatch(ua)) {
      return 'Edge on Windows';
    }
    return 'Windows';
  }
  if (RegExp(r'Linux', caseSensitive: false).hasMatch(ua)) {
    if (RegExp(r'Chrome', caseSensitive: false).hasMatch(ua)) {
      return 'Chrome on Linux';
    }
    if (RegExp(r'Firefox', caseSensitive: false).hasMatch(ua)) {
      return 'Firefox on Linux';
    }
    return 'Linux';
  }

  return 'Unknown Device';
}

/// Simple UUID v4 generator using DateTime + hashCode.
String _generateUuid() {
  final now = DateTime.now().microsecondsSinceEpoch;
  final random = now.hashCode;
  // Format as UUID-like string
  final hex = (now ^ random).toRadixString(16).padLeft(32, '0');
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-'
      '4${hex.substring(13, 16)}-'
      '${(int.parse(hex.substring(16, 17), radix: 16) & 0x3 | 0x8).toRadixString(16)}${hex.substring(17, 20)}-'
      '${hex.substring(20, 32)}';
}
