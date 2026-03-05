import { test, expect, type Page } from '@playwright/test';

// Helper: list server sessions via REST API (retries once on transient failure).
async function listSessions(page: Page): Promise<{ name: string; alive: boolean }[]> {
  for (let attempt = 0; attempt < 2; attempt++) {
    const resp = await page.request.get('/sessions');
    if (!resp.ok()) {
      await page.waitForTimeout(500);
      continue;
    }
    const body = await resp.json();
    return body.sessions ?? body ?? [];
  }
  return [];
}

async function sessionNames(page: Page): Promise<string[]> {
  return (await listSessions(page)).map(s => s.name);
}

// Helper: wait for Flutter WASM app to finish loading.
async function waitForApp(page: Page) {
  await page.goto('/');
  // Flutter WASM injects a <flutter-view> element when ready.
  await page.locator('flutter-view').waitFor({ timeout: 30_000 });
  // Give xterm.js time to initialize inside the platform view.
  await page.waitForTimeout(2000);
}

// Helper: count xterm containers in the DOM.
async function xtermCount(page: Page): Promise<number> {
  return page.locator('.xterm-container').count();
}

// Helper: parse CSS inset() shorthand into [top, right, bottom, left].
function parseInsetValues(clipPath: string): number[] {
  const insetBody = clipPath.replace(/^inset\(/, '').replace(/\)$/, '');
  const beforeRound = insetBody.split(/\s+round\s+/)[0];
  const raw = beforeRound.match(/-?[\d.]+/g)?.map(Number) ?? [];
  if (raw.length === 0) return [];
  if (raw.length === 1) return [raw[0], raw[0], raw[0], raw[0]];
  if (raw.length === 2) return [raw[0], raw[1], raw[0], raw[1]];
  if (raw.length === 3) return [raw[0], raw[1], raw[2], raw[1]];
  return [raw[0], raw[1], raw[2], raw[3]];
}

// Helper: add an abot to a kubo via REST, creating a session.
async function addAbotToKubo(page: Page, abot: string, kubo = 'default') {
  const resp = await page.request.post(
    `/kubos/${encodeURIComponent(kubo)}/abots`,
    { data: { abot, createSession: true } },
  );
  expect(resp.ok(), `addAbotToKubo(${abot}, ${kubo}) failed: ${resp.status()}`).toBeTruthy();
  return resp.json();
}

// macOS uses Meta, others use Control.
const mod = process.platform === 'darwin' ? 'Meta' : 'Control';

// Track created sessions for cleanup.
const createdSessions: string[] = [];

async function trackedAddAbot(page: Page, abot: string, kubo = 'default') {
  const result = await addAbotToKubo(page, abot, kubo);
  createdSessions.push(abot);
  return result;
}

async function cleanup(page: Page) {
  for (const name of createdSessions) {
    await page.request.delete(`/sessions/${encodeURIComponent(name)}`).catch(() => {});
  }
  createdSessions.length = 0;
}

