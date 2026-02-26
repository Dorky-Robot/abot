import { api } from './lib/api-client.js';
import { getOrCreateDeviceId, generateDeviceName } from './lib/device.js';
import { checkWebAuthnSupport, getWebAuthnErrorMessage } from './lib/webauthn-errors.js';

const $ = (id) => document.getElementById(id);

function showState(id) {
  for (const el of document.querySelectorAll('.card > div')) {
    el.classList.add('hidden');
  }
  $(id).classList.remove('hidden');
}

function showError(msg) {
  const el = $('login-error');
  el.textContent = msg;
  el.classList.remove('hidden');
}

async function init() {
  try {
    const status = await api.get('/auth/status');

    // Already authenticated — go straight to terminal
    if (status.authenticated) {
      window.location.replace('/');
      return;
    }

    // No passkeys set up yet
    if (!status.setup) {
      showState('state-setup');
      return;
    }

    // Passkeys exist — check if we can use WebAuthn here
    const support = checkWebAuthnSupport();
    if (!support.supported) {
      showState('state-insecure');
      return;
    }

    // Ready to login
    showState('state-login');
  } catch {
    showState('state-error');
  }
}

async function login() {
  const btn = $('btn-login');
  btn.disabled = true;
  btn.textContent = 'Authenticating...';
  $('login-error').classList.add('hidden');

  try {
    // 1. Get challenge from server
    const { options, challengeId } = await api.post('/auth/login/options');

    // 2. Ask browser for passkey assertion
    const credential = await navigator.credentials.get({ publicKey: options.publicKey });

    // 3. Send assertion to server
    const deviceId = await getOrCreateDeviceId();
    const deviceName = generateDeviceName();

    const result = await api.post('/auth/login/verify', {
      credential: {
        id: credential.id,
        rawId: arrayBufferToBase64url(credential.rawId),
        type: credential.type,
        response: {
          authenticatorData: arrayBufferToBase64url(credential.response.authenticatorData),
          clientDataJSON: arrayBufferToBase64url(credential.response.clientDataJSON),
          signature: arrayBufferToBase64url(credential.response.signature),
          userHandle: credential.response.userHandle
            ? arrayBufferToBase64url(credential.response.userHandle)
            : null,
        },
      },
      challengeId,
      deviceId,
      deviceName,
    });

    if (result.success) {
      window.location.replace('/');
    }
  } catch (err) {
    showError(getWebAuthnErrorMessage(err));
    btn.disabled = false;
    btn.textContent = 'Sign in with Passkey';
  }
}

function arrayBufferToBase64url(buffer) {
  const bytes = new Uint8Array(buffer);
  let binary = '';
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

$('btn-login').addEventListener('click', login);
init();
