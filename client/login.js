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

    // Check if user was redirected after session revocation
    const urlParams = new URLSearchParams(window.location.search);
    if (urlParams.get('reason') === 'revoked') {
      if (loginError) {
        loginError.innerHTML = '<i class="ph ph-info"></i> Your access was revoked. Please register a new passkey to continue.';
        loginError.style.color = '#6b9bd1';
        loginError.style.textAlign = 'center';
        loginError.style.marginBottom = '1rem';
      }
      window.history.replaceState({}, document.title, window.location.pathname);
    }

    // --- Shared registration flow ---

    async function performRegistration(token, errorEl) {
      const supportCheck = checkWebAuthnSupport();
      if (!supportCheck.supported) {
        errorEl.textContent = supportCheck.error;
        return false;
      }

      const optsRes = await fetch("/auth/register/options", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ setupToken: token }),
      });
      if (!optsRes.ok) {
        const err = await optsRes.json();
        throw new Error(err.error || "Failed to get registration options");
      }
      const optsData = await optsRes.json();

      const credential = await startRegistration({ optionsJSON: optsData.options });

      const deviceId = await getOrCreateDeviceId();
      const deviceName = generateDeviceName();

      const verifyRes = await fetch("/auth/register/verify", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          credential,
          userId: optsData.userId,
          challengeId: optsData.challengeId,
          setupToken: token,
          deviceId,
          deviceName,
          userAgent: navigator.userAgent
        }),
      });
      if (!verifyRes.ok) {
        const err = await verifyRes.json();
        throw new Error(err.error || "Registration failed");
      }

      localStorage.setItem('abot_current_credential', credential.id);
      window.location.href = "/";
      return true;
    }

    // --- Status check ---

    async function checkStatus() {
      const res = await fetch("/auth/status");
      const { setup } = await res.json();
      loadingView.classList.add("hidden");
      if (setup) {
        if (hasWebAuthn) {
          loginView.classList.remove("hidden");
          await checkForExistingPasskeys();
        } else {
          pairView.classList.remove("hidden");
        }
      } else {
        setupView.classList.remove("hidden");
        if (!hasWebAuthn) {
          const check = checkWebAuthnSupport();
          setupError.textContent = check.error;
          document.getElementById("register-btn").disabled = true;
        }
      }
    }

    async function checkForExistingPasskeys() {
      try {
        const optsRes = await fetch("/auth/login/options", { method: "POST" });
        if (optsRes.ok) {
          const optsData = await optsRes.json();

          if (!optsData.options?.allowCredentials || optsData.options.allowCredentials.length === 0) {
            const loginBtn = document.getElementById("login-btn");
            if (loginBtn) loginBtn.style.display = 'none';

            const showRegisterBtn = document.getElementById("show-register-btn");
            const registerFields = document.getElementById("register-fields");
            if (showRegisterBtn && registerFields) {
              showRegisterBtn.style.display = 'none';
              registerFields.classList.remove('hidden');
            }

            loginError.innerHTML = '<i class="ph ph-info"></i> No passkey registered yet. Please register your fingerprint/Touch ID below.';
            loginError.style.color = '#6b9bd1';
            loginError.style.textAlign = 'center';
            loginError.style.marginBottom = '1rem';
          }
        }
      } catch {
        // Silently fail - user can still try to login
      }
    }

    // --- First-time registration ---

    document.getElementById("register-btn").addEventListener("click", async () => {
      const btn = document.getElementById("register-btn");
      const token = document.getElementById("setup-token").value.trim();
      setupError.textContent = "";

      const isLocalhost = window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1' || window.location.hostname === '::1';
      if (!token && !isLocalhost) {
        setupError.textContent = "Setup token is required for remote registration.";
        return;
      }

      btn.disabled = true;
      try {
        await performRegistration(token, setupError);
      } catch (err) {
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

      if (!token) { loginError.textContent = "Setup token is required."; return; }

      btn.disabled = true;
      try {
        await performRegistration(token, loginError);
      } catch (err) {
        loginError.textContent = getWebAuthnErrorMessage(err);
      } finally {
        btn.disabled = false;
      }
    });

    // --- Login ---

    document.getElementById("login-btn").addEventListener("click", async () => {
      const btn = document.getElementById("login-btn");
      loginError.textContent = "";
      loginError.style.color = '';

      const supportCheck = checkWebAuthnSupport();
      if (!supportCheck.supported) {
        loginError.textContent = supportCheck.error;
        return;
      }

      btn.disabled = true;

      try {
        const optsRes = await fetch("/auth/login/options", { method: "POST" });
        if (!optsRes.ok) {
          const err = await optsRes.json();
          throw new Error(err.error || "Failed to get login options");
        }
        const optsData = await optsRes.json();

        if (!optsData.options?.allowCredentials || optsData.options.allowCredentials.length === 0) {
          loginError.innerHTML = 'No passkeys registered for this device. Please click <strong>"Register New Passkey"</strong> below to set one up.';
          return;
        }

        const credential = await startAuthentication({ optionsJSON: optsData.options });

        const verifyRes = await fetch("/auth/login/verify", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ credential, challengeId: optsData.challengeId }),
        });
        if (!verifyRes.ok) {
          const err = await verifyRes.json();
          throw new Error(err.error || "Login failed");
        }

        localStorage.setItem('abot_current_credential', credential.id);
        window.location.href = "/";
      } catch (err) {
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
