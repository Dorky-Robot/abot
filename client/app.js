    import { ModalRegistry } from "/lib/modal.js";
    import { createFacetManager } from "/lib/facet-manager.js";
    import {
      createSessionStore, invalidateSessions,
      createTokenStore, setNewToken, invalidateTokens, removeToken, loadTokens as reloadTokens,
      createShortcutsStore, loadShortcuts as reloadShortcuts,
    } from "/lib/stores.js";
    import { createSessionListComponent } from "/lib/session-list-component.js";
    import { createSessionManager } from "/lib/session-manager.js";
    import { createTokenListComponent } from "/lib/token-list-component.js";
    import { createTokenFormManager } from "/lib/token-form.js";
    import { createShortcutsPopup, createShortcutsEditPanel, createAddShortcutModal } from "/lib/shortcuts-components.js";
    import { createDictationModal } from "/lib/dictation-modal.js";
    import { createDragDropManager } from "/lib/drag-drop.js";
    import { showToast, isImageFile, uploadImageToTerminal as uploadImageToTerminalFn } from "/lib/image-upload.js";
    import { createJoystickManager } from "/lib/joystick.js";
    import { createPullToRefreshManager } from "/lib/pull-to-refresh.js";
    import { createThemeManager, DARK_THEME, LIGHT_THEME } from "/lib/theme-manager.js";
    import { createTabManager } from "/lib/tab-manager.js";
    import { isAtBottom, scrollToBottom, withPreservedScroll, terminalWriteWithScroll } from "/lib/scroll-utils.js";
    import { keysToSequence, sendSequence, displayKey, keysLabel, keysString, VALID_KEYS, normalizeKey } from "/lib/key-mapping.js";
    import { createShortcutBar } from "/lib/shortcut-bar.js";
    import { createPasteHandler } from "/lib/paste-handler.js";
    import { createNetworkMonitor } from "/lib/network-monitor.js";
    import { createP2PManager, createP2PIndicator } from "/lib/p2p-manager.js";
    import { createSettingsHandlers } from "/lib/settings-handlers.js";
    import { createTerminalKeyboard } from "/lib/terminal-keyboard.js";
    import { createInputSender } from "/lib/input-sender.js";
    import { createViewportManager } from "/lib/viewport-manager.js";
    import { createWebSocketConnection } from "/lib/websocket-connection.js";

    // --- Modal Manager ---
    const modals = new ModalRegistry();

    // Modal registration imported from /lib/modal-init.js

    // --- Theme (using composable theme manager) ---
    // onThemeChange wired after facetManager is created (see below)
    const themeManager = createThemeManager({
      onThemeChange: (themeData) => {
        // Apply to all facets
        if (facetManager) {
          facetManager.applyThemeToAll(themeData);
        }
      }
    });

    const applyTheme = themeManager.apply;

    // Forward declaration — assigned after shortcutBarInstance is created
    let renderBar = null;

    // --- Facet Manager ---
    const facetManager = createFacetManager({
      container: document.getElementById("facet-layer"),
      themeManager: { getEffective: themeManager.getEffective, DARK_THEME, LIGHT_THEME },
      onFocusChange: (facet) => {
        // Update state to reflect focused session
        state.update('session.name', facet.sessionName);
        document.title = facet.sessionName;
        renderBar?.(facet.sessionName);
      },
      onResize: (facetId, cols, rows) => {
        if (state.connection.ws?.readyState === 1) {
          const facet = facetManager.getAll().find(f => f.id === facetId);
          const session = facet ? facet.sessionName : undefined;
          state.connection.ws.send(JSON.stringify({ type: "resize", cols, rows, session }));
        }
      },
      onClose: (facetId, sessionName) => {
        // Detach from session on server
        if (state.connection.ws?.readyState === 1) {
          state.connection.ws.send(JSON.stringify({ type: "detach", session: sessionName }));
        }
      }
    });

    // --- State ---

    // --- Centralized application state (at edge) ---
    const createAppState = () => {
      const initialSessionName = new URLSearchParams(location.search).get("s") || "main";

      return {
        session: {
          name: initialSessionName,
          shortcuts: []
        },
        connection: {
          ws: null,
          attached: false,
          reconnectDelay: 1000
        },
        p2p: {
          peer: null,
          connected: false,
          retryTimer: 0,
        },
        scroll: {
          userScrolledUpBeforeDisconnect: false
        },
        // Controlled state updates
        update(path, value) {
          const keys = path.split('.');
          let obj = this;
          for (let i = 0; i < keys.length - 1; i++) {
            obj = obj[keys[i]];
          }
          obj[keys[keys.length - 1]] = value;
          return this;
        },
        // Batch updates
        updateMany(updates) {
          Object.entries(updates).forEach(([path, value]) => {
            this.update(path, value);
          });
          return this;
        }
      };
    };

    const state = createAppState();

    // --- Instance Icon ---
    let instanceIcon = "terminal-window";
    let shortcutBarInstance = null;
    const getInstanceIcon = () => instanceIcon;
    const setInstanceIcon = (icon) => {
      instanceIcon = icon.replace(/[^a-z0-9-]/g, "");
      // Re-render shortcut bar to show new icon
      if (shortcutBarInstance) {
        shortcutBarInstance.render(state.session.name);
      }
    };

    // --- Shortcuts state management (reactive store) ---
    const shortcutsStore = createShortcutsStore();
    const loadShortcuts = () => reloadShortcuts(shortcutsStore);

    // Subscribe to shortcuts changes for render side effects
    // Note: shortcuts store subscription moved after renderBar is defined (line ~640)

    // --- P2P Manager ---

    // Initialize P2P manager
    const p2pManager = createP2PManager({
      onStateChange: (p2pState) => {
        state.update('p2p.connected', p2pState.connected);
        state.update('p2p.peer', p2pState.peer);
        updateP2PIndicator();
      },
      onData: (str) => {
        try {
          const msg = JSON.parse(str);
          if (msg.type === "output") {
            // Route to correct facet if session specified
            const facet = msg.session ? facetManager.getBySession(msg.session) : facetManager.getFocused();
            if (facet) facet.term.write(msg.data);
          }
        } catch {
          // ignore malformed P2P data
        }
      },
      getWS: () => state.connection.ws
    });

    // P2P UI indicator
    const p2pIndicator = createP2PIndicator({
      p2pManager,
      getConnectionState: () => ({ attached: state.connection.attached })
    });
    const updateP2PIndicator = () => p2pIndicator.update();

    document.title = state.session.name;

    // --- Terminal setup via Facet Manager ---
    // Create default facet (fullscreen, matches legacy single-terminal behavior)
    const defaultFacet = facetManager.create(state.session.name);
    // Aliases for backward compatibility with code that references term/fit/searchAddon
    const term = defaultFacet.term;
    const fit = defaultFacet.fit;
    const searchAddon = defaultFacet.searchAddon;

    // Helper: get the currently focused facet's terminal
    const getFocusedTerm = () => {
      const f = facetManager.getFocused();
      return f ? f.term : term;
    };
    const getFocusedSearchAddon = () => {
      const f = facetManager.getFocused();
      return f ? f.searchAddon : searchAddon;
    };

    // --- Search bar ---
    const searchBar = document.getElementById("search-bar");
    const searchInput = document.getElementById("search-input");
    const searchClose = document.getElementById("search-close");

    function toggleSearchBar() {
      const visible = searchBar.classList.toggle("visible");
      if (visible) {
        searchInput.focus();
        searchInput.select();
      } else {
        searchInput.value = "";
        getFocusedSearchAddon().clearDecorations();
        getFocusedTerm().focus();
      }
    }

    searchInput.addEventListener("input", () => {
      if (searchInput.value) {
        getFocusedSearchAddon().findNext(searchInput.value);
      } else {
        getFocusedSearchAddon().clearDecorations();
      }
    });
    searchInput.addEventListener("keydown", (ev) => {
      if (ev.key === "Escape") {
        toggleSearchBar();
        ev.preventDefault();
      } else if (ev.key === "Enter") {
        if (ev.shiftKey) {
          getFocusedSearchAddon().findPrevious(searchInput.value);
        } else {
          getFocusedSearchAddon().findNext(searchInput.value);
        }
        ev.preventDefault();
      }
    });
    searchClose.addEventListener("click", toggleSearchBar);

    // Initialize modals with terminal reference
    const focusTerm = () => getFocusedTerm().focus();
    modals.register('shortcuts', 'shortcuts-overlay', {
      returnFocus: term,
      onClose: focusTerm
    });
    modals.register('edit', 'edit-overlay', {
      returnFocus: term,
      onClose: focusTerm
    });
    modals.register('add', 'add-modal', {
      returnFocus: term,
      onOpen: () => {
        const keyInput = document.getElementById("key-composer-input");
        if (keyInput) keyInput.focus();
      },
      onClose: focusTerm
    });
    modals.register('session', 'session-overlay', {
      returnFocus: term,
      onClose: focusTerm
    });
    modals.register('dictation', 'dictation-overlay', {
      returnFocus: term,
      onClose: focusTerm
    });
    modals.register('settings', 'settings-overlay', {
      returnFocus: term,
      onClose: focusTerm
    });

    document.fonts.ready.then(() => {
      // Refit all facets after fonts load
      for (const f of facetManager.getAll()) {
        withPreservedScroll(f.term, () => f.fit.fit());
        scrollToBottom(f.term);
      }
    });

    applyTheme(localStorage.getItem("theme") || "auto");
    window.matchMedia("(prefers-color-scheme: light)").addEventListener("change", () => {
      if ((localStorage.getItem("theme") || "auto") === "auto") applyTheme("auto");
    });

    // --- WebSocket ---

    // Create buffered input sender
    const inputSender = createInputSender({
      p2pManager,
      getWebSocket: () => state.connection.ws,
      getSessionName: () => {
        const focused = facetManager.getFocused();
        return focused ? focused.sessionName : state.session.name;
      }
    });

    const rawSend = (data) => inputSender.send(data);

    // Initialize terminal keyboard handlers
    const terminalKeyboard = createTerminalKeyboard({
      term,
      onSend: rawSend,
      onToggleSearch: toggleSearchBar
    });
    terminalKeyboard.init();

    // WebSocket connection setup moved to after all dependencies are initialized (see before Boot section)

    // --- Layout ---

    const termContainer = document.getElementById("terminal-container");
    const bar = document.getElementById("shortcut-bar");

    // --- Joystick (composable state machine) ---
    const joystickManager = createJoystickManager({
      onSend: (sequence) => rawSend(sequence)
    });
    joystickManager.init();



    // --- Pull-to-refresh (composable gesture handler) ---
    const pullToRefresh = createPullToRefreshManager({
      container: termContainer,
      isAtBottom,
      onRefresh: () => {
        if (state.connection.ws && state.connection.ws.readyState === WebSocket.OPEN && state.connection.attached) {
          rawSend("\x0C"); // Ctrl-L: refresh screen
        } else {
          if (state.connection.ws) state.connection.ws.close();
        }
      }
    });
    pullToRefresh.init();

    // --- Shortcuts popup (reactive component) ---

    const shortcutsPopup = createShortcutsPopup({
      onShortcutClick: (keys) => {
        sendSequence(keysToSequence(keys), rawSend);
      },
      modals
    });

    function openShortcutsPopup(items) {
      shortcutsPopup.render(document.getElementById("shortcuts-grid"), items);
      modals.open('shortcuts');
    }

    document.getElementById("shortcuts-edit-btn").addEventListener("click", () => {
      modals.close('shortcuts');
      shortcutsEditPanel.open(shortcutsStore.getState());
    });
    

    // --- Edit shortcuts (reactive component) ---

    const shortcutsEditPanel = createShortcutsEditPanel(shortcutsStore, { modals });

    // Subscribe to shortcuts changes to re-render edit list
    shortcutsStore.subscribe((shortcuts) => {
      const editList = document.getElementById("edit-list");
      if (editList && modals.get('edit')?.isOpen) {
        shortcutsEditPanel.render(editList, shortcuts);
      }
    });

    document.getElementById("edit-done").addEventListener("click", () => {
      shortcutsEditPanel.close();
    });

    document.getElementById("edit-add").addEventListener("click", () => {
      addShortcutModal.open();
    });

    // --- Add shortcut modal (reactive component) ---

    const addShortcutModal = createAddShortcutModal(shortcutsStore, {
      modals,
      keysLabel,
      keysString,
      displayKey,
      normalizeKey,
      VALID_KEYS
    });

    // Initialize the add modal event handlers
    addShortcutModal.init();

    

    // --- Session manager (render takes data) ---

    const sessionStore = createSessionStore(state.session.name);

    // Create session list component
    const sessionListComponent = createSessionListComponent(sessionStore);
    const sessionListEl = document.getElementById("session-list");
    if (sessionListEl) {
      sessionListComponent.mount(sessionListEl);
    }

    // Create session manager with callbacks
    const sessionManager = createSessionManager({
      modals,
      sessionStore,
      onSessionCreate: () => invalidateSessions(sessionStore, state.session.name)
    });
    sessionManager.init();

    // Expose openSessionManager for external use
    const openSessionManager = () => sessionManager.openSessionManager(state.session.name);
    

    // --- Settings ---

    const settingsHandlers = createSettingsHandlers({
      onThemeChange: (theme) => applyTheme(theme),
      onInstanceIconChange: setInstanceIcon,
      onToolbarColorChange: (color) => {
        const bar = document.getElementById("shortcut-bar");
        if (bar) {
          if (color && color !== "default") {
            bar.setAttribute("data-toolbar-color", color);
          } else {
            bar.removeAttribute("data-toolbar-color");
          }
        }
      }
    });
    settingsHandlers.init();

    // --- Settings tabs (using generic tab manager) ---
    const settingsTabManager = createTabManager({
      tabSelector: '.settings-tab',
      contentSelector: '.settings-tab-content',
      onTabChange: (targetTab) => {
        if (targetTab === "remote") {
          // Clear any lingering new token display before loading tokens
          const tokensList = document.getElementById("tokens-list");
          const staleNewToken = tokensList?.querySelector('.token-item-new');
          if (staleNewToken) staleNewToken.remove();
          loadTokens();
        }
      }
    });
    settingsTabManager.init();

    // --- Token management ---

    const tokenStore = createTokenStore();
    const loadTokens = () => reloadTokens(tokenStore);

    // Create token form manager with callbacks
    const tokenFormManager = createTokenFormManager({
      onCreate: (data) => {
        setNewToken(tokenStore, data);
      },
      onRename: () => {
        invalidateTokens(tokenStore);
      },
      onRevoke: (tokenId) => {
        removeToken(tokenStore, tokenId);
      }
    });
    tokenFormManager.init();

    // Create token list component
    const tokenListComponent = createTokenListComponent(tokenStore, {
      onRename: (tokenId) => tokenFormManager.renameToken(tokenId),
      onRevoke: (tokenId, hasCredential, isOrphaned) => tokenFormManager.revokeToken(tokenId, hasCredential, isOrphaned)
    });
    const tokensList = document.getElementById("tokens-list");
    if (tokensList) {
      tokenListComponent.mount(tokensList);
    }

    // --- Dictation modal (reactive component) ---

    const dictationModal = createDictationModal({
      modals,
      onSend: async (text, images) => {
        if (text) rawSend(text);
        for (const file of images) {
          await uploadImageToTerminal(file);
        }
      }
    });

    dictationModal.init();

    function openDictationModal() {
      dictationModal.open();
    }

    // --- Viewport manager & Shortcut bar ---
    // (Moved here after openSessionManager and openDictationModal are defined)

    const viewportManager = createViewportManager({
      term,
      fit,
      termContainer,
      bar,
      onWebSocketResize: (cols, rows) => {
        if (state.connection.ws?.readyState === 1) {
          const focused = facetManager.getFocused();
          const session = focused ? focused.sessionName : state.session.name;
          state.connection.ws.send(JSON.stringify({ type: "resize", cols, rows, session }));
        }
      },
      onDictationOpen: () => openDictationModal()
    });
    viewportManager.init();

    // --- Facet keyboard shortcuts ---
    document.addEventListener("keydown", (e) => {
      const isMeta = e.metaKey || e.ctrlKey;

      // Cmd+T: new facet with new session
      if (isMeta && e.key === "t" && !e.shiftKey) {
        e.preventDefault();
        const name = `session-${Date.now()}`;
        const facet = facetManager.createTiled(name);
        // Attach the new facet's session on the server
        if (state.connection.ws?.readyState === 1) {
          state.connection.ws.send(JSON.stringify({
            type: "attach", session: name,
            cols: facet.term.cols, rows: facet.term.rows
          }));
        }
      }

      // Cmd+W: close focused facet (only if multiple)
      if (isMeta && e.key === "w" && !e.shiftKey) {
        if (facetManager.count() > 1) {
          e.preventDefault();
          const focused = facetManager.getFocused();
          if (focused) facetManager.remove(focused.id);
        }
      }

      // Cmd+` or Ctrl+`: cycle facet focus
      if (isMeta && e.key === "`") {
        e.preventDefault();
        facetManager.cycleFocus();
      }
    });

    shortcutBarInstance = createShortcutBar({
      container: bar,
      pinnedKeys: [
        { label: "Esc", keys: "esc" },
        { label: "Tab", keys: "tab" }
      ],
      onSessionClick: openSessionManager,
      onShortcutsClick: () => openShortcutsPopup(state.session.shortcuts),
      onSettingsClick: () => modals.open('settings'),
      sendFn: rawSend,
      term,
      updateP2PIndicator,
      getInstanceIcon
    });

    renderBar = (name) => shortcutBarInstance.render(name);

    // Subscribe to shortcuts changes to re-render bar
    shortcutsStore.subscribe((shortcuts) => {
      // Update legacy state object (for backward compatibility)
      state.update('session.shortcuts', shortcuts);

      // Re-render bar when shortcuts change
      renderBar(state.session.name);
    });



    // --- Image upload (using imported helpers) ---
    const uploadImageToTerminal = (file) => uploadImageToTerminalFn(file, {
      onSend: rawSend,
      toast: showToast
    });

    // --- Drag-and-drop (reactive manager) ---

    const dragDropManager = createDragDropManager({
      isImageFile,
      onDrop: async (imageFiles, totalFiles) => {
        if (imageFiles.length === 0) {
          if (totalFiles > 0) showToast("Not an image file", true);
          return;
        }
        for (const file of imageFiles) {
          // Write image to system clipboard and send Ctrl+V so CLI tools
          // (like Claude Code) detect it the same way as a native paste.
          try {
            const blob = new Blob([await file.arrayBuffer()], { type: file.type });
            await navigator.clipboard.write([new ClipboardItem({ [file.type]: blob })]);
            rawSend("\x16"); // Ctrl+V triggers clipboard read in the PTY app
          } catch {
            // Fallback: upload and send absolute filesystem path
            uploadImageToTerminal(file);
          }
        }
      }
    });

    dragDropManager.init();

    // --- Global paste ---

    const pasteHandler = createPasteHandler({
      onText: (text) => rawSend(text),
      onImage: (file) => uploadImageToTerminal(file)
    });
    pasteHandler.init();

    // --- Network change monitoring ---

    const networkMonitor = createNetworkMonitor({
      onNetworkChange: () => {
        if (!state.connection.ws || state.connection.ws.readyState !== 1) return;
        p2pManager.create();
      }
    });
    networkMonitor.init();

    // --- WebSocket Connection ---

    const wsConnection = createWebSocketConnection({
      term,
      state,
      p2pManager,
      updateP2PIndicator,
      loadTokens,
      isAtBottom,
      renderBar,
      facetManager
    });
    wsConnection.initVisibilityReconnect();

    // --- Boot ---

    renderBar(state.session.name);  // Initial render
    wsConnection.connect();
    loadShortcuts();
    getFocusedTerm().focus();

    if ("serviceWorker" in navigator) {
      navigator.serviceWorker.register("/sw.js").catch(() => {});
    }
