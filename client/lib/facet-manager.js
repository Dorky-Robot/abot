/**
 * Facet Manager — Windowing System
 *
 * Each facet is a floating panel containing its own xterm.js Terminal.
 * Supports drag-to-move, edge-drag-to-resize, z-order on focus,
 * and localStorage persistence for layout.
 *
 * Default: single facet filling the screen (matches legacy behavior).
 * Users add more facets for multi-session side-by-side layout.
 */

import { Terminal } from "/vendor/xterm/xterm.esm.js";
import { FitAddon } from "/vendor/xterm/addon-fit.esm.js";
import { WebLinksAddon } from "/vendor/xterm/addon-web-links.esm.js";
import { WebglAddon } from "/vendor/xterm/addon-webgl.esm.js";
import { SearchAddon } from "/vendor/xterm/addon-search.esm.js";
import { ClipboardAddon } from "/vendor/xterm/addon-clipboard.esm.js";
import { withPreservedScroll, scrollToBottom, terminalWriteWithScroll } from "/lib/scroll-utils.js";

let nextZ = 10;
let facetIdCounter = 0;

/**
 * Create a facet manager
 */
export function createFacetManager(options = {}) {
  const {
    container,          // Parent element for facets (e.g. #facet-layer)
    themeManager,       // { getEffective, DARK_THEME, LIGHT_THEME }
    onFocusChange,      // (facet) => void — called when focused facet changes
    onResize,           // (facetId, cols, rows) => void — send resize to server
    onClose,            // (facetId, sessionName) => void — facet closed
  } = options;

  /** @type {Map<string, Facet>} */
  const facets = new Map();
  let focusedId = null;

  // --- Facet creation ---

  function createTerminal(themeData) {
    const term = new Terminal({
      fontSize: 14,
      fontFamily: "'JetBrains Mono', 'SF Mono', Monaco, 'Cascadia Code', 'Roboto Mono', Consolas, 'Courier New', monospace",
      theme: themeData,
      cursorBlink: true,
      scrollback: 10000,
      convertEol: true,
      macOptionIsMeta: true,
      minimumContrastRatio: 4.5,
      cursorInactiveStyle: 'outline',
      rightClickSelectsWord: true,
      rescaleOverlappingGlyphs: true,
    });

    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());

    const searchAddon = new SearchAddon();
    term.loadAddon(searchAddon);
    term.loadAddon(new ClipboardAddon());

    return { term, fit, searchAddon };
  }

  function tryLoadWebGL(term) {
    try {
      const testCanvas = document.createElement("canvas");
      const gl = testCanvas.getContext("webgl2", { failIfMajorPerformanceCaveat: true });
      if (gl) {
        const webgl = new WebglAddon();
        webgl.onContextLoss(() => webgl.dispose());
        term.loadAddon(webgl);
      }
    } catch {
      // WebGL unavailable
    }
  }

  /**
   * Create a new facet
   * @param {string} sessionName - Name of the session to attach
   * @param {object} [layout] - Optional {x, y, width, height} in percentages
   * @returns {object} facet
   */
  function create(sessionName, layout) {
    const id = `facet-${facetIdCounter++}`;
    const isFirst = facets.size === 0;

    // Determine theme
    const effectiveTheme = themeManager?.getEffective?.() || "dark";
    const themeData = effectiveTheme === "light"
      ? (themeManager?.LIGHT_THEME || {})
      : (themeManager?.DARK_THEME || {});

    const { term, fit, searchAddon } = createTerminal(themeData);

    // Build DOM
    const el = document.createElement("div");
    el.className = "facet" + (isFirst ? " facet-fullscreen" : "");
    el.dataset.facetId = id;

    // Title bar
    const titleBar = document.createElement("div");
    titleBar.className = "facet-titlebar";

    const titleText = document.createElement("span");
    titleText.className = "facet-title";
    titleText.textContent = sessionName;
    titleBar.appendChild(titleText);

    const closeBtn = document.createElement("button");
    closeBtn.className = "facet-close";
    closeBtn.innerHTML = '<i class="ph ph-x"></i>';
    closeBtn.title = "Close facet";
    closeBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      remove(id);
    });
    // Don't show close on the first (default) facet
    if (isFirst) closeBtn.style.display = "none";
    titleBar.appendChild(closeBtn);

    el.appendChild(titleBar);

    // Terminal container
    const termContainer = document.createElement("div");
    termContainer.className = "facet-terminal";
    el.appendChild(termContainer);

    // Resize handle (bottom-right corner)
    const resizeHandle = document.createElement("div");
    resizeHandle.className = "facet-resize-handle";
    el.appendChild(resizeHandle);

    container.appendChild(el);
    term.open(termContainer);
    tryLoadWebGL(term);

    // Apply layout
    if (!isFirst && layout) {
      el.style.left = layout.x + "%";
      el.style.top = layout.y + "%";
      el.style.width = layout.width + "%";
      el.style.height = layout.height + "%";
    } else if (!isFirst) {
      // Default position for new facets: center, 50% size
      el.style.left = "25%";
      el.style.top = "10%";
      el.style.width = "50%";
      el.style.height = "70%";
    }

    // Z-order
    el.style.zIndex = nextZ++;

    const facet = {
      id,
      sessionName,
      el,
      term,
      fit,
      searchAddon,
      titleBar,
      titleText,
      closeBtn,
      termContainer,
      resizeHandle,
      resizeObserver: null,
    };

    facets.set(id, facet);

    // ResizeObserver for terminal fit
    const ro = new ResizeObserver(() => {
      withPreservedScroll(term, () => fit.fit());
      if (onResize) {
        onResize(id, term.cols, term.rows);
      }
    });
    ro.observe(termContainer);
    facet.resizeObserver = ro;

    // Focus on click
    el.addEventListener("mousedown", () => focus(id));
    el.addEventListener("touchstart", () => focus(id), { passive: true });

    // Drag to move (title bar)
    initDrag(facet);

    // Resize (corner handle)
    initResize(facet);

    // Initial fit after DOM layout settles
    requestAnimationFrame(() => {
      fit.fit();
      scrollToBottom(term);
    });

    // Focus this facet
    focus(id);

    // Patch textarea for mobile
    patchTerminalTextarea(termContainer);

    return facet;
  }

  // --- Drag to move ---

  function initDrag(facet) {
    const { el, titleBar } = facet;
    let dragging = false;
    let startX, startY, startLeft, startTop;

    function onStart(clientX, clientY) {
      if (el.classList.contains("facet-fullscreen")) return;
      dragging = true;
      startX = clientX;
      startY = clientY;
      const rect = el.getBoundingClientRect();
      startLeft = rect.left;
      startTop = rect.top;
      el.classList.add("facet-dragging");
    }

    function onMove(clientX, clientY) {
      if (!dragging) return;
      const dx = clientX - startX;
      const dy = clientY - startY;
      el.style.left = (startLeft + dx) + "px";
      el.style.top = (startTop + dy) + "px";
      // Switch to px positioning once dragged
      el.style.right = "auto";
      el.style.bottom = "auto";
    }

    function onEnd() {
      if (!dragging) return;
      dragging = false;
      el.classList.remove("facet-dragging");
      saveLayout();
    }

    titleBar.addEventListener("mousedown", (e) => {
      if (e.target.closest(".facet-close")) return;
      e.preventDefault();
      onStart(e.clientX, e.clientY);
    });
    window.addEventListener("mousemove", (e) => onMove(e.clientX, e.clientY));
    window.addEventListener("mouseup", onEnd);

    titleBar.addEventListener("touchstart", (e) => {
      if (e.target.closest(".facet-close")) return;
      const t = e.touches[0];
      onStart(t.clientX, t.clientY);
    }, { passive: true });
    window.addEventListener("touchmove", (e) => {
      if (dragging) {
        const t = e.touches[0];
        onMove(t.clientX, t.clientY);
      }
    }, { passive: true });
    window.addEventListener("touchend", onEnd, { passive: true });
  }

  // --- Resize ---

  function initResize(facet) {
    const { el, resizeHandle } = facet;
    let resizing = false;
    let startX, startY, startW, startH;

    function onStart(clientX, clientY) {
      if (el.classList.contains("facet-fullscreen")) return;
      resizing = true;
      startX = clientX;
      startY = clientY;
      const rect = el.getBoundingClientRect();
      startW = rect.width;
      startH = rect.height;
      el.classList.add("facet-resizing");
    }

    function onMove(clientX, clientY) {
      if (!resizing) return;
      const dx = clientX - startX;
      const dy = clientY - startY;
      el.style.width = Math.max(300, startW + dx) + "px";
      el.style.height = Math.max(200, startH + dy) + "px";
    }

    function onEnd() {
      if (!resizing) return;
      resizing = false;
      el.classList.remove("facet-resizing");
      saveLayout();
    }

    resizeHandle.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      onStart(e.clientX, e.clientY);
    });
    window.addEventListener("mousemove", (e) => {
      if (resizing) onMove(e.clientX, e.clientY);
    });
    window.addEventListener("mouseup", onEnd);

    resizeHandle.addEventListener("touchstart", (e) => {
      e.stopPropagation();
      const t = e.touches[0];
      onStart(t.clientX, t.clientY);
    }, { passive: true });
    window.addEventListener("touchmove", (e) => {
      if (resizing) {
        const t = e.touches[0];
        onMove(t.clientX, t.clientY);
      }
    }, { passive: true });
    window.addEventListener("touchend", () => {
      if (resizing) onEnd();
    }, { passive: true });
  }

  // --- Focus management ---

  function focus(id) {
    const facet = facets.get(id);
    if (!facet) return;

    // Unfocus previous
    if (focusedId && focusedId !== id) {
      const prev = facets.get(focusedId);
      if (prev) {
        prev.el.classList.remove("facet-focused");
      }
    }

    focusedId = id;
    facet.el.classList.add("facet-focused");
    facet.el.style.zIndex = nextZ++;
    facet.term.focus();

    if (onFocusChange) onFocusChange(facet);
  }

  function getFocused() {
    return focusedId ? facets.get(focusedId) : null;
  }

  function cycleFocus() {
    const ids = [...facets.keys()];
    if (ids.length <= 1) return;

    const currentIdx = ids.indexOf(focusedId);
    const nextIdx = (currentIdx + 1) % ids.length;
    focus(ids[nextIdx]);
  }

  // --- Remove ---

  function remove(id) {
    const facet = facets.get(id);
    if (!facet) return;

    // Don't remove the last facet
    if (facets.size <= 1) return;

    if (facet.resizeObserver) facet.resizeObserver.disconnect();
    facet.term.dispose();
    facet.el.remove();
    facets.delete(id);

    if (onClose) onClose(id, facet.sessionName);

    // Focus another facet if we removed the focused one
    if (focusedId === id) {
      const remaining = [...facets.keys()];
      if (remaining.length > 0) {
        focus(remaining[remaining.length - 1]);
      }
    }

    // If only one facet remains, make it fullscreen
    if (facets.size === 1) {
      const last = facets.values().next().value;
      last.el.classList.add("facet-fullscreen");
      last.el.style.left = "";
      last.el.style.top = "";
      last.el.style.width = "";
      last.el.style.height = "";
      last.closeBtn.style.display = "none";
      requestAnimationFrame(() => last.fit.fit());
    }

    saveLayout();
  }

  // --- Multi-facet layout ---

  /**
   * When adding a second facet, convert from fullscreen to tiled layout.
   * Splits the screen: existing facet takes left half, new facet takes right half.
   */
  function exitFullscreen() {
    for (const facet of facets.values()) {
      if (facet.el.classList.contains("facet-fullscreen")) {
        facet.el.classList.remove("facet-fullscreen");
        facet.el.style.left = "0";
        facet.el.style.top = "0";
        facet.el.style.width = "50%";
        facet.el.style.height = "100%";
        facet.closeBtn.style.display = "";
        requestAnimationFrame(() => facet.fit.fit());
      }
    }
  }

  /**
   * Create a new facet and tile it alongside existing ones
   */
  function createTiled(sessionName) {
    if (facets.size === 1) {
      exitFullscreen();
      return create(sessionName, { x: 50, y: 0, width: 50, height: 100 });
    }

    // For 3+ facets, stack in the right half
    const count = facets.size;
    const height = 100 / Math.ceil(count / 2);
    return create(sessionName, { x: 50, y: 0, width: 50, height });
  }

  // --- Theme ---

  function applyThemeToAll(themeData) {
    for (const facet of facets.values()) {
      withPreservedScroll(facet.term, () => {
        facet.term.options.theme = themeData;
      });
    }
  }

  // --- Layout persistence ---

  function saveLayout() {
    const layout = {};
    for (const [id, facet] of facets) {
      const rect = facet.el.getBoundingClientRect();
      layout[id] = {
        sessionName: facet.sessionName,
        x: rect.left,
        y: rect.top,
        width: rect.width,
        height: rect.height,
        fullscreen: facet.el.classList.contains("facet-fullscreen"),
      };
    }
    try {
      localStorage.setItem("abot_facet_layout", JSON.stringify(layout));
    } catch {
      // localStorage may be full or unavailable
    }
  }

  function loadLayout() {
    try {
      const raw = localStorage.getItem("abot_facet_layout");
      return raw ? JSON.parse(raw) : null;
    } catch {
      return null;
    }
  }

  // --- Utilities ---

  function getBySession(sessionName) {
    for (const facet of facets.values()) {
      if (facet.sessionName === sessionName) return facet;
    }
    return null;
  }

  function getAll() {
    return [...facets.values()];
  }

  function count() {
    return facets.size;
  }

  function patchTerminalTextarea(termContainer) {
    const patch = () => {
      const ta = termContainer.querySelector(".xterm-helper-textarea");
      if (!ta || ta._patched) return;
      ta._patched = true;
      ta.setAttribute("autocorrect", "off");
      ta.setAttribute("autocapitalize", "none");
      ta.setAttribute("autocomplete", "new-password");
      ta.setAttribute("spellcheck", "false");
    };
    patch();
    new MutationObserver(patch).observe(termContainer, { childList: true, subtree: true });
  }

  return {
    create,
    createTiled,
    remove,
    focus,
    getFocused,
    cycleFocus,
    getBySession,
    getAll,
    count,
    applyThemeToAll,
    saveLayout,
    loadLayout,
  };
}
