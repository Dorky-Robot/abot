import 'dart:convert';
import 'dart:js_interop';
import 'package:web/web.dart' as web;

/// HTTP API client with CSRF token injection.
/// Mirrors client/lib/api-client.js
class ApiClient {
  const ApiClient();

  String? _getCsrfToken() {
    final meta = web.document.querySelector('meta[name="csrf-token"]');
    return meta?.getAttribute('content');
  }

  Map<String, String> _headers({bool mutating = false}) {
    final headers = <String, String>{'Content-Type': 'application/json'};
    if (mutating) {
      final token = _getCsrfToken();
      if (token != null) headers['X-CSRF-Token'] = token;
    }
    return headers;
  }

  Future<dynamic> get(String url) async {
    final response =
        await web.window.fetch(url.toJS).toDart;
    if (!response.ok) {
      throw ApiException('GET $url failed (${response.status})');
    }
    final text = (await response.text().toDart).toDart;
    if (text.isEmpty) return null;
    return jsonDecode(text);
  }

  Future<dynamic> post(String url, [dynamic data]) async {
    return _mutatingRequest('POST', url, data);
  }

  Future<dynamic> put(String url, [dynamic data]) async {
    return _mutatingRequest('PUT', url, data);
  }

  Future<dynamic> patch(String url, [dynamic data]) async {
    return _mutatingRequest('PATCH', url, data);
  }

  Future<dynamic> delete(String url, [dynamic data]) async {
    return _mutatingRequest('DELETE', url, data);
  }

  Future<dynamic> _mutatingRequest(
      String method, String url, dynamic data) async {
    final headers = _headers(mutating: true);
    final headersJs = web.Headers();
    for (final entry in headers.entries) {
      headersJs.append(entry.key, entry.value);
    }

    final init = web.RequestInit(
      method: method,
      headers: headersJs,
    );
    if (data != null) {
      init.body = jsonEncode(data).toJS;
    }

    final response = await web.window.fetch(url.toJS, init).toDart;
    if (!response.ok) {
      String message = '$method $url failed (${response.status})';
      try {
        final body =
            jsonDecode((await response.text().toDart).toDart);
        if (body is Map && body['error'] != null) {
          message = body['error'] as String;
        }
      } catch (_) {}
      throw ApiException(message, statusCode: response.status);
    }
    final text = (await response.text().toDart).toDart;
    if (text.isEmpty) return null;
    return jsonDecode(text);
  }
}

class ApiException implements Exception {
  final String message;
  final int? statusCode;
  const ApiException(this.message, {this.statusCode});
  @override
  String toString() => message;
}
