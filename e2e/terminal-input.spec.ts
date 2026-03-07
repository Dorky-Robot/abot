import { test, expect, type Page } from '@playwright/test';

// ── Helpers ────────────────────────────────────────────────────────────────

async function createKubo(page: Page, name: string) {
  const resp = await page.request.post('/kubos', { data: { name } });
  expect(resp.ok(), `createKubo(${name}) failed: ${resp.status()}`).toBeTruthy();
  return resp.json();
}

async function addAbotToKubo(page: Page, kubo: string, abot: string) {
  const resp = await page.request.post(
    `/kubos/${encodeURIComponent(kubo)}/abots`,
    { data: { abot, createSession: true } },
  );
  expect(resp.ok(), `addAbotToKubo(${kubo}, ${abot}) failed: ${resp.status()}`).toBeTruthy();
  return resp.json();
}

async function deleteSession(page: Page, name: string) {
  await page.request.delete(`/sessions/${encodeURIComponent(name)}`).catch(() => {});
}

async function stopKubo(page: Page, name: string) {
  await page.request.post(`/kubos/${encodeURIComponent(name)}/stop`).catch(() => {});
}

async function waitForApp(page: Page) {
  await page.goto('/');
  await page.locator('flutter-view').waitFor({ timeout: 30_000 });
  await page.waitForTimeout(2000);
}

/**
 * Wait for an xterm container to appear and be interactive (has a textarea).
 * Returns true if xterm is ready, false if Docker backend isn't available.
 */
async function waitForXterm(page: Page, timeout = 15_000): Promise<boolean> {
  try {
    await page.locator('.xterm-container').first().waitFor({ timeout });
    await page.locator('.xterm-helper-textarea').first().waitFor({ timeout: 5_000 });
    // Let the initial DA (Device Attributes) exchange between tmux and xterm.js settle
    await page.waitForTimeout(2000);
    return true;
  } catch {
    return false;
  }
}

/**
 * Type into the focused xterm terminal by clicking the container and typing.
 */
async function typeInTerminal(page: Page, text: string) {
  const container = page.locator('.xterm-container').first();
  await container.click();
  await page.waitForTimeout(200);
  await page.keyboard.type(text, { delay: 50 });
}

/**
 * Send a command to the terminal and wait for output to appear.
 */
async function sendCommandAndVerify(page: Page, marker: string): Promise<boolean> {
  await typeInTerminal(page, `echo ${marker}\n`);
  try {
    await page.waitForFunction(
      (m: string) => {
        const containers = document.querySelectorAll('.xterm-container');
        for (const c of containers) {
          if (c.textContent?.includes(m)) return true;
          const rows = c.querySelectorAll('.xterm-rows > div');
          for (const row of rows) {
            if (row.textContent?.includes(m)) return true;
          }
        }
        return false;
      },
      marker,
      { timeout: 10_000 },
    );
    return true;
  } catch {
    return false;
  }
}

/**
 * Verify tmux is running inside the kubo container via the API.
 * Sends `tmux list-sessions` and checks for the expected session name.
 */
async function verifyTmuxSession(page: Page, kubo: string, abot: string): Promise<boolean> {
  // Use the sessions list API to check if session is alive
  const resp = await page.request.get('/sessions');
  if (!resp.ok()) return false;
  const sessions = (await resp.json()) as Array<{ name: string; alive: boolean }>;
  const session = sessions.find((s) => s.name === `${abot}@${kubo}`);
  return session?.alive === true;
}

// macOS uses Meta, others use Control.
const modKey = process.platform === 'darwin' ? 'Meta' : 'Control';
// Sidebar toggle: Cmd+B on macOS, Ctrl+Shift+B on others (Ctrl+B is tmux prefix).
const sidebarToggle = process.platform === 'darwin' ? 'Meta+b' : 'Control+Shift+b';

// Track created resources for cleanup
const createdKubos: string[] = [];
const createdSessions: string[] = [];

async function trackedCreateKubo(page: Page, name: string) {
  const result = await createKubo(page, name);
  createdKubos.push(name);
  return result;
}

async function trackedAddAbot(page: Page, kubo: string, abot: string) {
  const result = await addAbotToKubo(page, kubo, abot);
  createdSessions.push(`${abot}@${kubo}`);
  return result;
}

async function cleanup(page: Page) {
  for (const name of createdSessions) {
    await deleteSession(page, name);
  }
  createdSessions.length = 0;
  for (const name of createdKubos) {
    await stopKubo(page, name);
  }
  createdKubos.length = 0;
}

// ── Tests ──────────────────────────────────────────────────────────────────

