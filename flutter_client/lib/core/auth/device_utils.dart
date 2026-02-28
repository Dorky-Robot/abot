import 'dart:math';
import 'package:web/web.dart' as web;

/// Check if the current page is served from localhost.
bool isLocalhost() {
  final hostname = web.window.location.hostname;
  return hostname == 'localhost' ||
      hostname == '127.0.0.1' ||
      hostname == '::1';
}

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
  final uaLower = ua.toLowerCase();

  // Mobile devices
  if (uaLower.contains('iphone')) {
    final match = RegExp(r'iPhone OS (\d+)').firstMatch(ua);
    return match != null ? 'iPhone (iOS ${match.group(1)})' : 'iPhone';
  }
  if (uaLower.contains('ipad')) return 'iPad';
  if (uaLower.contains('android')) {
    final match = RegExp(r'Android (\d+)').firstMatch(ua);
    return match != null ? 'Android ${match.group(1)}' : 'Android';
  }

  // Desktop browsers
  final hasChrome = uaLower.contains('chrome');
  final hasFirefox = uaLower.contains('firefox');

  if (uaLower.contains('mac os x')) {
    if (hasChrome) return 'Chrome on Mac';
    if (hasFirefox) return 'Firefox on Mac';
    if (uaLower.contains('safari') && !hasChrome) return 'Safari on Mac';
    return 'Mac';
  }
  if (uaLower.contains('windows')) {
    if (hasChrome) return 'Chrome on Windows';
    if (hasFirefox) return 'Firefox on Windows';
    if (uaLower.contains('edge')) return 'Edge on Windows';
    return 'Windows';
  }
  if (uaLower.contains('linux')) {
    if (hasChrome) return 'Chrome on Linux';
    if (hasFirefox) return 'Firefox on Linux';
    return 'Linux';
  }

  return 'Unknown Device';
}

/// Generate a random UUID v4 using dart:math Random.
String _generateUuid() {
  final rng = Random();
  final bytes = List<int>.generate(16, (_) => rng.nextInt(256));
  // Set version (4) and variant (10xx) bits
  bytes[6] = (bytes[6] & 0x0F) | 0x40;
  bytes[8] = (bytes[8] & 0x3F) | 0x80;
  final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-'
      '${hex.substring(12, 16)}-${hex.substring(16, 20)}-'
      '${hex.substring(20, 32)}';
}