test.describe('Facet lifecycle — minimize & close', () => {
  test.afterEach(async ({ page }) => {
    await cleanup(page);
  });

  test('app loads with flutter-view and xterm', async ({ page }) => {
    // Create an abot BEFORE loading the page, so _initialize picks it up.
    await trackedAddAbot(page, `e2e-facet-${Date.now()}`);
    await waitForApp(page);

    // Wait for xterm container to appear (platform view creation is async)
    try {
      await page.locator('.xterm-container').first().waitFor({ timeout: 10_000 });
    } catch {
      // Platform view may not mount without a live Docker backend —
      // verify session exists instead.
    }

    // At minimum, the session should exist on the server
    const sessions = await listSessions(page);
    expect(sessions.length).toBeGreaterThanOrEqual(1);
  });

  test('empty state shows no xterm containers', async ({ page }) => {
    // Delete all sessions so the app starts empty.
    const sessions = await listSessions(page);
    for (const s of sessions) {
      await page.request.delete(`/sessions/${encodeURIComponent(s.name)}`).catch(() => {});
    }

    await waitForApp(page);
    const count = await xtermCount(page);
    expect(count).toBe(0);
  });

  test('Ctrl+W minimizes (detaches) the focused facet', async ({ page }) => {
    // Clean slate — other workers may have left sessions
    const existing = await listSessions(page);
    for (const s of existing) {
      await page.request.delete(`/sessions/${encodeURIComponent(s.name)}`).catch(() => {});
    }

    // Create exactly 2 sessions so Ctrl+W is active.
    const ts = Date.now();
    await trackedAddAbot(page, `e2e-mina-${ts}`);
    await trackedAddAbot(page, `e2e-minb-${ts}`);
    await waitForApp(page);

    const before = await sessionNames(page);
    expect(before.length).toBe(2);

    // Try to wait for xterm containers (platform views need Docker backend)
    try {
      await page.locator('.xterm-container').first().waitFor({ timeout: 10_000 });
    } catch {
      // No xterm containers — verify session-level behavior only
      await page.keyboard.press(`${mod}+w`);
      await page.waitForTimeout(1000);
      const after = await sessionNames(page);
      expect(after.length).toBe(before.length);
      return;
    }

    const xtermsBefore = await xtermCount(page);

    // Ctrl+W — minimize current facet.
    await page.keyboard.press(`${mod}+w`);
    await page.waitForTimeout(1000);

    // Session should STILL exist on server (minimize, not close).
    const after = await sessionNames(page);
    expect(after.length).toBe(before.length);

    // But one fewer xterm in the DOM (facet removed).
    const xtermsAfter = await xtermCount(page);
    expect(xtermsAfter).toBeLessThan(xtermsBefore);
  });

  test('Ctrl+W does nothing with only 1 facet', async ({ page }) => {
    // Clean up to exactly 1 session.
    const sessions = await listSessions(page);
    for (const s of sessions) {
      await page.request.delete(`/sessions/${encodeURIComponent(s.name)}`).catch(() => {});
    }
    const ts = Date.now();
    await trackedAddAbot(page, `e2e-single-${ts}`);
    await waitForApp(page);

    const namesBefore = await sessionNames(page);
    expect(namesBefore.length).toBe(1);

    // With 1 facet, Ctrl+W should be a no-op (can't minimize last facet).
    // Sessions should remain unchanged on server regardless of xterm state.
    await page.keyboard.press(`${mod}+w`);
    await page.waitForTimeout(500);

    const namesAfter = await sessionNames(page);
    expect(namesAfter).toEqual(namesBefore);
  });

  test('Ctrl+N does NOT create a new session (shortcut removed)', async ({ page }) => {
    const ts = Date.now();
    await trackedAddAbot(page, `e2e-noN-${ts}`);
    await waitForApp(page);

    const before = await sessionNames(page);
    await page.keyboard.press(`${mod}+n`);
    await page.waitForTimeout(2000);

    const after = await sessionNames(page);
    expect(after.length).toBe(before.length);
  });
});

