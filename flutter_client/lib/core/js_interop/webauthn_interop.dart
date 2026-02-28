import 'dart:js_interop';
import 'dart:js_interop_unsafe';

/// JS interop bindings for simpleWebAuthn loaded via globalThis.simpleWebAuthn
///
/// The simpleWebAuthn ESM module is loaded in web/index.html and exposed as:
///   globalThis.simpleWebAuthn = { startRegistration, startAuthentication }

JSObject get _simpleWebAuthn => globalContext['simpleWebAuthn'] as JSObject;

/// Call simpleWebAuthn.startRegistration({ optionsJSON })
Future<JSObject> startRegistration(JSObject optionsJSON) async {
  final fn = _simpleWebAuthn['startRegistration'] as JSFunction;
  final args = JSObject();
  args['optionsJSON'] = optionsJSON;
  final promise = fn.callAsFunction(null, args) as JSPromise<JSObject>;
  return promise.toDart;
}

/// Call simpleWebAuthn.startAuthentication({ optionsJSON })
Future<JSObject> startAuthentication(JSObject optionsJSON) async {
  final fn = _simpleWebAuthn['startAuthentication'] as JSFunction;
  final args = JSObject();
  args['optionsJSON'] = optionsJSON;
  final promise = fn.callAsFunction(null, args) as JSPromise<JSObject>;
  return promise.toDart;
}
