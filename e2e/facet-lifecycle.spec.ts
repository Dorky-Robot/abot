import { test, expect, type Page } from '@playwright/test';

// Helper: list server sessions via REST API.
async function listSessions(page: Page): Promise<{ name: string; status: string }[]> {
  const resp = await page.request.get('/sessions');
  const body = await resp.json();
  return body.sessions ?? body ?? [];
}

async function sessionNames(page: Page): Promise<string[]> {
  return (await listSessions(page)).map(s => s.name);
}

// Helper: wait for Flutter WASM app to finish loading.
async function waitForApp(page: Page) {
  await page.goto('/');
  // Flutter WASM injects a <flutter-view> element when ready.
  await page.locator('flutter-view').waitFor({ timeout: 15_000 });
  // Give xterm.js time to initialize inside the platform view.
  await page.waitForTimeout(2000);
}

// Helper: count xterm containers in the DOM.
async function xtermCount(page: Page): Promise<number> {
  return page.locator('.xterm-container').count();
}

// Helper: create a session via API and return its name.
async function createSessionViaAPI(page: Page, name?: string): Promise<string> {
  const sessionName = name ?? `e2e-${Date.now()}`;
  await page.request.post('/sessions', { data: { name: sessionName } });
  return sessionName;
}

// macOS uses Meta, others use Control.
const mod = process.platform === 'darwin' ? 'Meta' : 'Control';

test.describe('Facet lifecycle — minimize & close', () => {
  test.beforeEach(async ({ page }) => {
    await waitForApp(page);
  });

  test('app loads with at least one session and xterm', async ({ page }) => {
    const sessions = await sessionNames(page);
    expect(sessions.length).toBeGreaterThanOrEqual(1);

    const count = await xtermCount(page);
    expect(count).toBeGreaterThanOrEqual(1);
  });

  test('Ctrl+N creates a new session', async ({ page }) => {
    const before = await sessionNames(page);

    // Ctrl+N / Cmd+N — new session shortcut.
    await page.keyboard.press(`${mod}+n`);
    await page.waitForTimeout(2000);

    const after = await sessionNames(page);
    expect(after.length).toBe(before.length + 1);

    // A new xterm container should appear.
    const count = await xtermCount(page);
    expect(count).toBeGreaterThanOrEqual(2);
  });

  test('Ctrl+W minimizes (detaches) the focused facet', async ({ page }) => {
    // Ensure we have 2+ sessions so Ctrl+W is active.
    await page.keyboard.press(`${mod}+n`);
    await page.waitForTimeout(2000);

    const before = await sessionNames(page);
    expect(before.length).toBeGreaterThanOrEqual(2);
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
    // Clean up to exactly 1 session so we're testing the single-facet guard.
    const sessions = await listSessions(page);
    for (const s of sessions.slice(1)) {
      await page.request.delete(`/sessions/${s.name}`);
    }
    // Reload so the app picks up the clean state.
    await waitForApp(page);

    const namesBefore = await sessionNames(page);
    expect(namesBefore.length).toBe(1);

    // With 1 facet, Ctrl+W should be a no-op (can't minimize last facet).
    await page.keyboard.press(`${mod}+w`);
    await page.waitForTimeout(500);

    const namesAfter = await sessionNames(page);
    expect(namesAfter.length).toBe(1);
    expect(namesAfter).toEqual(namesBefore);
  });
});

