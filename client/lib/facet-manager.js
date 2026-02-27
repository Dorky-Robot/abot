/**
 * Facet Manager — Hybrid Windowing System
 *
 * Tile by default, float on drag. Facets auto-tile via CSS Grid when created.
 * Dragging a titlebar past an 8px threshold pops it into floating mode.
 * Double-clicking/tapping the titlebar snaps it back into the grid.
 *
 * Tiling layouts:
 *   1 facet  → fullscreen (hide titlebar)
 *   2 facets → side-by-side 50/50 (stacked on narrow)
 *   3 facets → master-stack: left full-height, right column 50/50
 *   4 facets → 2×2 grid
 *   5+ facets → master-stack generalized
 *   <768px   → vertical stack
 */

import { Terminal } from "/vendor/xterm/xterm.esm.js";
import { FitAddon } from "/vendor/xterm/addon-fit.esm.js";
import { WebLinksAddon } from "/vendor/xterm/addon-web-links.esm.js";
import { WebglAddon } from "/vendor/xterm/addon-webgl.esm.js";
import { SearchAddon } from "/vendor/xterm/addon-search.esm.js";
import { ClipboardAddon } from "/vendor/xterm/addon-clipboard.esm.js";
import { withPreservedScroll, scrollToBottom } from "/lib/scroll-utils.js";

let nextZ = 10;
let facetIdCounter = 0;

