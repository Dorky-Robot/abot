import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:web/web.dart' as web;

/// Which sidebar tab is active.
enum SidebarTab { kubos, abots }

/// Sidebar UI state — collapsed/expanded and active tab.
/// Persisted to localStorage so it survives page reloads.
class SidebarState {
  final bool collapsed;
  final SidebarTab tab;

  const SidebarState({
    this.collapsed = false,
    this.tab = SidebarTab.abots,
  });

  SidebarState copyWith({bool? collapsed, SidebarTab? tab}) => SidebarState(
        collapsed: collapsed ?? this.collapsed,
        tab: tab ?? this.tab,
      );
}

final sidebarProvider =
    NotifierProvider<SidebarNotifier, SidebarState>(SidebarNotifier.new);

class SidebarNotifier extends Notifier<SidebarState> {
  static const _tabKey = 'abot_sidebar_tab';

  @override
  SidebarState build() {
    final tabStr = web.window.localStorage.getItem(_tabKey);
    final tab = tabStr == 'kubos' ? SidebarTab.kubos : SidebarTab.abots;
    return SidebarState(tab: tab);
  }

  void toggleCollapsed() {
    state = state.copyWith(collapsed: !state.collapsed);
  }

  void setTab(SidebarTab tab) {
    state = state.copyWith(tab: tab);
    web.window.localStorage.setItem(_tabKey, tab.name);
  }
}