test.describe('Sidebar preview transforms', () => {
  test.beforeEach(async ({ page }) => {
    await waitForApp(page);
  });

  test('xterm containers have valid CSS transforms with 2+ facets', async ({ page }) => {
    // Ensure 2+ sessions so sidebar previews are active.
    const sessions = await sessionNames(page);
    if (sessions.length < 2) {
      await page.keyboard.press(`${mod}+n`);
      await page.waitForTimeout(2000);
    }

    // Wait for transforms to be applied.
    await page.waitForTimeout(500);

    // Read CSS transform and clip-path from all xterm containers.
    const styles = await page.locator('.xterm-container').evaluateAll(els =>
      els.map(el => ({
        transform: el.style.transform,
        clipPath: el.style.clipPath,
        pointerEvents: el.style.pointerEvents,
      }))
    );

    // At least one container should have a CSS transform (sidebar preview).
    const transformed = styles.filter(s => s.transform && s.transform !== 'none' && s.transform !== '');
    expect(transformed.length).toBeGreaterThanOrEqual(1);
  });

  test('clip-path inset values are all non-negative', async ({ page }) => {
    // Ensure 2+ sessions.
    await page.keyboard.press(`${mod}+n`);
    await page.waitForTimeout(2000);

    const clipPaths = await page.locator('.xterm-container').evaluateAll(els =>
      els
        .map(el => el.style.clipPath)
        .filter(cp => cp && cp.startsWith('inset('))
    );

    expect(clipPaths.length).toBeGreaterThanOrEqual(1);

    for (const cp of clipPaths) {
      // Parse inset values — extract all numbers before "round" keyword.
      // CSS inset() uses margin shorthand: 1-4 values (T, T R, T R B, T R B L).
      const insetBody = cp.replace(/^inset\(/, '').replace(/\)$/, '');
      const beforeRound = insetBody.split(/\s+round\s+/)[0];
      const raw = beforeRound.match(/-?[\d.]+/g)?.map(Number) ?? [];
      if (raw.length === 0) continue;

      // Expand CSS shorthand to [top, right, bottom, left].
      let top: number, right: number, bottom: number, left: number;
      if (raw.length === 1) {
        [top, right, bottom, left] = [raw[0], raw[0], raw[0], raw[0]];
      } else if (raw.length === 2) {
        [top, right, bottom, left] = [raw[0], raw[1], raw[0], raw[1]];
      } else if (raw.length === 3) {
        [top, right, bottom, left] = [raw[0], raw[1], raw[2], raw[1]];
      } else {
        [top, right, bottom, left] = raw;
      }

      expect(top).toBeGreaterThanOrEqual(0);
      expect(right).toBeGreaterThanOrEqual(0);
      expect(bottom).toBeGreaterThanOrEqual(0);
      expect(left).toBeGreaterThanOrEqual(0);
    }
  });

  test('sidebar preview stays within viewport bounds', async ({ page }) => {
    // Ensure 2+ sessions.
    const sessions = await sessionNames(page);
    if (sessions.length < 2) {
      await page.keyboard.press(`${mod}+n`);
      await page.waitForTimeout(2000);
    }
    await page.waitForTimeout(500);

    const viewportSize = page.viewportSize()!;

    // Get bounding rects of transformed xterm containers.
    const rects = await page.locator('.xterm-container').evaluateAll(els =>
      els
        .filter(el => el.style.transform && el.style.transform.includes('translate'))
        .map(el => {
          const r = el.getBoundingClientRect();
          return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
        })
    );

    for (const r of rects) {
      // Transformed containers should be positioned somewhere reasonable.
      // They can be offscreen (translate(-9999px)) when sidebar is collapsed,
      // but when visible they should overlap with the sidebar region (left 200px).
      if (r.left > -1000) {
        // Visible preview — should be within viewport width.
        expect(r.left).toBeGreaterThanOrEqual(-10); // small tolerance
        expect(r.left).toBeLessThan(viewportSize.width);
      }
    }
  });

  test('clip-path inset values remain valid on small viewport', async ({ page }) => {
    // Resize to a short viewport to stress-test the bottomClip math.
    await page.setViewportSize({ width: 1280, height: 400 });
    await page.waitForTimeout(500);

    // Ensure 2+ sessions.
    const sessions = await sessionNames(page);
    if (sessions.length < 2) {
      await page.keyboard.press(`${mod}+n`);
      await page.waitForTimeout(2000);
    }
    await page.waitForTimeout(500);

    const clipPaths = await page.locator('.xterm-container').evaluateAll(els =>
      els
        .map(el => el.style.clipPath)
        .filter(cp => cp && cp.startsWith('inset('))
    );

    for (const cp of clipPaths) {
      const insetBody = cp.replace(/^inset\(/, '').replace(/\)$/, '');
      const beforeRound = insetBody.split(/\s+round\s+/)[0];
      const raw = beforeRound.match(/-?[\d.]+/g)?.map(Number) ?? [];
      if (raw.length === 0) continue;

      let top: number, right: number, bottom: number, left: number;
      if (raw.length === 1) {
        [top, right, bottom, left] = [raw[0], raw[0], raw[0], raw[0]];
      } else if (raw.length === 2) {
        [top, right, bottom, left] = [raw[0], raw[1], raw[0], raw[1]];
      } else if (raw.length === 3) {
        [top, right, bottom, left] = [raw[0], raw[1], raw[2], raw[1]];
      } else {
        [top, right, bottom, left] = raw;
      }

      expect(top).toBeGreaterThanOrEqual(0);
      expect(right).toBeGreaterThanOrEqual(0);
      expect(bottom).toBeGreaterThanOrEqual(0);
      expect(left).toBeGreaterThanOrEqual(0);
    }

    // Restore viewport.
    await page.setViewportSize({ width: 1280, height: 800 });
  });
});

test.describe('Session API contract', () => {
  test('GET /sessions returns session list', async ({ page }) => {
    const resp = await page.request.get('/sessions');
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    const sessions = body.sessions ?? body;
    expect(Array.isArray(sessions)).toBeTruthy();
  });

  test('POST /sessions creates a new session', async ({ page }) => {
    const name = `e2e-test-${Date.now()}`;
    const resp = await page.request.post('/sessions', { data: { name } });
    expect(resp.ok()).toBeTruthy();

    const sessions = await sessionNames(page);
    expect(sessions).toContain(name);

    // Clean up.
    await page.request.delete(`/sessions/${name}`);
  });

  test('DELETE /sessions/:name removes the session', async ({ page }) => {
    const name = `e2e-delete-${Date.now()}`;
    await page.request.post('/sessions', { data: { name } });

    const resp = await page.request.delete(`/sessions/${name}`);
    expect(resp.ok()).toBeTruthy();

    const sessions = await sessionNames(page);
    expect(sessions).not.toContain(name);
  });

  test('GET /sessions/:name returns individual session', async ({ page }) => {
    const name = `e2e-get-${Date.now()}`;
    await page.request.post('/sessions', { data: { name } });

    const resp = await page.request.get(`/sessions/${name}`);
    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body.name).toBe(name);

    // Clean up.
    await page.request.delete(`/sessions/${name}`);
  });
});
