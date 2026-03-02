/**
 * WebAuthn Error Handling Module
 *
 * Provides utilities for checking WebAuthn support and generating
 * user-friendly error messages for WebAuthn operations.
 */

/**
 * Checks if WebAuthn is supported in the current browser environment
 * @returns {{ supported: boolean, error?: string }} Support status and error message if not supported
 */
export function checkWebAuthnSupport() {
  if (!window.isSecureContext) {
    return {
      supported: false,
      error: "Passkeys require a secure connection (HTTPS). You're accessing this page over plain HTTP.\n\nTo fix this, expose abot over HTTPS using a reverse proxy — Cloudflare Tunnel (free) or ngrok work well for self-hosted setups. Alternatively, access via localhost or an SSH tunnel."
    };
  }

  if (!window.PublicKeyCredential) {
    return {
      supported: false,
      error: "Passkeys are not supported in this browser. Please use a modern browser (Chrome, Safari, Firefox, Edge)."
    };
  }

  return { supported: true };
}

/**
 * Converts WebAuthn error objects into user-friendly error messages
 * @param {Error} err - The error object from a WebAuthn operation
 * @returns {string} User-friendly error message
 */
export function getWebAuthnErrorMessage(err) {
  if (err.name === "NotAllowedError") {
    // Check if we're in incognito mode (heuristic)
    const isLikelyIncognito = !navigator.storage || !navigator.storage.estimate;
    if (isLikelyIncognito) {
      return "Passkey registration cancelled. Note: Private/Incognito mode may not support biometric authentication. Please use a regular browser window.";
    }
    return "Passkey registration cancelled. Please try again and approve the biometric prompt.";
  }

  if (err.name === "InvalidStateError") {
    return "This passkey is already registered. Please use a different authenticator.";
  }

  if (err.name === "NotSupportedError") {
    return "Passkey not supported on this device. Please try a different browser or device.";
  }

  if (err.name === "AbortError") {
    return "Registration timed out. Please try again.";
  }

  // Generic error
  return err.message || "An error occurred during passkey registration.";
}
