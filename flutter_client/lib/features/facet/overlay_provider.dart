import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Overlay state — which settings/detail panels are open.
/// At most one overlay is shown at a time (opening one closes the others).
class OverlayState {
  final bool showSettings;
  final String? sessionSettings;
  final String? kuboSettings;
  final String? abotDetail;

  const OverlayState({
    this.showSettings = false,
    this.sessionSettings,
    this.kuboSettings,
    this.abotDetail,
  });

  bool get hasOverlay =>
      showSettings ||
      sessionSettings != null ||
      kuboSettings != null ||
      abotDetail != null;
}

final overlayProvider =
    NotifierProvider<OverlayNotifier, OverlayState>(OverlayNotifier.new);

class OverlayNotifier extends Notifier<OverlayState> {
  @override
  OverlayState build() => const OverlayState();

  void toggleSettings() {
    state = state.showSettings
        ? const OverlayState()
        : const OverlayState(showSettings: true);
  }

  void showSessionSettings(String name) {
    state = OverlayState(sessionSettings: name);
  }

  void showKuboSettings(String name) {
    state = OverlayState(kuboSettings: name);
  }

  void showAbotDetail(String name) {
    state = OverlayState(abotDetail: name);
  }

  void renameSession(String newName) {
    if (state.sessionSettings != null) {
      state = OverlayState(sessionSettings: newName);
    }
  }

  void closeAll() {
    state = const OverlayState();
  }
}
