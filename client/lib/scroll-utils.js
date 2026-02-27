/**
 * Scroll State Utilities
 *
 * Composable scroll management helpers for terminal.
 * Uses xterm's buffer API (not DOM viewport queries) for scroll detection.
 */

/**
 * Check if terminal is scrolled to the bottom.
 * Uses xterm buffer API: viewportY >= baseY means at bottom.
 */
export function isAtBottom(term) {
  if (!term?.buffer?.active) return true;
  const buf = term.buffer.active;
  return buf.viewportY >= buf.baseY;
}

/**
 * Scroll to bottom with double RAF for layout settling
 */
export const scrollToBottom = (term) => {
  requestAnimationFrame(() => {
    requestAnimationFrame(() => term.scrollToBottom());
  });
};

/**
 * Preserve scroll position during operation (composable)
 */
export const withPreservedScroll = (term, operation) => {
  const wasAtBottom = isAtBottom(term);
  operation();
  if (wasAtBottom) scrollToBottom(term);
};

/**
 * Terminal write with preserved scroll (composable)
 */
export const terminalWriteWithScroll = (term, data, onComplete) => {
  const wasAtBottom = isAtBottom(term);
  term.write(data, () => {
    if (wasAtBottom) scrollToBottom(term);
    if (onComplete) onComplete();
  });
};