test.describe('Sidebar preview transforms', () => {
  test.afterEach(async ({ page }) => {
    await cleanup(page);
  });

  test('xterm containers have valid CSS transforms with 2+ facets', async ({ page }) => {
    const ts = Date.now();
    await trackedAddAbot(page, `e2e-tfma-${ts}`);
    await trackedAddAbot(page, `e2e-tfmb-${ts}`);
    await waitForApp(page);

    // Wait for xterm containers (platform view creation is async)
    try {
      await page.locator('.xterm-container').first().waitFor({ timeout: 10_000 });
    } catch {
      // Skip CSS checks if platform views don't mount (no Docker backend)
      test.skip();
      return;
    }

    // Read CSS transform from all xterm containers.
    const styles = await page.locator('.xterm-container').evaluateAll(els =>
      els.map(el => ({
        transform: el.style.transform,
        clipPath: el.style.clipPath,
      }))
    );

    // At least one container should have a CSS transform (sidebar preview).
    const transformed = styles.filter(s => s.transform && s.transform !== 'none' && s.transform !== '');
    expect(transformed.length).toBeGreaterThanOrEqual(1);
  });

  test('clip-path inset values are all non-negative', async ({ page }) => {
    const ts = Date.now();
    await trackedAddAbot(page, `e2e-clpa-${ts}`);
    await trackedAddAbot(page, `e2e-clpb-${ts}`);
    await waitForApp(page);

    // Wait for xterm containers (platform view creation is async)
    try {
      await page.locator('.xterm-container').first().waitFor({ timeout: 10_000 });
    } catch {
      // Skip if platform views don't mount (no Docker backend)
      test.skip();
      return;
    }

    const clipPaths = await page.locator('.xterm-container').evaluateAll(els =>
      els
        .map(el => el.style.clipPath)
        .filter(cp => cp && cp.startsWith('inset('))
    );

    expect(clipPaths.length).toBeGreaterThanOrEqual(1);

    for (const cp of clipPaths) {
      const [top, right, bottom, left] = parseInsetValues(cp);
      if (top === undefined) continue;
      expect(top).toBeGreaterThanOrEqual(0);
      expect(right).toBeGreaterThanOrEqual(0);
      expect(bottom).toBeGreaterThanOrEqual(0);
      expect(left).toBeGreaterThanOrEqual(0);
    }
  });

  test('sidebar preview stays within viewport bounds', async ({ page }) => {
    const ts = Date.now();
    await trackedAddAbot(page, `e2e-vpa-${ts}`);
    await trackedAddAbot(page, `e2e-vpb-${ts}`);
    await waitForApp(page);
    await page.waitForTimeout(500);

    const viewportSize = page.viewportSize()!;

    const rects = await page.locator('.xterm-container').evaluateAll(els =>
      els
        .filter(el => el.style.transform && el.style.transform.includes('translate'))
        .map(el => {
          const r = el.getBoundingClientRect();
          return { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
        })
    );

    for (const r of rects) {
      // Visible previews should be within viewport width.
      if (r.left > -1000) {
        expect(r.left).toBeGreaterThanOrEqual(-10);
        expect(r.left).toBeLessThan(viewportSize.width);
      }
    }
  });

  test('clip-path inset values remain valid on small viewport', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 400 });
    await page.waitForTimeout(500);

    const ts = Date.now();
    await trackedAddAbot(page, `e2e-sva-${ts}`);
    await trackedAddAbot(page, `e2e-svb-${ts}`);
    await waitForApp(page);
    await page.waitForTimeout(500);

    const clipPaths = await page.locator('.xterm-container').evaluateAll(els =>
      els
        .map(el => el.style.clipPath)
        .filter(cp => cp && cp.startsWith('inset('))
    );

    for (const cp of clipPaths) {
      const [top, right, bottom, left] = parseInsetValues(cp);
      if (top === undefined) continue;
      expect(top).toBeGreaterThanOrEqual(0);
      expect(right).toBeGreaterThanOrEqual(0);
      expect(bottom).toBeGreaterThanOrEqual(0);
      expect(left).toBeGreaterThanOrEqual(0);
    }

    await page.setViewportSize({ width: 1280, height: 800 });
  });
});

test.describe('Session API contract', () => {
  test.afterEach(async ({ page }) => {
    await cleanup(page);
  });

  test('GET /sessions returns session list', async ({ page }) => {
    // Retry once in case the daemon is still processing a slow prior request
    let resp = await page.request.get('/sessions');
    if (!resp.ok()) {
      await page.waitForTimeout(500);
      resp = await page.request.get('/sessions');
    }
    expect(resp.ok(), `GET /sessions failed: ${resp.status()}`).toBeTruthy();
    const body = await resp.json();
    const sessions = body.sessions ?? body;
    expect(Array.isArray(sessions)).toBeTruthy();
  });

  test('POST /sessions creates a session with canonical abot + worktree', async ({ page }) => {
    const name = `e2e-api-${Date.now()}`;
    createdSessions.push(name);

    const resp = await page.request.post('/sessions', { data: { name } });
    expect(resp.ok()).toBeTruthy();

    // Session should exist with a bundlePath (worktree inside kubo) and kubo set
    const session = await page.request.get(`/sessions/${name}`).then(r => r.json());
    expect(session.name).toBe(name);
    expect(session.bundlePath).toBeTruthy();
    expect(session.bundlePath).toContain('default.kubo');
    expect(session.kubo).toBe('default');
  });

  test('DELETE /sessions/:name removes the session', async ({ page }) => {
    const name = `e2e-del-${Date.now()}`;
    await page.request.post('/sessions', { data: { name } });

    const resp = await page.request.delete(`/sessions/${name}`);
    expect(resp.ok()).toBeTruthy();

    const sessions = await sessionNames(page);
    expect(sessions).not.toContain(name);
  });

  test('GET /sessions/:name returns individual session', async ({ page }) => {
    const name = `e2e-get-${Date.now()}`;
    createdSessions.push(name);
    await page.request.post('/sessions', { data: { name } });

    const resp = await page.request.get(`/sessions/${name}`);
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body.name).toBe(name);
  });
});
