import 'dart:convert';
import 'dart:js_interop';
import 'dart:js_interop_unsafe';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;
import '../js_interop/webauthn_interop.dart' as webauthn;
import '../network/api_client.dart';
import 'device_utils.dart';

/// Auth state for the app.
class AuthState {
  final bool isChecking;
  final bool isAuthenticated;
  final bool isSetup;
  final String accessMethod;
  final String? error;

  const AuthState({
    this.isChecking = true,
    this.isAuthenticated = false,
    this.isSetup = false,
    this.accessMethod = 'internet',
    this.error,
  });

  AuthState copyWith({
    bool? isChecking,
    bool? isAuthenticated,
    bool? isSetup,
    String? accessMethod,
    Object? error = _sentinel,
  }) =>
      AuthState(
        isChecking: isChecking ?? this.isChecking,
        isAuthenticated: isAuthenticated ?? this.isAuthenticated,
        isSetup: isSetup ?? this.isSetup,
        accessMethod: accessMethod ?? this.accessMethod,
        error: error == _sentinel ? this.error : error as String?,
      );

  static const Object _sentinel = Object();
}

final authProvider =
    NotifierProvider<AuthNotifier, AuthState>(AuthNotifier.new);

class AuthNotifier extends Notifier<AuthState> {
  final _api = const ApiClient();

  @override
  AuthState build() => const AuthState();

  /// Set an error message.
  void setError(String message) {
    state = state.copyWith(error: message);
  }

  /// Check auth status from server.
  Future<void> checkStatus() async {
    state = state.copyWith(isChecking: true, error: null);
    try {
      final data = await _api.get('/auth/status') as Map<String, dynamic>;
      state = state.copyWith(
        isChecking: false,
        isSetup: data['setup'] as bool,
        accessMethod: data['accessMethod'] as String,
        isAuthenticated: data['authenticated'] as bool,
      );
    } catch (e) {
      state = state.copyWith(
        isChecking: false,
        error: 'Failed to check auth status: $e',
      );
    }
  }

  /// Login with an existing passkey.
  Future<void> login() async {
    state = state.copyWith(error: null);
    try {
      // Get login options from server
      final optsData =
          await _api.post('/auth/login/options') as Map<String, dynamic>;

      final optionsJson = optsData['options'];
      final challengeId = optsData['challengeId'] as String;

      // Convert options to JSObject for WebAuthn API
      final optionsJS = _dartToJs(optionsJson);

      // Call browser WebAuthn API
      final credential = await webauthn.startAuthentication(optionsJS);

      // Verify with server
      await _api.post('/auth/login/verify', {
        'credential': _jsToDart(credential),
        'challengeId': challengeId,
      });

      // Store credential ID
      final credId = (credential['id'] as JSString?)?.toDart;
      if (credId != null) {
        web.window.localStorage.setItem('abot_current_credential', credId);
      }

      // Reload to get fresh CSRF token
      web.window.location.href = '/';
    } on ApiException catch (e) {
      state = state.copyWith(error: e.message);
    } catch (e) {
      state = state.copyWith(error: _getWebAuthnErrorMessage(e));
    }
  }

  /// Register a new passkey.
  Future<void> register(String? setupToken) async {
    state = state.copyWith(error: null);
    try {
      // Get registration options from server
      final body = <String, dynamic>{};
      if (setupToken != null && setupToken.isNotEmpty) {
        body['setupToken'] = setupToken;
      }
      final optsData =
          await _api.post('/auth/register/options', body) as Map<String, dynamic>;

      final optionsJson = optsData['options'];
      final userId = optsData['userId'] as String;
      final challengeId = optsData['challengeId'] as String;

      // Convert options to JSObject for WebAuthn API
      final optionsJS = _dartToJs(optionsJson);

      // Call browser WebAuthn API
      final credential = await webauthn.startRegistration(optionsJS);

      // Gather device info
      final deviceId = getOrCreateDeviceId();
      final deviceName = generateDeviceName();

      // Verify with server
      await _api.post('/auth/register/verify', {
        'credential': _jsToDart(credential),
        'userId': userId,
        'challengeId': challengeId,
        'setupToken': setupToken ?? '',
        'deviceId': deviceId,
        'deviceName': deviceName,
        'userAgent': web.window.navigator.userAgent,
      });

      // Store credential ID
      final credId = (credential['id'] as JSString?)?.toDart;
      if (credId != null) {
        web.window.localStorage.setItem('abot_current_credential', credId);
      }

      // Reload to get fresh CSRF token
      web.window.location.href = '/';
    } on ApiException catch (e) {
      state = state.copyWith(error: e.message);
    } catch (e) {
      state = state.copyWith(error: _getWebAuthnErrorMessage(e));
    }
  }

  /// Convert a Dart object to a JSObject (for passing to JS APIs).
  JSObject _dartToJs(dynamic value) {
    final json = jsonEncode(value);
    return _jsonParseJs(json);
  }

  /// Parse a JSON string into a JSObject using JS JSON.parse.
  JSObject _jsonParseJs(String json) {
    final jsonObj = globalContext['JSON'] as JSObject;
    final parseFn = jsonObj['parse'] as JSFunction;
    return parseFn.callAsFunction(null, json.toJS) as JSObject;
  }

  /// Convert a JSObject back to a Dart object via JSON serialization.
  dynamic _jsToDart(JSObject value) {
    final jsonObj = globalContext['JSON'] as JSObject;
    final stringifyFn = jsonObj['stringify'] as JSFunction;
    final jsonStr =
        (stringifyFn.callAsFunction(null, value) as JSString).toDart;
    return jsonDecode(jsonStr);
  }

  /// Get a user-friendly error message for WebAuthn errors.
  String _getWebAuthnErrorMessage(Object error) {
    final msg = error.toString();
    if (msg.contains('NotAllowedError')) {
      return 'Authentication was cancelled or not allowed.';
    }
    if (msg.contains('SecurityError')) {
      return 'Security error — ensure you are using HTTPS or localhost.';
    }
    if (msg.contains('InvalidStateError')) {
      return 'This passkey is already registered.';
    }
    return 'Authentication error: $msg';
  }
}
