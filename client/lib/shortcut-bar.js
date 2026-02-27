/**
 * Shortcut Bar Renderer
 *
 * Global bar with facet tabs, P2P indicator, pinned shortcuts, and action buttons.
 * [+ New] [tab1] [tab2] ... [spacer] [Esc] [Tab] [keyboard] [settings]
 */

import { keysToSequence, sendSequence } from "/lib/key-mapping.js";

/**
 * Create shortcut bar renderer
 */
export function createShortcutBar(options = {}) {
  const {
    container,
    pinnedKeys = [
      { label: "Esc", keys: "esc" },
      { label: "Tab", keys: "tab" }
    ],
    onShortcutsClick,
    onSettingsClick,
    onNewFacet,
    onFocusFacet,
    sendFn,
    getFocusedTerm,
    updateP2PIndicator,
    getInstanceIcon
  } = options;

  /**
   * Render the shortcut bar
   * @param {{ facets: Array, focusedId: string|null }} state
   */
  function render(state = {}) {
    if (!container) return;

    const { facets = [], focusedId = null } = state;

    container.innerHTML = "";

    // P2P indicator
    const p2pDot = document.createElement("span");
    p2pDot.id = "p2p-indicator";
    p2pDot.title = "P2P: connecting...";
    container.appendChild(p2pDot);

    // Update P2P indicator if callback provided
    if (updateP2PIndicator) updateP2PIndicator();

    // [+ New] button
    if (onNewFacet) {
      const newBtn = document.createElement("button");
      newBtn.className = "facet-new-btn";
      newBtn.tabIndex = -1;
      newBtn.setAttribute("aria-label", "New facet");
      newBtn.innerHTML = '<i class="ph ph-plus"></i> New';
      newBtn.addEventListener("click", onNewFacet);
      container.appendChild(newBtn);
    }

    // Facet tabs
    const instanceIcon = getInstanceIcon ? getInstanceIcon() : "terminal-window";
    for (const facet of facets) {
      const tab = document.createElement("button");
      tab.className = "facet-tab";
      if (facet.id === focusedId) {
        tab.classList.add("facet-tab-active");
      }
      tab.tabIndex = -1;
      tab.setAttribute("aria-label", `Focus ${facet.sessionName}`);

      const iconEl = document.createElement("i");
      iconEl.className = `ph ph-${instanceIcon}`;
      tab.appendChild(iconEl);

      const nameSpan = document.createElement("span");
      nameSpan.textContent = facet.sessionName;
      nameSpan.style.overflow = "hidden";
      nameSpan.style.textOverflow = "ellipsis";
      tab.appendChild(nameSpan);

      tab.addEventListener("click", () => {
        if (onFocusFacet) onFocusFacet(facet.id);
      });
      container.appendChild(tab);
    }

    // Spacer
    const spacer = document.createElement("span");
    spacer.className = "bar-spacer";
    container.appendChild(spacer);

    // Pinned shortcut buttons
    for (const s of pinnedKeys) {
      const btn = document.createElement("button");
      btn.className = "shortcut-btn";
      btn.tabIndex = -1;
      btn.textContent = s.label;
      btn.setAttribute("aria-label", `Send ${s.label}`);
      btn.addEventListener("click", () => {
        if (sendFn) {
          sendSequence(keysToSequence(s.keys), sendFn);
        }
        const term = getFocusedTerm ? getFocusedTerm() : null;
        if (term) term.focus();
      });
      container.appendChild(btn);
    }

    // Shortcuts button
    const kbBtn = document.createElement("button");
    kbBtn.className = "bar-icon-btn";
    kbBtn.tabIndex = -1;
    kbBtn.setAttribute("aria-label", "Open shortcuts");
    kbBtn.innerHTML = '<i class="ph ph-keyboard"></i>';
    if (onShortcutsClick) {
      kbBtn.addEventListener("click", onShortcutsClick);
    }
    container.appendChild(kbBtn);

    // Settings button
    const setBtn = document.createElement("button");
    setBtn.className = "bar-icon-btn";
    setBtn.tabIndex = -1;
    setBtn.setAttribute("aria-label", "Settings");
    setBtn.innerHTML = '<i class="ph ph-gear"></i>';
    if (onSettingsClick) {
      setBtn.addEventListener("click", onSettingsClick);
    }
    container.appendChild(setBtn);
  }

  return {
    render
  };
}