const NARROW_BREAKPOINT = 768;
const DRAG_THRESHOLD = 8;

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

  // --- Windowing state ---
  /** Ordered list of facet IDs in the tile grid */
  const tiledOrder = [];
  /** Set mirror of tiledOrder for O(1) membership checks */
  const tiledSet = new Set();
  /** Set of facet IDs currently floating (maintained for future queries) */
  const floatingSet = new Set();

  // --- Global drag/resize controller ---
  let activeDrag = null;   // { facetId, startX, startY, startLeft, startTop, committed }
  let activeResize = null; // { facetId, startX, startY, startW, startH }

  function setupGlobalPointerHandlers() {
    function onMove(clientX, clientY) {
      if (activeDrag) {
        const facet = facets.get(activeDrag.facetId);
        if (!facet) return;
        const dx = clientX - activeDrag.startX;
        const dy = clientY - activeDrag.startY;
        const dist = Math.sqrt(dx * dx + dy * dy);

        if (!activeDrag.committed) {
          if (dist < DRAG_THRESHOLD) return;
          activeDrag.committed = true;
          // If tiled, float it out
          if (isTiled(activeDrag.facetId)) {
            floatOut(activeDrag.facetId);
          }
          facet.el.classList.add("facet-dragging");
        }

        facet.el.style.left = (activeDrag.startLeft + dx) + "px";
        facet.el.style.top = (activeDrag.startTop + dy) + "px";
      }

      if (activeResize) {
        const facet = facets.get(activeResize.facetId);
        if (!facet) return;
        const dx = clientX - activeResize.startX;
        const dy = clientY - activeResize.startY;
        facet.el.style.width = Math.max(300, activeResize.startW + dx) + "px";
        facet.el.style.height = Math.max(200, activeResize.startH + dy) + "px";
      }
    }

    function onEnd() {
      if (activeDrag) {
        const facet = facets.get(activeDrag.facetId);
        if (facet) facet.el.classList.remove("facet-dragging");
        activeDrag = null;
      }
      if (activeResize) {
        const facet = facets.get(activeResize.facetId);
        if (facet) facet.el.classList.remove("facet-resizing");
        activeResize = null;
      }
    }

    window.addEventListener("mousemove", (e) => onMove(e.clientX, e.clientY));
    window.addEventListener("mouseup", onEnd);
    window.addEventListener("touchmove", (e) => {
      if (activeDrag || activeResize) {
        e.preventDefault();
        const t = e.touches[0];
        onMove(t.clientX, t.clientY);
      }
    }, { passive: false });
    window.addEventListener("touchend", onEnd, { passive: true });
  }

  setupGlobalPointerHandlers();

  // --- Terminal creation ---

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

  // --- Tiling algorithm ---

  /**
   * Compute CSS Grid layout for N tiled facets.
   * Returns { gridTemplateColumns, gridTemplateRows, areas } where areas[i] = { gridColumn, gridRow }
   */
  function computeTileLayout(count, containerWidth) {
    const narrow = containerWidth < NARROW_BREAKPOINT;

    if (count === 0) return { gridTemplateColumns: "1fr", gridTemplateRows: "1fr", areas: [] };

    if (count === 1) {
      return {
        gridTemplateColumns: "1fr",
        gridTemplateRows: "1fr",
        areas: [{ gridColumn: "1", gridRow: "1" }],
      };
    }

    if (narrow) {
      // Vertical stack
      const rows = Array(count).fill("1fr").join(" ");
      return {
        gridTemplateColumns: "1fr",
        gridTemplateRows: rows,
        areas: Array.from({ length: count }, (_, i) => ({
          gridColumn: "1",
          gridRow: `${i + 1}`,
        })),
      };
    }

    if (count === 2) {
      return {
        gridTemplateColumns: "1fr 1fr",
        gridTemplateRows: "1fr",
        areas: [
          { gridColumn: "1", gridRow: "1" },
          { gridColumn: "2", gridRow: "1" },
        ],
      };
    }

    if (count === 3) {
      // Master-stack: left spans full height, right column split 50/50
      return {
        gridTemplateColumns: "1fr 1fr",
        gridTemplateRows: "1fr 1fr",
        areas: [
          { gridColumn: "1", gridRow: "1 / 3" },
          { gridColumn: "2", gridRow: "1" },
          { gridColumn: "2", gridRow: "2" },
        ],
      };
    }

    if (count === 4) {
      return {
        gridTemplateColumns: "1fr 1fr",
        gridTemplateRows: "1fr 1fr",
        areas: [
          { gridColumn: "1", gridRow: "1" },
          { gridColumn: "2", gridRow: "1" },
          { gridColumn: "1", gridRow: "2" },
          { gridColumn: "2", gridRow: "2" },
        ],
      };
    }

    // 5+: Master-stack generalized — left = main, right = stacked
    const stackCount = count - 1;
    const rows = Array(stackCount).fill("1fr").join(" ");
    const areas = [
      { gridColumn: "1", gridRow: `1 / ${stackCount + 1}` },
    ];
    for (let i = 0; i < stackCount; i++) {
      areas.push({ gridColumn: "2", gridRow: `${i + 1}` });
    }
    return {
      gridTemplateColumns: "1fr 1fr",
      gridTemplateRows: rows,
      areas,
    };
  }

  /**
   * Apply the tiling layout to the container and tiled facets.
   */
  function applyTileLayout() {
    const tiledFacets = getTiledFacets();
    const count = tiledFacets.length;
    const containerWidth = container.clientWidth;
    const layout = computeTileLayout(count, containerWidth);

    container.style.gridTemplateColumns = layout.gridTemplateColumns;
    container.style.gridTemplateRows = layout.gridTemplateRows;

    const isSolo = count === 1;

    tiledFacets.forEach((facet, i) => {
      const area = layout.areas[i];
      if (area) {
        facet.el.style.gridColumn = area.gridColumn;
        facet.el.style.gridRow = area.gridRow;
      }

      // Solo class for hiding titlebar when only one tiled facet
      facet.el.classList.toggle("facet-solo", isSolo);

      // Always show close on multi-facet, hide on solo only if it's the last overall facet
      facet.closeBtn.style.display = (facets.size <= 1) ? "none" : "";
    });

    // Refit all tiled terminals after layout change
    requestAnimationFrame(() => {
      for (const facet of tiledFacets) {
        facet.fit.fit();
      }
    });
  }

  // --- Facet creation ---

  /**
   * Create a new facet. Always enters tiled mode.
   * @param {string} sessionName - Name of the session to attach
   * @returns {object} facet
   */
  function create(sessionName) {
    const id = `facet-${facetIdCounter++}`;

    // Determine theme
    const effectiveTheme = themeManager?.getEffective?.() || "dark";
    const themeData = effectiveTheme === "light"
      ? (themeManager?.LIGHT_THEME || {})
      : (themeManager?.DARK_THEME || {});

    const { term, fit, searchAddon } = createTerminal(themeData);

    // Build DOM
    const el = document.createElement("div");
    el.className = "facet facet-tiled";
    el.dataset.facetId = id;

    // Title bar
    const titleBar = document.createElement("div");
    titleBar.className = "facet-titlebar";

    const titleText = document.createElement("span");
    titleText.className = "facet-title";
    titleText.textContent = sessionName;
    titleBar.appendChild(titleText);

    // Titlebar right-side controls
    const titleControls = document.createElement("span");
    titleControls.className = "facet-titlebar-controls";

    // Maximize/restore button
    const maxBtn = document.createElement("button");
    maxBtn.className = "facet-max-btn";
    maxBtn.innerHTML = '<i class="ph ph-arrows-out"></i>';
    maxBtn.title = "Float out";
    maxBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      if (isTiled(id)) {
        floatOut(id);
      } else {
        snapBack(id);
      }
    });
    titleControls.appendChild(maxBtn);

    const closeBtn = document.createElement("button");
    closeBtn.className = "facet-close";
    closeBtn.innerHTML = '<i class="ph ph-x"></i>';
    closeBtn.title = "Close facet";
    closeBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      remove(id);
    });
    titleControls.appendChild(closeBtn);
    titleBar.appendChild(titleControls);

    el.appendChild(titleBar);

    // Terminal container
    const termContainer = document.createElement("div");
    termContainer.className = "facet-terminal";
    el.appendChild(termContainer);

    // Resize handle (only visible when floating)
    const resizeHandle = document.createElement("div");
    resizeHandle.className = "facet-resize-handle";
    el.appendChild(resizeHandle);

    container.appendChild(el);
    term.open(termContainer);
    tryLoadWebGL(term);

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
      maxBtn,
      termContainer,
      resizeHandle,
      resizeObserver: null,
      mutationObserver: null,
    };

    facets.set(id, facet);

    // Add to tiled order
    tiledOrder.push(id);
    tiledSet.add(id);

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

    // Drag (titlebar) — start tracking, global handler does the rest
    initDrag(facet);

    // Resize handle — start tracking for floating facets
    initResize(facet);

    // Double-click/tap titlebar to snap back
    initSnapBack(facet);

    // Apply tiling layout
    applyTileLayout();

    // Initial fit after DOM layout settles
    requestAnimationFrame(() => {
      fit.fit();
      scrollToBottom(term);
    });

    // Focus this facet
    focus(id);

    // Patch textarea for mobile
    facet.mutationObserver = patchTerminalTextarea(termContainer);

    return facet;
  }

  function isTitlebarButton(e) {
    return e.target.closest(".facet-close") || e.target.closest(".facet-max-btn");
  }

  // --- Drag to move (titlebar) ---

  function initDrag(facet) {
    const { id, el, titleBar } = facet;

    function onStart(clientX, clientY) {
      const rect = el.getBoundingClientRect();
      const containerRect = container.getBoundingClientRect();
      activeDrag = {
        facetId: id,
        startX: clientX,
        startY: clientY,
        startLeft: rect.left - containerRect.left,
        startTop: rect.top - containerRect.top,
        committed: false,
      };

      // If already floating, mark committed immediately (no threshold needed)
      if (!isTiled(id)) {
        activeDrag.committed = true;
        el.classList.add("facet-dragging");
      }
    }

    titleBar.addEventListener("mousedown", (e) => {
      if (isTitlebarButton(e)) return;
      e.preventDefault();
      onStart(e.clientX, e.clientY);
    });

    titleBar.addEventListener("touchstart", (e) => {
      if (isTitlebarButton(e)) return;
      const t = e.touches[0];
      onStart(t.clientX, t.clientY);
    }, { passive: true });
  }

  // --- Resize (corner handle, floating only) ---

  function initResize(facet) {
    const { id, el, resizeHandle } = facet;

    function onStart(clientX, clientY) {
      if (isTiled(id)) return; // Only floating facets can be resized
      const rect = el.getBoundingClientRect();
      activeResize = {
        facetId: id,
        startX: clientX,
        startY: clientY,
        startW: rect.width,
        startH: rect.height,
      };
      el.classList.add("facet-resizing");
    }

    resizeHandle.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      onStart(e.clientX, e.clientY);
    });

    resizeHandle.addEventListener("touchstart", (e) => {
      e.stopPropagation();
      const t = e.touches[0];
      onStart(t.clientX, t.clientY);
    }, { passive: true });
  }

  // --- Snap back on double-click/tap ---

  function initSnapBack(facet) {
    const { id, titleBar } = facet;
    let lastTap = 0;

    titleBar.addEventListener("dblclick", (e) => {
      if (isTitlebarButton(e)) return;
      if (!isTiled(id)) {
        snapBack(id);
      }
    });

    // Double-tap for touch
    titleBar.addEventListener("touchend", (e) => {
      if (isTitlebarButton(e)) return;
      const now = Date.now();
      if (now - lastTap < 300) {
        if (!isTiled(id)) {
          snapBack(id);
        }
        lastTap = 0;
      } else {
        lastTap = now;
      }
    }, { passive: true });
  }

  // --- Float out / snap back ---

  /**
   * Float a facet out of the tile grid.
   */
  function floatOut(id) {
    const facet = facets.get(id);
    if (!facet || !isTiled(id)) return;

    // Snapshot current rect before removing from grid
    const rect = facet.el.getBoundingClientRect();
    const containerRect = container.getBoundingClientRect();

    // Remove from tiled order
    const idx = tiledOrder.indexOf(id);
    if (idx !== -1) tiledOrder.splice(idx, 1);
    tiledSet.delete(id);

    // Add to floating set
    floatingSet.add(id);

    // Swap CSS classes
    facet.el.classList.remove("facet-tiled", "facet-solo");
    facet.el.classList.add("facet-floating");

    // Position at snapshotted location (container-relative, not viewport-relative)
    facet.el.style.left = (rect.left - containerRect.left) + "px";
    facet.el.style.top = (rect.top - containerRect.top) + "px";
    facet.el.style.width = rect.width + "px";
    facet.el.style.height = rect.height + "px";
    facet.el.style.gridColumn = "";
    facet.el.style.gridRow = "";

    // Z-order
    facet.el.style.zIndex = nextZ++;

    // Update maximize button icon
    facet.maxBtn.innerHTML = '<i class="ph ph-arrows-in"></i>';
    facet.maxBtn.title = "Snap back";

    // Re-tile remaining
    applyTileLayout();

    // Refit the floating facet
    requestAnimationFrame(() => facet.fit.fit());
  }

  /**
   * Snap a floating facet back into the tile grid.
   */
  function snapBack(id) {
    const facet = facets.get(id);
    if (!facet || isTiled(id)) return;

    // Remove from floating set
    floatingSet.delete(id);

    // Add to tiled order
    tiledOrder.push(id);
    tiledSet.add(id);

    // Swap CSS classes
    facet.el.classList.remove("facet-floating");
    facet.el.classList.add("facet-tiled");

    // Clear inline positioning
    facet.el.style.left = "";
    facet.el.style.top = "";
    facet.el.style.width = "";
    facet.el.style.height = "";
    facet.el.style.zIndex = "";

    // Update maximize button icon
    facet.maxBtn.innerHTML = '<i class="ph ph-arrows-out"></i>';
    facet.maxBtn.title = "Float out";

    // Re-tile
    applyTileLayout();
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

    // Only bump z-index for floating facets
    if (!isTiled(id)) {
      facet.el.style.zIndex = nextZ++;
    }

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
    if (facet.mutationObserver) facet.mutationObserver.disconnect();
    facet.term.dispose();
    facet.el.remove();
    facets.delete(id);

    // Remove from windowing state
    const tiledIdx = tiledOrder.indexOf(id);
    if (tiledIdx !== -1) tiledOrder.splice(tiledIdx, 1);
    tiledSet.delete(id);
    floatingSet.delete(id);

    if (onClose) onClose(id, facet.sessionName);

    // Focus another facet if we removed the focused one
    if (focusedId === id) {
      const remaining = [...facets.keys()];
      if (remaining.length > 0) {
        focus(remaining[remaining.length - 1]);
      }
    }

    // Re-tile remaining
    applyTileLayout();
  }

  // --- Windowing queries ---

  function isTiled(id) {
    return tiledSet.has(id);
  }

  function getTiledFacets() {
    return tiledOrder.map(id => facets.get(id)).filter(Boolean);
  }

  // --- Theme ---

  function applyThemeToAll(themeData) {
    for (const facet of facets.values()) {
      withPreservedScroll(facet.term, () => {
        facet.term.options.theme = themeData;
      });
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
    const mo = new MutationObserver(patch);
    mo.observe(termContainer, { childList: true, subtree: true });
    return mo;
  }

  // --- Container resize: re-tile on window resize ---
  const resizeObserver = new ResizeObserver(() => {
    applyTileLayout();
  });
  resizeObserver.observe(container);

  return {
    create,
    remove,
    focus,
    getFocused,
    cycleFocus,
    getBySession,
    getAll,
    count,
    applyThemeToAll,
    isTiled,
    getTiledFacets,
    floatOut,
    snapBack,
  };
}