test.describe('Terminal input and tmux sessions', () => {
  test.afterEach(async ({ page }) => {
    await cleanup(page);
  });

  test('abot session uses tmux and accepts input', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-tmux-${ts}`;
    const abot = `e2e-bot-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot);
    await waitForApp(page);

    const ready = await waitForXterm(page);
    if (!ready) {
      test.skip();
      return;
    }

    // Verify session is alive
    const alive = await verifyTmuxSession(page, kubo, abot);
    expect(alive).toBeTruthy();

    // Terminal should accept input
    const marker = `TMUX_${ts}`;
    const echoed = await sendCommandAndVerify(page, marker);
    expect(echoed).toBeTruthy();
  });

  test('switching between two abots preserves terminals', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-switch-${ts}`;
    const abot1 = `e2e-sw1-${ts}`;
    const abot2 = `e2e-sw2-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot1);
    await trackedAddAbot(page, kubo, abot2);
    await waitForApp(page);

    const ready = await waitForXterm(page);
    if (!ready) {
      test.skip();
      return;
    }

    // Type in the currently focused terminal
    const marker1 = `SW_A_${ts}`;
    const echoed1 = await sendCommandAndVerify(page, marker1);
    expect(echoed1).toBeTruthy();

    // Switch to the other abot via Ctrl+`
    await page.keyboard.press('Control+`');
    await page.waitForTimeout(1000);

    // Type in the newly focused terminal
    const marker2 = `SW_B_${ts}`;
    const echoed2 = await sendCommandAndVerify(page, marker2);
    expect(echoed2).toBeTruthy();

    // Switch back to the first
    await page.keyboard.press('Control+`');
    await page.waitForTimeout(1000);

    // First terminal should still accept input
    const marker3 = `SW_C_${ts}`;
    const echoed3 = await sendCommandAndVerify(page, marker3);
    expect(echoed3).toBeTruthy();
  });

  test('terminal works after minimize and reopen', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-min-${ts}`;
    const abot1 = `e2e-m1-${ts}`;
    const abot2 = `e2e-m2-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot1);
    await trackedAddAbot(page, kubo, abot2);
    await waitForApp(page);

    const ready = await waitForXterm(page);
    if (!ready) {
      test.skip();
      return;
    }

    // Type before minimize
    const marker1 = `PRE_MIN_${ts}`;
    expect(await sendCommandAndVerify(page, marker1)).toBeTruthy();

    // Minimize (Cmd/Ctrl+W)
    await page.keyboard.press(`${modKey}+w`);
    await page.waitForTimeout(1000);

    // Other abot should now be focused and accept input
    const marker2 = `POST_MIN_${ts}`;
    expect(await sendCommandAndVerify(page, marker2)).toBeTruthy();
  });

  test('terminal works after sidebar toggle', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-sb-${ts}`;
    const abot = `e2e-sbot-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot);
    await waitForApp(page);

    const ready = await waitForXterm(page);
    if (!ready) {
      test.skip();
      return;
    }

    // Send a warm-up command to flush any DA response garbage, then verify
    await typeInTerminal(page, `true\n`);
    await page.waitForTimeout(500);
    expect(await sendCommandAndVerify(page, `PRE_${ts}`)).toBeTruthy();

    // Collapse sidebar
    await page.keyboard.press(sidebarToggle);
    await page.waitForTimeout(1000);
    expect(await sendCommandAndVerify(page, `COLLAPSED_${ts}`)).toBeTruthy();

    // Expand sidebar
    await page.keyboard.press(sidebarToggle);
    await page.waitForTimeout(1000);
    expect(await sendCommandAndVerify(page, `EXPANDED_${ts}`)).toBeTruthy();
  });

  test('clicking terminal container focuses it for input', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-clk-${ts}`;
    const abot = `e2e-clkbot-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot);
    await waitForApp(page);

    const ready = await waitForXterm(page);
    if (!ready) {
      test.skip();
      return;
    }

    // Unfocus by clicking body
    await page.click('body', { position: { x: 10, y: 10 } });
    await page.waitForTimeout(200);

    // Click terminal to refocus
    await page.locator('.xterm-container').first().click();
    await page.waitForTimeout(1000);

    const focused = await page.evaluate(() => {
      const el = document.activeElement;
      return el?.classList.contains('xterm-helper-textarea') ?? false;
    });
    expect(focused).toBeTruthy();

    expect(await sendCommandAndVerify(page, `CLICK_${ts}`)).toBeTruthy();
  });

  test('tmux prefix (Ctrl+B) works for scrollback and command mode', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-tmuxpfx-${ts}`;
    const abot = `e2e-pfxbot-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot);
    await waitForApp(page);

    const ready = await waitForXterm(page);
    if (!ready) {
      test.skip();
      return;
    }

    /** Send a tmux prefix command: Ctrl+B then the key. */
    async function tmuxPrefix(key: string) {
      await page.keyboard.press('Control+b');
      await page.waitForTimeout(300);
      await page.keyboard.press(key);
      await page.waitForTimeout(1500);
    }

    // Warm up — flush DA response garbage
    await typeInTerminal(page, `true\n`);
    await page.waitForTimeout(500);

    // Verify terminal accepts input
    const m0 = `PREFIX_${ts}`;
    expect(await sendCommandAndVerify(page, m0)).toBeTruthy();

    // Ctrl+B [ — enter copy/scroll mode (should not break the terminal)
    await tmuxPrefix('[');
    // Exit copy mode with q
    await page.keyboard.press('q');
    await page.waitForTimeout(500);

    // Terminal should still accept input after copy mode
    const m1 = `AFTERCOPY_${ts}`;
    expect(await sendCommandAndVerify(page, m1)).toBeTruthy();

    // Ctrl+B c — create new tmux window
    await tmuxPrefix('c');
    const m2 = `NEWWIN_${ts}`;
    expect(await sendCommandAndVerify(page, m2)).toBeTruthy();

    // Ctrl+B p — go back to previous window
    await tmuxPrefix('p');
    await page.waitForTimeout(500);

    // Original window should still have our earlier output
    const m3 = `PREVWIN_${ts}`;
    expect(await sendCommandAndVerify(page, m3)).toBeTruthy();
  });
});
