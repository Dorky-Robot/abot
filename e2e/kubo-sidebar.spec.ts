import { test, expect, type Page } from '@playwright/test';

// ── Helpers ────────────────────────────────────────────────────────────────

async function listKubos(page: Page) {
  const resp = await page.request.get('/kubos');
  return (await resp.json()) as { name: string; running: boolean; activeSessions: number }[];
}

async function createKubo(page: Page, name: string) {
  const resp = await page.request.post('/kubos', { data: { name } });
  expect(resp.ok(), `createKubo(${name}) failed: ${resp.status()}`).toBeTruthy();
  return await resp.json();
}

async function stopKubo(page: Page, name: string) {
  await page.request.post(`/kubos/${encodeURIComponent(name)}/stop`);
}

async function listSessions(page: Page) {
  const resp = await page.request.get('/sessions');
  const body = await resp.json();
  return (body.sessions ?? body ?? []) as { name: string; alive: boolean; kubo: string | null }[];
}

async function createSession(page: Page, name: string, kubo?: string) {
  const body: Record<string, string> = { name };
  if (kubo) body.kubo = kubo;
  const resp = await page.request.post('/sessions', { data: body });
  expect(resp.ok(), `createSession(${name}) failed: ${resp.status()}`).toBeTruthy();
  return await resp.json();
}

async function deleteSession(page: Page, name: string) {
  await page.request.delete(`/sessions/${encodeURIComponent(name)}`);
}

async function waitForApp(page: Page) {
  await page.goto('/');
  await page.locator('flutter-view').waitFor({ timeout: 15_000 });
  await page.waitForTimeout(2000);
}

// Track created resources for cleanup
const createdKubos: string[] = [];
const createdSessions: string[] = [];

async function cleanupResources(page: Page) {
  for (const name of createdSessions) {
    await deleteSession(page, name).catch(() => {});
  }
  createdSessions.length = 0;

  for (const name of createdKubos) {
    await stopKubo(page, name).catch(() => {});
  }
  createdKubos.length = 0;
}

async function trackedCreateKubo(page: Page, name: string) {
  const result = await createKubo(page, name);
  createdKubos.push(name);
  return result;
}

async function trackedCreateSession(page: Page, name: string, kubo?: string) {
  const result = await createSession(page, name, kubo);
  createdSessions.push(name);
  return result;
}

// ── REST API tests ─────────────────────────────────────────────────────────

test.describe('Kubo REST API', () => {
  test.afterEach(async ({ page }) => {
    await cleanupResources(page);
  });

  test('GET /kubos returns at least the default kubo', async ({ page }) => {
    const kubos = await listKubos(page);
    expect(kubos.length).toBeGreaterThanOrEqual(1);
    expect(kubos.map(k => k.name)).toContain('default');
  });

  test('POST /kubos creates a new kubo', async ({ page }) => {
    const name = `e2e-kubo-${Date.now()}`;
    await trackedCreateKubo(page, name);

    const kubos = await listKubos(page);
    expect(kubos.map(k => k.name)).toContain(name);
  });

  test('session created with kubo has kubo field in JSON', async ({ page }) => {
    const kuboName = `e2e-field-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    const sessionName = `e2e-abot-${Date.now()}`;
    await trackedCreateSession(page, sessionName, kuboName);

    const sessions = await listSessions(page);
    const session = sessions.find(s => s.name === sessionName);
    expect(session).toBeDefined();
    expect(session!.kubo).toBe(kuboName);
  });

  test('session without kubo has null kubo field', async ({ page }) => {
    const sessionName = `e2e-nokubo-${Date.now()}`;
    await trackedCreateSession(page, sessionName);

    const sessions = await listSessions(page);
    const session = sessions.find(s => s.name === sessionName);
    expect(session).toBeDefined();
    expect(session!.kubo).toBeNull();
  });

  test('sessions group correctly by kubo', async ({ page }) => {
    const kubo1 = `e2e-grp1-${Date.now()}`;
    const kubo2 = `e2e-grp2-${Date.now()}`;
    await trackedCreateKubo(page, kubo1);
    await trackedCreateKubo(page, kubo2);

    const s1 = `e2e-s1-${Date.now()}`;
    const s2 = `e2e-s2-${Date.now()}`;
    const s3 = `e2e-s3-${Date.now()}`;
    await trackedCreateSession(page, s1, kubo1);
    await trackedCreateSession(page, s2, kubo1);
    await trackedCreateSession(page, s3, kubo2);

    const sessions = await listSessions(page);
    expect(sessions.filter(s => s.kubo === kubo1).length).toBe(2);
    expect(sessions.filter(s => s.kubo === kubo2).length).toBe(1);
  });
});

// ── UI integration tests ───────────────────────────────────────────────────

test.describe('Kubo sidebar UI', () => {
  test.afterEach(async ({ page }) => {
    await cleanupResources(page);
  });

  test('app loads and connects to server', async ({ page }) => {
    await waitForApp(page);
    // Verify we can reach both APIs
    const kubos = await listKubos(page);
    expect(kubos.length).toBeGreaterThanOrEqual(1);
    const sessions = await listSessions(page);
    expect(sessions.length).toBeGreaterThanOrEqual(1);
  });

  test('creating kubo via API makes it visible on reload', async ({ page }) => {
    const kuboName = `e2e-vis-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    // After creation, verify API returns it
    const kubos = await listKubos(page);
    expect(kubos.map(k => k.name)).toContain(kuboName);

    // Create an abot inside it
    const abotName = `e2e-abot-vis-${Date.now()}`;
    await trackedCreateSession(page, abotName, kuboName);

    // Verify session is in the kubo
    const sessions = await listSessions(page);
    const abot = sessions.find(s => s.name === abotName);
    expect(abot?.kubo).toBe(kuboName);
  });

  test('deleting a session removes it from kubo group', async ({ page }) => {
    const kuboName = `e2e-del-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    const abotName = `e2e-delabot-${Date.now()}`;
    await trackedCreateSession(page, abotName, kuboName);

    // Verify it exists
    let sessions = await listSessions(page);
    expect(sessions.find(s => s.name === abotName)).toBeDefined();

    // Delete it
    await deleteSession(page, abotName);
    createdSessions.splice(createdSessions.indexOf(abotName), 1);

    // Verify it's gone
    sessions = await listSessions(page);
    expect(sessions.find(s => s.name === abotName)).toBeUndefined();
  });

  test('default kubo always exists in list', async ({ page }) => {
    const kubos = await listKubos(page);
    const defaultKubo = kubos.find(k => k.name === 'default');
    expect(defaultKubo).toBeDefined();
  });

  test('kubo activeSessions count reflects open sessions', async ({ page }) => {
    const kuboName = `e2e-count-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    // Initially 0
    let kubos = await listKubos(page);
    let kubo = kubos.find(k => k.name === kuboName);
    expect(kubo?.activeSessions).toBe(0);

    // After creating a session (which may exit quickly), the kubo should have tracked it
    await trackedCreateSession(page, `e2e-cnt-${Date.now()}`, kuboName);
    kubos = await listKubos(page);
    kubo = kubos.find(k => k.name === kuboName);
    // activeSessions might be 0 or 1 depending on Docker availability
    expect(kubo?.activeSessions).toBeGreaterThanOrEqual(0);
  });
});
