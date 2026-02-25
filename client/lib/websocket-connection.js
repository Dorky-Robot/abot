/**
 * WebSocket Connection Manager — adapted for abot protocol.
 *
 * abot uses session.* prefixed messages and requires explicit
 * session.list → session.create → session.attach flow.
 *
 * Based on katulong's websocket-connection.js.
 */

import { scrollToBottom, terminalWriteWithScroll } from "/assets/lib/scroll-utils.js";

/**
 * Create WebSocket connection manager with injected dependencies
 */
export function createWebSocketConnection(deps = {}) {
  const {
    term,
    state,
    p2pManager,
    updateP2PIndicator,
    loadTokens,
    isAtBottom
  } = deps;

  let isConnecting = false;
  let reconnectTimeout = null;

  // --- Helper: send JSON over WS ---
  function wsSend(obj) {
    if (state.connection.ws && state.connection.ws.readyState === WebSocket.OPEN) {
      state.connection.ws.send(JSON.stringify(obj));
    }
  }

  // --- Pure WebSocket message handlers (functional core) ---
  // abot uses "session.*" message types
  const wsMessageHandlers = {
    'session.attached': (msg, currentState) => ({
      stateUpdates: {
        'connection.attached': true,
        'session.name': msg.id,
        'scroll.userScrolledUpBeforeDisconnect': false
      },
      effects: [
        ...(msg.buffer ? [{ type: 'terminalWrite', data: msg.buffer, preserveScroll: false }] : []),
        { type: 'sendResize', id: msg.id },
        { type: 'updateP2PIndicator' },
        { type: 'initP2P' },
        { type: 'scrollToBottomIfNeeded', condition: !currentState.scroll.userScrolledUpBeforeDisconnect }
      ]
    }),

    'session.output': (msg) => ({
      stateUpdates: {},
      effects: [
        { type: 'terminalWrite', data: msg.data, preserveScroll: true }
      ]
    }),

    'session.exit': (msg) => ({
      stateUpdates: {},
      effects: [{ type: 'terminalWrite', data: `\r\n[process exited with code ${msg.code}]\r\n` }]
    }),

    'session.removed': () => ({
      stateUpdates: {},
      effects: [{ type: 'terminalWrite', data: '\r\n[session deleted]\r\n' }]
    }),

    'session.list': (msg) => ({
      stateUpdates: {},
      effects: [{ type: 'handleSessionList', sessions: msg.sessions }]
    }),

    'session.created': (msg) => ({
      stateUpdates: { 'session.name': msg.id },
      effects: [{ type: 'attachSession', id: msg.id }]
    }),

    'p2p.signal': (msg, currentState) => ({
      stateUpdates: {},
      effects: currentState.p2p?.peer
        ? [{ type: 'p2pSignal', data: msg.data }]
        : []
    }),

    'p2p.ready': () => ({
      stateUpdates: { 'p2p.connected': true },
      effects: [
        { type: 'log', message: '[P2P] DataChannel ready' },
        { type: 'updateP2PIndicator' }
      ]
    }),

    'p2p.closed': () => ({
      stateUpdates: { 'p2p.connected': false },
      effects: [
        { type: 'log', message: '[P2P] DataChannel closed' },
        { type: 'destroyP2P' },
        { type: 'updateP2PIndicator' }
      ]
    }),

    'p2p.unavailable': () => ({
      stateUpdates: {},
      effects: [{ type: 'log', message: '[P2P] Server WebRTC unavailable' }]
    }),

    'server.draining': () => ({
      stateUpdates: {},
      effects: [
        { type: 'log', message: '[WS] Server is draining, reconnecting immediately' },
        { type: 'fastReconnect' }
      ]
    }),

    'error': (msg) => ({
      stateUpdates: {},
      effects: [{ type: 'log', message: `[server error] ${msg.message}` }]
    })
  };

  // Effect executor (side effects at edges)
  function executeEffect(effect) {
    switch (effect.type) {
      case 'updateP2PIndicator':
        if (updateP2PIndicator) updateP2PIndicator();
        break;
      case 'initP2P':
        if (p2pManager) p2pManager.create();
        break;
      case 'p2pSignal':
        if (p2pManager) p2pManager.signal(effect.data);
        break;
      case 'destroyP2P':
        if (p2pManager) p2pManager.destroy();
        break;
      case 'log':
        console.log(effect.message);
        break;
      case 'scrollToBottomIfNeeded':
        if (effect.condition) {
          scrollToBottom(term);
        }
        break;
      case 'terminalWrite':
        if (effect.preserveScroll) {
          terminalWriteWithScroll(term, effect.data);
        } else {
          term.write(effect.data);
        }
        break;
      case 'reload':
        location.reload();
        break;
      case 'updateSessionUI':
        document.title = effect.name;
        if (deps.renderBar) deps.renderBar(effect.name);
        break;
      case 'refreshTokensAfterRegistration':
        if (loadTokens) loadTokens();
        break;
      case 'handleSessionList': {
        const sessions = effect.sessions || [];
        if (sessions.length === 0) {
          // No sessions — create one
          wsSend({ type: 'session.create', kind: 'terminal', config: { name: 'main' } });
        } else {
          // Attach to first session
          const s = sessions[0];
          const id = s.name || s.id;
          state.update('session.name', id);
          wsSend({
            type: 'session.attach',
            id,
            viewport: { cols: term.cols, rows: term.rows }
          });
        }
        break;
      }
      case 'attachSession':
        wsSend({
          type: 'session.attach',
          id: effect.id,
          viewport: { cols: term.cols, rows: term.rows }
        });
        break;
      case 'sendResize':
        wsSend({
          type: 'session.resize',
          id: effect.id,
          cols: term.cols,
          rows: term.rows
        });
        break;
      case 'fastReconnect':
        state.connection.reconnectDelay = 500;
        if (state.connection.ws && state.connection.ws.readyState === WebSocket.OPEN) {
          state.connection.ws.close();
        }
        break;
    }
  }

  // WebSocket connection function
  function connect() {
    if (isConnecting) return;

    if (reconnectTimeout) {
      clearTimeout(reconnectTimeout);
      reconnectTimeout = null;
    }

    isConnecting = true;
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    // abot WebSocket endpoint is /stream
    state.connection.ws = new WebSocket(`${proto}//${location.host}/stream`);

    state.connection.ws.onopen = () => {
      isConnecting = false;
      state.connection.reconnectDelay = 1000;
      // abot flow: request session list, then attach or create in the handler
      wsSend({ type: 'session.list' });
    };

    state.connection.ws.onmessage = (e) => {
      const msg = JSON.parse(e.data);
      const handler = wsMessageHandlers[msg.type];

      if (handler) {
        const { stateUpdates, effects } = handler(msg, state);

        if (Object.keys(stateUpdates).length > 0) {
          state.updateMany(stateUpdates);
        }

        effects.forEach(executeEffect);
      } else {
        console.log('[ws] unhandled:', msg.type, msg);
      }
    };

    state.connection.ws.onclose = (event) => {
      isConnecting = false;

      if (event.code === 1008) {
        window.location.href = '/login?reason=revoked';
        return;
      }

      const viewport = document.querySelector(".xterm-viewport");
      state.scroll.userScrolledUpBeforeDisconnect = !isAtBottom(viewport);
      state.connection.attached = false;
      if (p2pManager) p2pManager.destroy();

      console.log(`[WS] Reconnecting in ${state.connection.reconnectDelay}ms`);
      reconnectTimeout = setTimeout(connect, state.connection.reconnectDelay);
      state.connection.reconnectDelay = Math.min(state.connection.reconnectDelay * 2, 10000);
    };

    state.connection.ws.onerror = () => {
      isConnecting = false;
      state.connection.ws.close();
    };
  }

  // Visibility change handler for reconnection after backgrounding
  function initVisibilityReconnect() {
    let hiddenAt = 0;
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) {
        hiddenAt = Date.now();
      } else {
        const hiddenDuration = Date.now() - hiddenAt;
        if (isConnecting) return;

        if (hiddenDuration > 5000 && state.connection.ws && !isConnecting) {
          state.connection.ws.close();
        } else if (state.connection.ws && state.connection.ws.readyState === WebSocket.OPEN) {
          try {
            // abot resize includes session ID
            const id = state.session.name;
            state.connection.ws.send(JSON.stringify({
              type: "session.resize", id, cols: term.cols, rows: term.rows
            }));
          } catch {
            state.connection.ws.close();
          }
        }
      }
    });
  }

  return {
    connect,
    initVisibilityReconnect,
    wsMessageHandlers,
    executeEffect
  };
}
