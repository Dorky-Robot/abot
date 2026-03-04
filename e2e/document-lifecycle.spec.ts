import { test, expect, type Page } from '@playwright/test';
import * as path from 'path';
import * as os from 'os';
import * as fs from 'fs';

// Helper: list server sessions via REST API.
async function listSessions(page: Page): Promise<{ name: string; alive: boolean; bundlePath?: string; dirty?: boolean }[]> {
  const resp = await page.request.get('/sessions');
  const body = await resp.json();
  return body.sessions ?? body ?? [];
}

async function sessionNames(page: Page): Promise<string[]> {
  return (await listSessions(page)).map(s => s.name);
}

// Helper: get a single session by name.
async function getSession(page: Page, name: string) {
  const resp = await page.request.get(`/sessions/${encodeURIComponent(name)}`);
  expect(resp.ok()).toBeTruthy();
  return resp.json();
}

// Helper: create a unique temp dir for test bundles.
function tempBundleDir(testName: string): string {
  const dir = path.join(os.tmpdir(), `abot-e2e-${testName}-${Date.now()}`);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

test.describe('Document lifecycle — save/open/close API', () => {
  let testSession: string;

  test.beforeEach(async ({ page }) => {
    // Create a fresh session for each test.
    // Sessions now get a canonical abot + worktree automatically.
    testSession = `doc-test-${Date.now()}`;
    const resp = await page.request.post('/sessions', {
      data: { name: testSession },
    });
    expect(resp.ok()).toBeTruthy();
  });

  test.afterEach(async ({ page }) => {
    try {
      await page.request.delete(`/sessions/${encodeURIComponent(testSession)}`);
    } catch {}
  });

  test('new session has bundlePath (worktree in kubo) and is not dirty', async ({ page }) => {
    const session = await getSession(page, testSession);
    // Sessions now always have a bundlePath — the worktree inside the kubo
    expect(session.bundlePath).toBeTruthy();
    expect(session.bundlePath).toContain('default.kubo');
    expect(session.kubo).toBe('default');
    expect(session.dirty).toBe(false);
  });

  test('POST save-as creates bundle on disk and returns path', async ({ page }) => {
    const dir = tempBundleDir('save-as');
    const bundlePath = path.join(dir, `${testSession}.abot`);

    const resp = await page.request.post(
      `/sessions/${encodeURIComponent(testSession)}/save-as`,
      { data: { path: bundlePath } },
    );
    expect(resp.ok()).toBeTruthy();

    const body = await resp.json();
    expect(body.session).toBe(testSession);
    expect(body.path).toBe(bundlePath);

    // Verify files were created on disk.
    expect(fs.existsSync(path.join(bundlePath, 'manifest.json'))).toBeTruthy();
    expect(fs.existsSync(path.join(bundlePath, 'credentials.json'))).toBeTruthy();
    expect(fs.existsSync(path.join(bundlePath, 'config.json'))).toBeTruthy();

    // Verify manifest content.
    const manifest = JSON.parse(fs.readFileSync(path.join(bundlePath, 'manifest.json'), 'utf-8'));
    expect(manifest.version).toBe(2);
    expect(manifest.name).toBe(testSession);
    expect(manifest.updated_at).toBeDefined();

    fs.rmSync(dir, { recursive: true, force: true });
  });

  test('save-as updates session bundlePath', async ({ page }) => {
    const dir = tempBundleDir('save-as-path');
    const bundlePath = path.join(dir, `${testSession}.abot`);

    await page.request.post(
      `/sessions/${encodeURIComponent(testSession)}/save-as`,
      { data: { path: bundlePath } },
    );

    const session = await getSession(page, testSession);
    expect(session.bundlePath).toBe(bundlePath);
    expect(session.dirty).toBe(false);

    fs.rmSync(dir, { recursive: true, force: true });
  });

  test('POST save works on session with worktree bundlePath', async ({ page }) => {
    // Sessions now have a bundlePath from creation (the worktree).
    // Regular save should work immediately.
    const resp = await page.request.post(
      `/sessions/${encodeURIComponent(testSession)}/save`,
    );
    expect(resp.ok()).toBeTruthy();

    const body = await resp.json();
    expect(body.session).toBe(testSession);
    expect(body.path).toBeTruthy();
  });

  test('POST close removes the session', async ({ page }) => {
    const resp = await page.request.post(
      `/sessions/${encodeURIComponent(testSession)}/close`,
      { data: { save: false } },
    );
    expect(resp.ok()).toBeTruthy();

    const sessions = await sessionNames(page);
    expect(sessions).not.toContain(testSession);
  });

  test('POST close with save:true saves before closing', async ({ page }) => {
    // Close with save — the worktree bundlePath should persist.
    const session = await getSession(page, testSession);
    const bundlePath = session.bundlePath;
    expect(bundlePath).toBeTruthy();

    const resp = await page.request.post(
      `/sessions/${encodeURIComponent(testSession)}/close`,
      { data: { save: true } },
    );
    expect(resp.ok()).toBeTruthy();

    // Session should be gone.
    const sessions = await sessionNames(page);
    expect(sessions).not.toContain(testSession);

    // But the bundle should still exist on disk.
    expect(fs.existsSync(path.join(bundlePath, 'manifest.json'))).toBeTruthy();
  });

  test('POST open restores session from bundle', async ({ page }) => {
    const dir = tempBundleDir('open');
    const bundlePath = path.join(dir, `${testSession}.abot`);

    // Save the session to a custom path.
    await page.request.post(
      `/sessions/${encodeURIComponent(testSession)}/save-as`,
      { data: { path: bundlePath } },
    );

    // Close the session.
    await page.request.post(`/sessions/${encodeURIComponent(testSession)}/close`, {
      data: { save: false },
    });

    // Verify it's gone.
    let sessions = await sessionNames(page);
    expect(sessions).not.toContain(testSession);

    // Open the bundle.
    const resp = await page.request.post('/sessions/open', {
      data: { path: bundlePath },
    });
    expect(resp.ok()).toBeTruthy();

    const body = await resp.json();
    expect(body.name).toBe(testSession);

    // Session should be back.
    sessions = await sessionNames(page);
    expect(sessions).toContain(testSession);

    // The reopened session should point to the worktree (not the original bundle)
    const session = await getSession(page, testSession);
    expect(session.bundlePath).toBeTruthy();
    expect(session.bundlePath).toContain('default.kubo');

    fs.rmSync(dir, { recursive: true, force: true });
  });

  test('open on nonexistent path returns error', async ({ page }) => {
    const resp = await page.request.post('/sessions/open', {
      data: { path: '/tmp/abot-does-not-exist-xyz.abot' },
    });
    expect(resp.status()).toBe(400);
  });

  test('save-as preserves created_at on re-save', async ({ page }) => {
    const dir = tempBundleDir('preserve-created');
    const bundlePath = path.join(dir, `${testSession}.abot`);

    // First save.
    await page.request.post(
      `/sessions/${encodeURIComponent(testSession)}/save-as`,
      { data: { path: bundlePath } },
    );

    const manifest1 = JSON.parse(
      fs.readFileSync(path.join(bundlePath, 'manifest.json'), 'utf-8'),
    );
    const createdAt = manifest1.created_at;

    // Wait so timestamps differ.
    await page.waitForTimeout(100);

    // Save again (regular save).
    await page.request.post(
      `/sessions/${encodeURIComponent(testSession)}/save`,
    );

    const manifest2 = JSON.parse(
      fs.readFileSync(path.join(bundlePath, 'manifest.json'), 'utf-8'),
    );
    expect(manifest2.created_at).toBe(createdAt);
    expect(manifest2.updated_at).toBeDefined();

    fs.rmSync(dir, { recursive: true, force: true });
  });
});
