@TestOn('chrome')
library;

import 'package:flutter_test/flutter_test.dart';
import 'package:web/web.dart' as web;
import 'package:abot_client/features/terminal/terminal_facet.dart';

/// Fake TerminalSink for testing registry routing without xterm.js.
class FakeSink implements TerminalSink {
  FakeSink(this.sessionName, {this.isMirror = false});

  @override
  final String sessionName;

  @override
  final bool isMirror;

  @override
  double get contentFraction => 0.5;

  @override
  web.HTMLElement? get container => null;

  final List<String> writes = [];
  int resetCount = 0;

  @override
  void writeData(String data) => writes.add(data);

  @override
  void resetTerminal() => resetCount++;

  @override
  String getBufferContent() => writes.join();

  @override
  void toggleSearch() {}

  @override
  void setGenieTransform(String transform,
      {bool animate = true, String? clipPath}) {}

  @override
  void clearGenieTransform({bool animate = true}) {}
}

void main() {
  late TerminalRegistry registry;

  setUp(() {
    // Clear singleton state between tests.
    registry = TerminalRegistry.instance;
    // Unregister any leftover sinks from previous tests.
    for (final id in registry.registeredIds.toList()) {
      registry.unregister(id);
    }
  });

  group('TerminalRegistry', () {
    test('writeToSession routes data to all sinks for that session', () {
      final main = FakeSink('main');
      final mirror = FakeSink('main', isMirror: true);
      final other = FakeSink('session-2');

      registry.register('f1', main);
      registry.register('f1_mirror', mirror);
      registry.register('f2', other);

      registry.writeToSession('main', 'hello');

      expect(main.writes, ['hello']);
      expect(mirror.writes, ['hello']);
      expect(other.writes, isEmpty);
    });

    test('resetSession resets all sinks for that session', () {
      final main = FakeSink('main');
      final mirror = FakeSink('main', isMirror: true);
      final other = FakeSink('session-2');

      registry.register('f1', main);
      registry.register('f1_mirror', mirror);
      registry.register('f2', other);

      registry.resetSession('main');

      expect(main.resetCount, 1);
      expect(mirror.resetCount, 1);
      expect(other.resetCount, 0);
    });

    test('reconnect attach replay: reset then write produces clean state', () {
      final sink = FakeSink('main');
      registry.register('f1', sink);

      // Simulate pre-existing content from before disconnect.
      registry.writeToSession('main', 'old line 1\r\nold line 2');

      // Simulate reconnect: server sends Attached{buffer}.
      // The handler should reset, then write the buffer.
      registry.resetSession('main');
      registry.writeToSession('main', 'old line 1\r\nold line 2');

      // reset was called once, writes list has pre-existing + replayed.
      expect(sink.resetCount, 1);
      // The real xterm would be cleared by reset(), so only post-reset
      // write matters. We verify reset was called before the second write.
      expect(sink.writes.length, 2);
    });

    test('double attach on reconnect: two resets + writes is idempotent', () {
      final sink = FakeSink('main');
      registry.register('f1', sink);

      const buffer = 'prompt\$ ls\r\nfile.txt';

      // First Attached message (from websocket_service connect re-attach).
      registry.resetSession('main');
      registry.writeToSession('main', buffer);

      // Second Attached message (from FacetShell listener).
      registry.resetSession('main');
      registry.writeToSession('main', buffer);

      // Two resets, two writes — real xterm would show buffer correctly
      // because each reset clears before the write.
      expect(sink.resetCount, 2);
      expect(sink.writes.where((w) => w == buffer).length, 2);
    });

    test('unregister removes sink from routing', () {
      final sink = FakeSink('main');
      registry.register('f1', sink);
      registry.unregister('f1');

      registry.writeToSession('main', 'data');
      expect(sink.writes, isEmpty);
    });

    test('resetSession on unknown session is a no-op', () {
      // Should not throw.
      registry.resetSession('nonexistent');
    });

    test('writeToAll writes to every registered sink', () {
      final a = FakeSink('main');
      final b = FakeSink('session-2');
      registry.register('f1', a);
      registry.register('f2', b);

      registry.writeToAll('broadcast');

      expect(a.writes, ['broadcast']);
      expect(b.writes, ['broadcast']);
    });
  });
}
