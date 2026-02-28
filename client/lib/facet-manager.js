/**
 * Facet Manager — Tiled Windowing System
 *
 * All facets live in a CSS Grid tile layout organized into columns. Each column
 * contains one or more vertically stacked facets. Dragging a titlebar
 * horizontally onto another facet swaps their columns (FLIP animated).
 *
 * Dragging a titlebar downward past 80px moves the facet into the adjacent
 * column (appended at the bottom), collapsing from side-by-side into stacked.
 * Dragging upward past 80px splits the facet out of a stacked column into
 * its own new column to the right.
 *
 * Tiling layouts:
 *   1 facet  → fullscreen (hide titlebar)
 *   N facets → equal-width columns; facets within a column share height
 *   <768px   → vertical stack (all facets in one column)
 */

import { Terminal } from "/vendor/xterm/xterm.esm.js";
import { FitAddon } from "/vendor/xterm/addon-fit.esm.js";
import { WebLinksAddon } from "/vendor/xterm/addon-web-links.esm.js";
import { WebglAddon } from "/vendor/xterm/addon-webgl.esm.js";
import { SearchAddon } from "/vendor/xterm/addon-search.esm.js";
import { ClipboardAddon } from "/vendor/xterm/addon-clipboard.esm.js";
import { withPreservedScroll, scrollToBottom } from "/lib/scroll-utils.js";

let facetIdCounter = 0;

const NARROW_BREAKPOINT = 768;
const DRAG_THRESHOLD = 8;

