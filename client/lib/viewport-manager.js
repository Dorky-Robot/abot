/**
 * Viewport Manager
 *
 * Handles viewport resizing, scroll button UI, and terminal gesture handlers.
 */

import { isAtBottom } from "/lib/scroll-utils.js";

/**
 * Create viewport manager for responsive terminal layout
 */
export function createViewportManager(options = {}) {
  const {
    getFocusedTerm,
    facetLayer,
    bar,
    onDictationOpen
  } = options;

  // Scroll button elements
  const scrollBtn = document.getElementById("scroll-bottom");

  // Resize viewport to match visual viewport (handles mobile keyboard)
  function resizeToViewport() {
    const vv = window.visualViewport;
    // In Chromium mobile emulation (isMobile: true), vv.height can be 0 during
    // initial JS module execution before the visual viewport is fully initialised.
    // Fall back to window.innerHeight so the terminal container gets a valid height.
    const h = (vv && vv.height > 0) ? vv.height : window.innerHeight;
    const top = vv ? vv.offsetTop : 0;
    const barHeight = bar ? bar.offsetHeight : 44;
    bar.style.top = top + "px";
    // Adjust facet layer to fit within visual viewport (critical for mobile keyboard)
    facetLayer.style.top = (top + barHeight) + "px";
    facetLayer.style.height = (h - barHeight) + "px";
    facetLayer.style.bottom = "auto";
    const s = document.documentElement.style;
    s.setProperty("--viewport-h", h + "px");
    s.setProperty("--viewport-top", top + "px");
  }

  // Initialize viewport resize handlers
  function initViewportResize() {
    resizeToViewport();
    if (window.visualViewport) {
      window.visualViewport.addEventListener("resize", resizeToViewport);
      window.visualViewport.addEventListener("scroll", resizeToViewport);
    }
    window.addEventListener("resize", resizeToViewport);
    // Re-run after page load in case the visual viewport height was 0 during
    // the initial module execution (mobile Chromium emulation timing issue).
    window.addEventListener("load", resizeToViewport);
  }

  // Initialize scroll-to-bottom button
  function initScrollButton() {
    if (!scrollBtn) return;

    // Listen for scroll events on the facet layer (captures bubble from any viewport)
    let scrollRaf = 0;
    facetLayer.addEventListener("scroll", () => {
      if (!scrollRaf) {
        scrollRaf = requestAnimationFrame(() => {
          scrollRaf = 0;
          const focusedTerm = getFocusedTerm ? getFocusedTerm() : null;
          if (!focusedTerm) return;
          scrollBtn.style.display = isAtBottom(focusedTerm) ? "none" : "flex";
        });
      }
    }, { passive: true, capture: true });

    scrollBtn.addEventListener("click", () => {
      const focusedTerm = getFocusedTerm ? getFocusedTerm() : null;
      if (focusedTerm) focusedTerm.scrollToBottom();
      scrollBtn.style.display = "none";
    });
  }

  // Initialize terminal gesture handlers
  function initTerminalGestures() {
    let longPressTimer = null;
    let touchStartPos = null;
    const LONG_PRESS_DURATION = 500; // ms
    const MOVE_THRESHOLD = 10; // px

    // Focus terminal on tap
    facetLayer.addEventListener("touchstart", (e) => {
      const focusedTerm = getFocusedTerm ? getFocusedTerm() : null;
      if (focusedTerm) focusedTerm.focus();

      // Don't start long-press timer on titlebar (drag-to-reorder uses it)
      if (e.target.closest('.facet-titlebar')) return;

      // Start long-press timer
      touchStartPos = { x: e.touches[0].clientX, y: e.touches[0].clientY };
      longPressTimer = setTimeout(() => {
        if (onDictationOpen) {
          onDictationOpen();
          // Prevent contextmenu from also firing
          longPressTimer = null;
        }
      }, LONG_PRESS_DURATION);
    }, { passive: true });

    facetLayer.addEventListener("touchmove", (e) => {
      // Cancel long-press if finger moves too much
      if (touchStartPos && longPressTimer) {
        const dx = Math.abs(e.touches[0].clientX - touchStartPos.x);
        const dy = Math.abs(e.touches[0].clientY - touchStartPos.y);
        if (dx > MOVE_THRESHOLD || dy > MOVE_THRESHOLD) {
          clearTimeout(longPressTimer);
          longPressTimer = null;
        }
      }
    }, { passive: true });

    facetLayer.addEventListener("touchend", () => {
      // Cancel long-press on touch end
      if (longPressTimer) {
        clearTimeout(longPressTimer);
        longPressTimer = null;
      }
      touchStartPos = null;
    }, { passive: true });

    facetLayer.addEventListener("touchcancel", () => {
      // Cancel long-press on touch cancel
      if (longPressTimer) {
        clearTimeout(longPressTimer);
        longPressTimer = null;
      }
      touchStartPos = null;
    }, { passive: true });

    // Long-press: native contextmenu event (fired by OS on long-press)
    // Keep this as fallback for desktop/non-touch devices
    facetLayer.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      // Only trigger if not already handled by touch events
      if (!longPressTimer && onDictationOpen) {
        onDictationOpen();
      }
    });
  }

  // Initialize all viewport features
  function init() {
    initViewportResize();
    initScrollButton();
    initTerminalGestures();
  }

  return {
    init,
    resizeToViewport,
    initViewportResize,
    initScrollButton,
    initTerminalGestures
  };
}
