    import {
      startRegistration,
      startAuthentication,
    } from "/vendor/simplewebauthn/browser.esm.js";
    import { getOrCreateDeviceId, generateDeviceName } from "/lib/device.js";
    import { checkWebAuthnSupport, getWebAuthnErrorMessage } from "/lib/webauthn-errors.js";

    const setupView = document.getElementById("setup-view");
    const loginView = document.getElementById("login-view");
    const pairView = document.getElementById("pair-view");
    const loadingView = document.getElementById("loading-view");
    const setupError = document.getElementById("setup-error");
    const loginError = document.getElementById("login-error");

    const hasWebAuthn = window.isSecureContext && !!window.PublicKeyCredential;
    const isMobile = /Android|iPad|iPhone|iPod/.test(navigator.userAgent);

    // Check if user was redirected after session revocation
    const urlParams = new URLSearchParams(window.location.search);
    if (urlParams.get('reason') === 'revoked') {
      // Show info message about session being revoked
      if (loginError) {
        loginError.innerHTML = '<i class="ph ph-info"></i> Your access was revoked. Please register a new passkey to continue.';
        loginError.style.color = '#6b9bd1'; // Info blue
        loginError.style.textAlign = 'center';
        loginError.style.marginBottom = '1rem';
      }
      // Clean up URL without reload
      window.history.replaceState({}, document.title, window.location.pathname);
    }

    // --- WebAuthn Support Checks ---
    // WebAuthn support and error functions imported from /lib/webauthn-errors.js

    // --- Device ID Management ---
    // Device management functions imported from /lib/device.js

    async function checkStatus() {
      console.log("[auth] checkStatus: fetching /auth/status");
      const res = await fetch("/auth/status");
      const { setup, accessMethod } = await res.json();
      console.log("[auth] checkStatus: result", { setup, accessMethod, hasWebAuthn, isSecureContext: window.isSecureContext, hostname: window.location.hostname, protocol: window.location.protocol });
      loadingView.classList.add("hidden");
      if (setup) {
        // Decide which login flow based on access method
        if (hasWebAuthn) {
          // localhost or internet with WebAuthn → passkey login/registration
          console.log("[auth] checkStatus: showing login view (WebAuthn available)");
          loginView.classList.remove("hidden");

          // Check if user has passkeys for this domain
          await checkForExistingPasskeys();
        } else {
          // HTTP without WebAuthn → show QR pairing instructions
          console.log("[auth] checkStatus: showing pair view (no WebAuthn)", { isSecureContext: window.isSecureContext, hasPublicKeyCredential: !!window.PublicKeyCredential });
          pairView.classList.remove("hidden");
        }
      } else {
        console.log("[auth] checkStatus: not set up, showing setup view");
        setupView.classList.remove("hidden");
      }
    }

    async function checkForExistingPasskeys() {
      try {
        // Try to get login options to see if there are any credentials
        const optsRes = await fetch("/auth/login/options", { method: "POST" });
        if (optsRes.ok) {
          const opts = await optsRes.json();

          // If no credentials available, hide login button and show only register
          if (!opts.allowCredentials || opts.allowCredentials.length === 0) {
            // Hide the login button
            const loginBtn = document.getElementById("login-btn");
            if (loginBtn) {
              loginBtn.style.display = 'none';
            }

            // Hide the "Register New Passkey" button and show fields directly
            const showRegisterBtn = document.getElementById("show-register-btn");
            const registerFields = document.getElementById("register-fields");
            if (showRegisterBtn && registerFields) {
              showRegisterBtn.style.display = 'none';
              registerFields.classList.remove('hidden');
            }

            // Show helpful message
            loginError.innerHTML = '<i class="ph ph-info"></i> No passkey registered yet. Please register your fingerprint/Touch ID below.';
            loginError.style.color = '#6b9bd1'; // Info blue instead of error red
            loginError.style.textAlign = 'center';
            loginError.style.marginBottom = '1rem';
          }
        }
      } catch {
        // Silently fail - user can still try to login and get proper error
      }
    }

    // --- Registration ---

    document.getElementById("register-btn").addEventListener("click", async () => {
      const btn = document.getElementById("register-btn");
      const token = document.getElementById("setup-token").value.trim();
      setupError.textContent = "";

      // Token is optional for first registration from localhost
      const isLocalhost = window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1' || window.location.hostname === '::1';
      console.log("[auth] register (first-time): clicked", { isLocalhost, hasToken: !!token, protocol: window.location.protocol });
      if (!token && !isLocalhost) {
        setupError.textContent = "Setup token is required for remote registration.";
        return;
      }

      // Check WebAuthn support
      const supportCheck = checkWebAuthnSupport();
      console.log("[auth] register: WebAuthn support check", supportCheck);
      if (!supportCheck.supported) {
        setupError.textContent = supportCheck.error;
        return;
      }

      btn.disabled = true;
      try {
        // Get registration options
        console.log("[auth] register: fetching /auth/register/options");
        const optsRes = await fetch("/auth/register/options", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ setupToken: token }),
        });
        console.log("[auth] register: /auth/register/options response", { status: optsRes.status, ok: optsRes.ok });
        if (!optsRes.ok) {
          const err = await optsRes.json();
          console.error("[auth] register: options request failed", err);
          throw new Error(err.error || "Failed to get registration options");
        }
        const opts = await optsRes.json();
        console.log("[auth] register: received options", {
          rpId: opts.rp?.id, rpName: opts.rp?.name,
          hasChallenge: !!opts.challenge,
          authenticatorSelection: opts.authenticatorSelection,
          pubKeyCredParams: opts.pubKeyCredParams?.map(p => p.alg),
          excludeCredentials: opts.excludeCredentials?.length ?? 0,
        });

        // Start WebAuthn registration
        console.log("[auth] register: calling startRegistration (browser WebAuthn prompt)");
        const credential = await startRegistration({ optionsJSON: opts });
        console.log("[auth] register: startRegistration succeeded", {
          credentialId: credential.id,
          type: credential.type,
          hasAttestationObject: !!credential.response?.attestationObject,
          hasClientDataJSON: !!credential.response?.clientDataJSON,
        });

        // Get device metadata
        const deviceId = await getOrCreateDeviceId();
        const deviceName = generateDeviceName();
        const userAgent = navigator.userAgent;
        console.log("[auth] register: device metadata", { deviceId, deviceName });

        // Verify with server
        console.log("[auth] register: sending to /auth/register/verify");
        const verifyRes = await fetch("/auth/register/verify", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            credential,
            setupToken: token,
            deviceId,
            deviceName,
            userAgent
          }),
        });
        console.log("[auth] register: /auth/register/verify response", { status: verifyRes.status, ok: verifyRes.ok });
        if (!verifyRes.ok) {
          const err = await verifyRes.json();
          console.error("[auth] register: verify failed", err);
          throw new Error(err.error || "Registration failed");
        }

        // Store credential ID for "this device" detection
        localStorage.setItem('abot_current_credential', credential.id);

        // Success — redirect
        console.log("[auth] register: success — redirecting to /");
        window.location.href = "/";
      } catch (err) {
        console.error("[auth] register: error", { name: err.name, message: err.message, stack: err.stack });
        setupError.textContent = getWebAuthnErrorMessage(err);
      } finally {
        btn.disabled = false;
      }
    });

    // --- Register new passkey (on already-setup instance) ---

    document.getElementById("show-register-btn").addEventListener("click", () => {
      document.getElementById("register-fields").classList.toggle("hidden");
    });

    document.getElementById("register-new-btn").addEventListener("click", async () => {
      const btn = document.getElementById("register-new-btn");
      const token = document.getElementById("register-token").value.trim();
      loginError.textContent = "";

      console.log("[auth] register-new: clicked", { hasToken: !!token });
      if (!token) { loginError.textContent = "Setup token is required."; return; }

      // Check WebAuthn support
      const supportCheck = checkWebAuthnSupport();
      console.log("[auth] register-new: WebAuthn support check", supportCheck);
      if (!supportCheck.supported) {
        loginError.textContent = supportCheck.error;
        return;
      }

      btn.disabled = true;
      try {
        console.log("[auth] register-new: fetching /auth/register/options");
        const optsRes = await fetch("/auth/register/options", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ setupToken: token }),
        });
        console.log("[auth] register-new: /auth/register/options response", { status: optsRes.status, ok: optsRes.ok });
        if (!optsRes.ok) {
          const err = await optsRes.json();
          console.error("[auth] register-new: options request failed", err);
          throw new Error(err.error || "Failed to get registration options");
        }
        const opts = await optsRes.json();
        console.log("[auth] register-new: received options", {
          rpId: opts.rp?.id, rpName: opts.rp?.name,
          hasChallenge: !!opts.challenge,
          authenticatorSelection: opts.authenticatorSelection,
          excludeCredentials: opts.excludeCredentials?.length ?? 0,
        });

        console.log("[auth] register-new: calling startRegistration (browser WebAuthn prompt)");
        const credential = await startRegistration({ optionsJSON: opts });
        console.log("[auth] register-new: startRegistration succeeded", {
          credentialId: credential.id, type: credential.type,
        });

        // Get device metadata
        const deviceId = await getOrCreateDeviceId();
        const deviceName = generateDeviceName();
        const userAgent = navigator.userAgent;
        console.log("[auth] register-new: device metadata", { deviceId, deviceName });

        console.log("[auth] register-new: sending to /auth/register/verify");
        const verifyRes = await fetch("/auth/register/verify", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            credential,
            setupToken: token,
            deviceId,
            deviceName,
            userAgent
          }),
        });
        console.log("[auth] register-new: /auth/register/verify response", { status: verifyRes.status, ok: verifyRes.ok });
        if (!verifyRes.ok) {
          const err = await verifyRes.json();
          console.error("[auth] register-new: verify failed", err);
          throw new Error(err.error || "Registration failed");
        }

        // Store credential ID for "this device" detection
        localStorage.setItem('abot_current_credential', credential.id);

        console.log("[auth] register-new: success — redirecting to /");
        window.location.href = "/";
      } catch (err) {
        console.error("[auth] register-new: error", { name: err.name, message: err.message, stack: err.stack });
        loginError.textContent = getWebAuthnErrorMessage(err);
      } finally {
        btn.disabled = false;
      }
    });

    // --- Login ---

    document.getElementById("login-btn").addEventListener("click", async () => {
      const btn = document.getElementById("login-btn");
      loginError.textContent = "";
      loginError.style.color = ''; // Reset to default error color

      console.log("[auth] login: clicked");

      // Check WebAuthn support
      const supportCheck = checkWebAuthnSupport();
      console.log("[auth] login: WebAuthn support check", supportCheck);
      if (!supportCheck.supported) {
        loginError.textContent = supportCheck.error;
        return;
      }

      btn.disabled = true;

      try {
        // Get authentication options
        console.log("[auth] login: fetching /auth/login/options");
        const optsRes = await fetch("/auth/login/options", { method: "POST" });
        console.log("[auth] login: /auth/login/options response", { status: optsRes.status, ok: optsRes.ok });
        if (!optsRes.ok) {
          const err = await optsRes.json();
          console.error("[auth] login: options request failed", err);
          throw new Error(err.error || "Failed to get login options");
        }
        const opts = await optsRes.json();
        console.log("[auth] login: received options", {
          rpId: opts.rpId,
          hasChallenge: !!opts.challenge,
          allowCredentials: opts.allowCredentials?.length ?? 0,
          allowCredentialIds: opts.allowCredentials?.map(c => c.id),
          userVerification: opts.userVerification,
        });

        // Check if there are any passkeys available for this domain
        if (!opts.allowCredentials || opts.allowCredentials.length === 0) {
          console.warn("[auth] login: no allowCredentials in options — no passkeys available");
          loginError.innerHTML = 'No passkeys registered for this device. Please click <strong>"Register New Passkey"</strong> below to set one up.';
          return;
        }

        // Start WebAuthn authentication
        console.log("[auth] login: calling startAuthentication (browser WebAuthn prompt)");
        const credential = await startAuthentication({ optionsJSON: opts });
        console.log("[auth] login: startAuthentication succeeded", {
          credentialId: credential.id, type: credential.type,
          hasAuthenticatorData: !!credential.response?.authenticatorData,
          hasSignature: !!credential.response?.signature,
          hasClientDataJSON: !!credential.response?.clientDataJSON,
        });

        // Verify with server
        console.log("[auth] login: sending to /auth/login/verify");
        const verifyRes = await fetch("/auth/login/verify", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ credential }),
        });
        console.log("[auth] login: /auth/login/verify response", { status: verifyRes.status, ok: verifyRes.ok });
        if (!verifyRes.ok) {
          const err = await verifyRes.json();
          console.error("[auth] login: verify failed", err);
          throw new Error(err.error || "Login failed");
        }

        // Store credential ID for "this device" detection
        localStorage.setItem('abot_current_credential', credential.id);

        // Success — redirect
        console.log("[auth] login: success — redirecting to /");
        window.location.href = "/";
      } catch (err) {
        console.error("[auth] login: error", { name: err.name, message: err.message, stack: err.stack });
        // Special handling for "no passkeys available" error
        if (err.name === "NotAllowedError" && err.message?.includes("No available authenticator")) {
          loginError.innerHTML = 'No passkeys found for this device. Please click <strong>"Register New Passkey"</strong> below to set one up.';
        } else {
          loginError.textContent = getWebAuthnErrorMessage(err);
        }
      } finally {
        btn.disabled = false;
      }
    });

    checkStatus();