function gcd(a, b) {
  while (b !== 0) { [a, b] = [b, a % b]; }
  return a;
}
function lcm(a, b) { return (a * b) / gcd(a, b); }

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
  /** Columns of facet IDs: each column is a vertical stack */
  const columns = [];

  /** Flatten columns into a single ordered list of IDs (column-major) */
  function getAllTiledIds() {
    const ids = [];
    for (const col of columns) {
      for (const id of col) ids.push(id);
    }
    return ids;
  }

  /** Find which column a facet belongs to: returns [colIndex, rowIndex] or null */
  function findFacet(facetId) {
    for (let c = 0; c < columns.length; c++) {
      const r = columns[c].indexOf(facetId);
      if (r !== -1) return [c, r];
    }
    return null;
  }

  // --- Global drag controller ---
  let activeDrag = null;   // { facetId, startX, startY, committed, lastSwapTime, moveTriggered }
  const SWAP_COOLDOWN = 100; // ms between reorder swaps
  const MOVE_PREVIEW_THRESHOLD = 40;  // px to show move indicator
  const MOVE_THRESHOLD = 80;          // px to execute move

  function setupGlobalPointerHandlers() {
    function onMove(clientX, clientY) {
      if (!activeDrag) return;
      const facet = facets.get(activeDrag.facetId);
      if (!facet) return;
      const dx = clientX - activeDrag.startX;
      const dy = clientY - activeDrag.startY;

      if (!activeDrag.committed) {
        if (dx * dx + dy * dy < DRAG_THRESHOLD * DRAG_THRESHOLD) return;
        activeDrag.committed = true;
        facet.el.classList.add("facet-dragging", "facet-reordering");
        facet.el.style.pointerEvents = "none";
      }

      // Classify drag direction
      const absDx = Math.abs(dx);
      const absDy = Math.abs(dy);
      const primarilyDown = dy > 0 && absDy > absDx;
      const primarilyUp = dy < 0 && absDy > absDx;

      if (primarilyDown && !activeDrag.moveTriggered) {
        // Drag down: move facet into adjacent column (stack)
        // Only valid when there are multiple columns
        if (columns.length <= 1) return;

        if (dy > MOVE_THRESHOLD) {
          const targetEl = document.elementFromPoint(clientX, clientY);
          const targetFacetEl = targetEl?.closest(".facet");
          if (targetFacetEl && targetFacetEl !== facet.el) {
            const targetId = targetFacetEl.dataset.facetId;
            const targetPos = findFacet(targetId);
            if (targetPos) {
              activeDrag.moveTriggered = true;
              facet.el.classList.remove("facet-move-preview", "facet-reordering", "facet-dragging");
              facet.el.style.pointerEvents = "";
              moveFacetToColumn(activeDrag.facetId, targetPos[0]);
              activeDrag = null;
              return;
            }
          }
        } else if (dy > MOVE_PREVIEW_THRESHOLD) {
          facet.el.classList.add("facet-move-preview");
        }
      } else if (primarilyUp && !activeDrag.moveTriggered) {
        // Drag up: split facet out of stacked column into its own column
        // Only valid when the facet's column has multiple facets
        const pos = findFacet(activeDrag.facetId);
        if (!pos || columns[pos[0]].length <= 1) return;

        if (-dy > MOVE_THRESHOLD) {
          activeDrag.moveTriggered = true;
          facet.el.classList.remove("facet-split-out-preview", "facet-reordering", "facet-dragging");
          facet.el.style.pointerEvents = "";
          splitFacetToOwnColumn(activeDrag.facetId);
          activeDrag = null;
          return;
        } else if (-dy > MOVE_PREVIEW_THRESHOLD) {
          facet.el.classList.add("facet-split-out-preview");
        }
      } else {
        facet.el.classList.remove("facet-move-preview", "facet-split-out-preview");

        // Hit-test through the dragged facet to find the target for reorder
        const targetEl = document.elementFromPoint(clientX, clientY);
        const targetFacetEl = targetEl?.closest(".facet");
        const isOverOtherFacet = targetFacetEl && targetFacetEl !== facet.el;

        if (isOverOtherFacet) {
          const targetId = targetFacetEl.dataset.facetId;
          const now = Date.now();
          if (targetId && now - activeDrag.lastSwapTime > SWAP_COOLDOWN) {
            activeDrag.lastSwapTime = now;
            reorderColumns(activeDrag.facetId, targetId);
          }
        }
      }
    }

    function onEnd() {
      if (activeDrag) {
        const wasCommitted = activeDrag.committed;
        const facet = facets.get(activeDrag.facetId);
        if (facet) {
          facet.el.classList.remove("facet-dragging", "facet-reordering", "facet-move-preview", "facet-split-out-preview");
          facet.el.style.pointerEvents = "";
        }
        activeDrag = null;
        // Refit all terminals after reorder (skip if drag never committed)
        if (wasCommitted) {
          requestAnimationFrame(() => {
            for (const f of getTiledFacets()) {
              f.fit.fit();
            }
          });
        }
      }
    }

    window.addEventListener("mousemove", (e) => onMove(e.clientX, e.clientY));
    window.addEventListener("mouseup", onEnd);
    window.addEventListener("touchmove", (e) => {
      if (activeDrag) {
        e.preventDefault();
        const t = e.touches[0];
        onMove(t.clientX, t.clientY);
      }
    }, { passive: false });
    window.addEventListener("touchend", onEnd, { passive: true });
  }

  setupGlobalPointerHandlers();

  // --- FLIP animation ---

  /**
   * Snapshot facet rects, run a mutation callback, apply layout, then FLIP animate.
   * @param {Set<string>|null} skipIds - facet IDs to exclude from the snapshot (e.g. the dragged facet)
   * @param {() => void} mutate - callback that changes `columns` before layout is applied
   */
  function flipAnimate(skipIds, mutate) {
    const allIds = getAllTiledIds();
    const snapshots = new Map();
    for (const id of allIds) {
      if (skipIds && skipIds.has(id)) continue;
      const f = facets.get(id);
      if (f) snapshots.set(id, f.el.getBoundingClientRect());
    }

    mutate();
    applyTileLayout();

    for (const [id, oldRect] of snapshots) {
      const f = facets.get(id);
      if (!f) continue;
      const newRect = f.el.getBoundingClientRect();
      const dx = oldRect.left - newRect.left;
      const dy = oldRect.top - newRect.top;
      if (dx === 0 && dy === 0) continue;

      // Cancel any in-flight FLIP animation to avoid listener leaks
      if (f.el._flipHandler) {
        f.el.removeEventListener("transitionend", f.el._flipHandler);
        f.el._flipHandler = null;
      }

      f.el.style.transform = `translate(${dx}px, ${dy}px)`;
      f.el.style.transition = "none";
      // Force reflow so browser commits the inverse state before transitioning
      void f.el.offsetHeight;
      requestAnimationFrame(() => {
        f.el.style.transition = "transform 200ms ease";
        f.el.style.transform = "none";
        function handler(e) {
          if (e.propertyName !== "transform") return;
          f.el.style.transition = "";
          f.el.style.transform = "";
          f.el.removeEventListener("transitionend", handler);
          f.el._flipHandler = null;
        }
        f.el._flipHandler = handler;
        f.el.addEventListener("transitionend", handler);
      });
    }
  }

  // --- Column operations ---

  /**
   * Swap the columns containing draggedId and targetId with FLIP animation.
   * The dragged facet is excluded from the animation (it's under the pointer).
   */
  function reorderColumns(draggedId, targetId) {
    const dragPos = findFacet(draggedId);
    const targetPos = findFacet(targetId);
    if (!dragPos || !targetPos) return;
    const [dragCol] = dragPos;
    const [targetCol] = targetPos;
    if (dragCol === targetCol) return;

    flipAnimate(new Set([draggedId]), () => {
      [columns[dragCol], columns[targetCol]] = [columns[targetCol], columns[dragCol]];
    });
  }

  /**
   * Move a facet from its current column into another column (appended at bottom).
   * If the source column becomes empty, it is removed. All facets animate (the
   * moved facet's pointer-events and classes are already cleared by the caller).
   */
  function moveFacetToColumn(facetId, targetColIdx) {
    const srcPos = findFacet(facetId);
    if (!srcPos) return;
    const [srcCol, srcRow] = srcPos;
    if (srcCol === targetColIdx) return;

    flipAnimate(null, () => {
      columns[srcCol].splice(srcRow, 1);
      let adjustedTarget = targetColIdx;
      if (columns[srcCol].length === 0) {
        columns.splice(srcCol, 1);
        if (targetColIdx > srcCol) adjustedTarget--;
      }
      columns[adjustedTarget].push(facetId);
    });
  }

  /**
   * Split a facet out of a multi-facet column into its own new column
   * (inserted to the right of the source column). FLIP animated.
   */
  function splitFacetToOwnColumn(facetId) {
    const srcPos = findFacet(facetId);
    if (!srcPos) return;
    const [srcCol, srcRow] = srcPos;
    if (columns[srcCol].length <= 1) return;

    flipAnimate(null, () => {
      columns[srcCol].splice(srcRow, 1);
      columns.splice(srcCol + 1, 0, [facetId]);
    });
  }

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
   * Compute CSS Grid layout from the column structure.
   * Returns { gridTemplateColumns, gridTemplateRows, areas }
   * where areas is ordered column-major (matching getAllTiledIds()).
   */
  function computeTileLayout(cols, containerWidth) {
    const narrow = containerWidth < NARROW_BREAKPOINT;
    const totalFacets = cols.reduce((n, col) => n + col.length, 0);

    if (totalFacets === 0) {
      return { gridTemplateColumns: "1fr", gridTemplateRows: "1fr", areas: [] };
    }

    if (totalFacets === 1) {
      return {
        gridTemplateColumns: "1fr",
        gridTemplateRows: "1fr",
        areas: [{ gridColumn: "1", gridRow: "1" }],
      };
    }

    if (narrow) {
      // Vertical stack — ignore column structure
      const rows = Array(totalFacets).fill("1fr").join(" ");
      let i = 0;
      const areas = [];
      for (const col of cols) {
        for (let r = 0; r < col.length; r++) {
          areas.push({ gridColumn: "1", gridRow: `${++i}` });
        }
      }
      return { gridTemplateColumns: "1fr", gridTemplateRows: rows, areas };
    }

    const numCols = cols.length;
    const heights = cols.map(c => c.length);
    const totalRows = heights.reduce((a, b) => lcm(a, b), 1);

    const gridTemplateColumns = Array(numCols).fill("1fr").join(" ");
    const gridTemplateRows = Array(totalRows).fill("1fr").join(" ");

    const areas = [];
    for (let c = 0; c < numCols; c++) {
      const col = cols[c];
      const rowsPerFacet = totalRows / col.length;
      for (let r = 0; r < col.length; r++) {
        const startRow = r * rowsPerFacet + 1;
        const endRow = startRow + rowsPerFacet;
        areas.push({
          gridColumn: `${c + 1}`,
          gridRow: `${startRow} / ${endRow}`,
        });
      }
    }

    return { gridTemplateColumns, gridTemplateRows, areas };
  }

  /**
   * Apply the tiling layout to the container and all facets.
   */
  function applyTileLayout() {
    const allIds = getAllTiledIds();
    const allFacets = allIds.map(id => facets.get(id)).filter(Boolean);
    const count = allFacets.length;
    const containerWidth = container.clientWidth;
    const layout = computeTileLayout(columns, containerWidth);

    container.style.gridTemplateColumns = layout.gridTemplateColumns;
    container.style.gridTemplateRows = layout.gridTemplateRows;

    const isSolo = count === 1;

    allFacets.forEach((facet, i) => {
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
      for (const facet of allFacets) {
        facet.fit.fit();
      }
    });
  }

  // --- Facet creation ---

  /**
   * Create a new facet. Always enters tiled mode (new column).
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
      termContainer,
      resizeObserver: null,
      mutationObserver: null,
    };

    facets.set(id, facet);

    // Add as a new column
    columns.push([id]);

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
    return e.target.closest(".facet-close");
  }

  // --- Drag to move (titlebar) ---

  function initDrag(facet) {
    const { id, titleBar } = facet;

    function onStart(clientX, clientY) {
      activeDrag = {
        facetId: id,
        startX: clientX,
        startY: clientY,
        committed: false,
        lastSwapTime: 0,
        moveTriggered: false,
      };
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

    // Remove from column structure; drop empty columns
    for (let c = columns.length - 1; c >= 0; c--) {
      const idx = columns[c].indexOf(id);
      if (idx !== -1) {
        columns[c].splice(idx, 1);
        if (columns[c].length === 0) columns.splice(c, 1);
        break;
      }
    }

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

  function has(id) {
    return facets.has(id);
  }

  function getTiledFacets() {
    return getAllTiledIds().map(id => facets.get(id)).filter(Boolean);
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
    has,
    getTiledFacets,
  };
}
