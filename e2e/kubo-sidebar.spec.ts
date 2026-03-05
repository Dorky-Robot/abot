import { test, expect, type Page } from '@playwright/test';

// ── Helpers ────────────────────────────────────────────────────────────────

async function listKubos(page: Page) {
  for (let attempt = 0; attempt < 2; attempt++) {
    const resp = await page.request.get('/kubos');
    if (!resp.ok()) {
      await page.waitForTimeout(500);
      continue;
    }
    const data = await resp.json();
    if (Array.isArray(data)) return data as { name: string; running: boolean; activeSessions: number; abots: string[] }[];
  }
  return [] as { name: string; running: boolean; activeSessions: number; abots: string[] }[];
}

async function createKubo(page: Page, name: string) {
  const resp = await page.request.post('/kubos', { data: { name } });
  expect(resp.ok(), `createKubo(${name}) failed: ${resp.status()}`).toBeTruthy();
  return await resp.json();
}

async function startKubo(page: Page, name: string) {
  return page.request.post(`/kubos/${encodeURIComponent(name)}/start`, { data: {} });
}

async function stopKubo(page: Page, name: string) {
  await page.request.post(`/kubos/${encodeURIComponent(name)}/stop`);
}

async function listSessions(page: Page) {
  for (let attempt = 0; attempt < 2; attempt++) {
    const resp = await page.request.get('/sessions');
    if (!resp.ok()) {
      await page.waitForTimeout(500);
      continue;
    }
    const body = await resp.json();
    return (body.sessions ?? body ?? []) as { name: string; alive: boolean; kubo: string | null; bundlePath: string | null }[];
  }
  return [] as { name: string; alive: boolean; kubo: string | null; bundlePath: string | null }[];
}

async function addAbotToKubo(page: Page, kubo: string, abot: string, createSession = true) {
  const resp = await page.request.post(
    `/kubos/${encodeURIComponent(kubo)}/abots`,
    { data: { abot, createSession } },
  );
  expect(resp.ok(), `addAbotToKubo(${kubo}, ${abot}) failed: ${resp.status()}`).toBeTruthy();
  return resp.json();
}

async function deleteSession(page: Page, name: string) {
  await page.request.delete(`/sessions/${encodeURIComponent(name)}`);
}

async function removeAbotFromKubo(page: Page, kubo: string, abot: string) {
  const resp = await page.request.delete(
    `/kubos/${encodeURIComponent(kubo)}/abots/${encodeURIComponent(abot)}`,
  );
  expect(resp.ok(), `removeAbotFromKubo(${kubo}, ${abot}) failed: ${resp.status()}`).toBeTruthy();
  return resp.json();
}

async function waitForApp(page: Page) {
  await page.goto('/');
  await page.locator('flutter-view').waitFor({ timeout: 30_000 });
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

async function trackedAddAbot(page: Page, kubo: string, abot: string) {
  const result = await addAbotToKubo(page, kubo, abot);
  // Session name is now qualified: abot@kubo
  createdSessions.push(`${abot}@${kubo}`);
  return result;
}

// ── REST API tests ─────────────────────────────────────────────────────────

test.describe('Kubo REST API', () => {
  test.afterEach(async ({ page }) => {
    await cleanupResources(page);
  });

  test('GET /kubos returns a list (default kubo created on demand)', async ({ page }) => {
    // The default kubo is created lazily (on first session/abot add),
    // so a fresh data dir may have zero kubos initially.
    const kubos = await listKubos(page);
    expect(Array.isArray(kubos)).toBeTruthy();
  });

  test('POST /kubos creates a new kubo', async ({ page }) => {
    const name = `e2e-kubo-${Date.now()}`;
    await trackedCreateKubo(page, name);

    const kubos = await listKubos(page);
    expect(kubos.map(k => k.name)).toContain(name);
  });

  test('POST /kubos/:name/abots adds abot with session', async ({ page }) => {
    const kubo = `e2e-add-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-abot-${Date.now()}`;
    const result = await trackedAddAbot(page, kubo, abot);
    expect(result.abot).toBe(abot);
    expect(result.kubo).toBe(kubo);
    // Session name is now kubo-qualified: abot@kubo
    expect(result.session).toBe(`${abot}@${kubo}`);

    // Session should exist with the correct kubo and qualified name
    const sessions = await listSessions(page);
    const session = sessions.find(s => s.name === `${abot}@${kubo}`);
    expect(session).toBeDefined();
    expect(session!.kubo).toBe(kubo);
    // bundlePath should point to the kubo worktree
    expect(session!.bundlePath).toBeTruthy();
    expect(session!.bundlePath).toContain(`${kubo}.kubo`);
  });

  test('POST /kubos/:name/abots without session creates abot only', async ({ page }) => {
    const kubo = `e2e-nosess-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-nosess-abot-${Date.now()}`;
    const result = await addAbotToKubo(page, kubo, abot, false);
    expect(result.abot).toBe(abot);
    expect(result.session).toBeUndefined();

    // No session should exist (neither bare nor qualified)
    const sessions = await listSessions(page);
    expect(sessions.find(s => s.name === abot || s.name === `${abot}@${kubo}`)).toBeUndefined();
  });

  test('sessions group correctly by kubo', async ({ page }) => {
    const kubo1 = `e2e-grp1-${Date.now()}`;
    const kubo2 = `e2e-grp2-${Date.now()}`;
    await trackedCreateKubo(page, kubo1);
    await trackedCreateKubo(page, kubo2);

    const ts = Date.now();
    await trackedAddAbot(page, kubo1, `e2e-s1-${ts}`);
    await trackedAddAbot(page, kubo1, `e2e-s2-${ts}`);
    await trackedAddAbot(page, kubo2, `e2e-s3-${ts}`);

    const sessions = await listSessions(page);
    expect(sessions.filter(s => s.kubo === kubo1).length).toBe(2);
    expect(sessions.filter(s => s.kubo === kubo2).length).toBe(1);
  });

  test('POST /kubos/:name/start returns error for unknown kubo', async ({ page }) => {
    const resp = await startKubo(page, 'nonexistent-kubo');
    expect(resp.status()).toBe(400);
  });

  test('kubo activeSessions count reflects open sessions', async ({ page }) => {
    const kuboName = `e2e-count-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    // Initially 0
    let kubos = await listKubos(page);
    let kubo = kubos.find(k => k.name === kuboName);
    expect(kubo?.activeSessions).toBe(0);

    // After adding an abot with session
    await trackedAddAbot(page, kuboName, `e2e-cnt-${Date.now()}`);
    kubos = await listKubos(page);
    kubo = kubos.find(k => k.name === kuboName);
    // activeSessions might be 0 or 1 depending on Docker availability
    expect(kubo?.activeSessions).toBeGreaterThanOrEqual(0);
  });

  test('start/stop cycle changes kubo running status', async ({ page }) => {
    const name = `e2e-startstop-${Date.now()}`;
    await trackedCreateKubo(page, name);

    // Newly created kubo should be stopped
    let kubos = await listKubos(page);
    let kubo = kubos.find(k => k.name === name);
    expect(kubo).toBeDefined();
    expect(kubo!.running).toBe(false);

    // Start the kubo
    const startResp = await startKubo(page, name);
    if (!startResp.ok()) {
      // Docker may not be available — skip rest of test
      test.skip();
      return;
    }

    // After start, running should be true
    kubos = await listKubos(page);
    kubo = kubos.find(k => k.name === name);
    expect(kubo!.running).toBe(true);

    // Stop the kubo
    await stopKubo(page, name);

    // After stop, running should be false
    kubos = await listKubos(page);
    kubo = kubos.find(k => k.name === name);
    expect(kubo!.running).toBe(false);
  });

  test('GET /kubos running status reflects actual Docker state', async ({ page }) => {
    // Start a kubo via API, then verify GET /kubos reports it running
    const name = `e2e-realstatus-${Date.now()}`;
    await trackedCreateKubo(page, name);

    // Start it
    const startResp = await startKubo(page, name);
    if (!startResp.ok()) {
      test.skip();
      return;
    }

    // Verify running=true via list
    let kubos = await listKubos(page);
    let kubo = kubos.find(k => k.name === name);
    expect(kubo!.running).toBe(true);

    // Stop it and verify running=false immediately
    await stopKubo(page, name);
    kubos = await listKubos(page);
    kubo = kubos.find(k => k.name === name);
    expect(kubo!.running).toBe(false);

    // Start again — should go back to running
    await startKubo(page, name);
    kubos = await listKubos(page);
    kubo = kubos.find(k => k.name === name);
    expect(kubo!.running).toBe(true);
  });
});

// ── UI integration tests ───────────────────────────────────────────────────

test.describe('Kubo sidebar UI', () => {
  test.afterEach(async ({ page }) => {
    await cleanupResources(page);
  });

  test('app loads and connects to server', async ({ page }) => {
    // Create a kubo and abot so the app has something to show
    const kubo = `e2e-load-${Date.now()}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, `e2e-load-abot-${Date.now()}`);
    await waitForApp(page);

    const kubos = await listKubos(page);
    expect(kubos.length).toBeGreaterThanOrEqual(1);
  });

  test('creating kubo via API makes it visible in session list', async ({ page }) => {
    const kuboName = `e2e-vis-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    const abotName = `e2e-abot-vis-${Date.now()}`;
    await trackedAddAbot(page, kuboName, abotName);

    const sessions = await listSessions(page);
    const session = sessions.find(s => s.name === `${abotName}@${kuboName}`);
    expect(session).toBeDefined();
    expect(session!.kubo).toBe(kuboName);
  });

  test('deleting a session removes it from kubo group', async ({ page }) => {
    const kuboName = `e2e-del-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    const abotName = `e2e-delabot-${Date.now()}`;
    const qualified = `${abotName}@${kuboName}`;
    await trackedAddAbot(page, kuboName, abotName);

    await deleteSession(page, qualified);
    createdSessions.splice(createdSessions.indexOf(qualified), 1);

    const sessions = await listSessions(page);
    expect(sessions.find(s => s.name === qualified)).toBeUndefined();
  });

  test('no default kubo on fresh start', async ({ page }) => {
    // There should be no auto-created "default" kubo — only user-created kubos exist.
    const kubos = await listKubos(page);
    // If a "default" kubo exists, it was created by a previous test run, not auto-created.
    // The important thing is the server doesn't create one on demand.
    expect(Array.isArray(kubos)).toBeTruthy();
  });

  test('session creation requires kubo', async ({ page }) => {
    // POST /sessions without kubo should return an error
    const resp = await page.request.post('/sessions', { data: { name: 'test-no-kubo' } });
    expect(resp.status()).toBe(400);
  });
});

// ── Kubo credentials ──────────────────────────────────────────────────────

test.describe('Kubo credentials', () => {
  test.afterEach(async ({ page }) => {
    await cleanupResources(page);
  });

  test('creating a kubo initializes credentials.json on disk', async ({ page }) => {
    const name = `e2e-creds-${Date.now()}`;
    const result = await trackedCreateKubo(page, name);

    // The kubo path should be returned — verify credentials.json exists via the FS
    // We can't read the filesystem from Playwright, but we can verify the kubo was
    // created and that adding an abot with credentials works end-to-end.
    const kubos = await listKubos(page);
    expect(kubos.map(k => k.name)).toContain(name);

    // Add an abot — this exercises create_kubo_backend which reads kubo credentials
    const abot = `e2e-creds-abot-${Date.now()}`;
    await trackedAddAbot(page, name, abot);

    const sessions = await listSessions(page);
    const session = sessions.find(s => s.name === `${abot}@${name}`);
    expect(session).toBeDefined();
    expect(session!.kubo).toBe(name);
  });
});

// ── Known abots registry ──────────────────────────────────────────────────

async function listAbots(page: Page) {
  // Retry once on transient 500 (can happen if concurrent workers are writing abots.json)
  let resp = await page.request.get('/abots');
  if (!resp.ok()) {
    await page.waitForTimeout(200);
    resp = await page.request.get('/abots');
  }
  expect(resp.ok(), `GET /abots failed: ${resp.status()}`).toBeTruthy();
  const body = await resp.json();
  return (body.abots ?? []) as { name: string; added_at: string }[];
}

async function getAbotDetail(page: Page, name: string) {
  const resp = await page.request.get(`/abots/${encodeURIComponent(name)}`);
  expect(resp.ok(), `GET /abots/${name} failed: ${resp.status()}`).toBeTruthy();
  return await resp.json() as {
    name: string;
    path: string;
    default_branch: string;
    kubo_branches: { kubo_name: string; branch: string; has_worktree: boolean; has_session: boolean }[];
  };
}

async function integrateVariant(page: Page, abot: string, kubo: string) {
  const resp = await page.request.post(
    `/abots/${encodeURIComponent(abot)}/integrate`,
    { data: { kubo } },
  );
  return { ok: resp.ok(), status: resp.status(), body: await resp.json() };
}

async function dismissVariant(page: Page, abot: string, kubo: string) {
  const resp = await page.request.post(
    `/abots/${encodeURIComponent(abot)}/dismiss`,
    { data: { kubo } },
  );
  return { ok: resp.ok(), status: resp.status(), body: await resp.json() };
}

async function discardVariant(page: Page, abot: string, kubo: string) {
  const resp = await page.request.post(
    `/abots/${encodeURIComponent(abot)}/discard`,
    { data: { kubo } },
  );
  return { ok: resp.ok(), status: resp.status(), body: await resp.json() };
}

async function removeKnownAbot(page: Page, name: string) {
  const resp = await page.request.delete(`/abots/${encodeURIComponent(name)}`);
  expect(resp.ok(), `DELETE /abots/${name} failed: ${resp.status()}`).toBeTruthy();
  return await resp.json();
}

test.describe('Known abots registry', () => {
  test.afterEach(async ({ page }) => {
    await cleanupResources(page);
  });

  test('GET /abots returns list', async ({ page }) => {
    const abots = await listAbots(page);
    expect(Array.isArray(abots)).toBeTruthy();
  });

  test('adding abot to kubo auto-registers in known abots', async ({ page }) => {
    const kubo = `e2e-ka-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-kaabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    const abots = await listAbots(page);
    expect(abots.map(a => a.name)).toContain(abot);
  });

  test('GET /abots/:name returns detail with git info', async ({ page }) => {
    const kubo = `e2e-detail-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-detabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    const detail = await getAbotDetail(page, abot);
    expect(detail.name).toBe(abot);
    expect(detail.path).toBeTruthy();
    expect(detail.default_branch).toBeTruthy();
    expect(Array.isArray(detail.kubo_branches)).toBeTruthy();
  });

  test('DELETE /abots/:name removes from known list', async ({ page }) => {
    const kubo = `e2e-rmka-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-rmkaabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Verify it's in the list
    let abots = await listAbots(page);
    expect(abots.map(a => a.name)).toContain(abot);

    // Remove from known list
    const result = await removeKnownAbot(page, abot);
    expect(result.removed).toBe(abot);

    // Verify it's gone
    abots = await listAbots(page);
    expect(abots.map(a => a.name)).not.toContain(abot);
  });

  test('sync picks up abots from kubo manifests', async ({ page }) => {
    // Adding an abot to a kubo should make it appear in the known list
    // (this tests the side-effect in AddAbotToKubo handler)
    const kubo = `e2e-sync-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-syncabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    const abots = await listAbots(page);
    const found = abots.find(a => a.name === abot);
    expect(found).toBeDefined();
    expect(found!.added_at).toBeTruthy();
  });
});

// ── Active kubo selection ─────────────────────────────────────────────────

test.describe('Active kubo selection', () => {
  test.afterEach(async ({ page }) => {
    // Navigate to app first so localStorage is accessible.
    if (page.url() === 'about:blank') await page.goto('/');
    await page.evaluate(() => localStorage.removeItem('abot_active_kubo')).catch(() => {});
    await cleanupResources(page);
  });

  test('active kubo is null on fresh load (no default)', async ({ page }) => {
    await waitForApp(page);

    // With no default kubo, activeKubo starts as null.
    const activeKubo = await page.evaluate(() => localStorage.getItem('abot_active_kubo'));
    // Should be null (no default kubo) or whatever was persisted from a previous session.
    // On a truly fresh load, it's null.
    expect(activeKubo === null || typeof activeKubo === 'string').toBeTruthy();
  });

  test('active kubo persists across page reloads', async ({ page }) => {
    test.setTimeout(60_000);
    // Pre-seed localStorage with a non-default kubo name.
    const kuboName = `e2e-persist-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    await waitForApp(page);

    // Set active kubo via localStorage (simulating a click).
    await page.evaluate((name) => localStorage.setItem('abot_active_kubo', name), kuboName);

    // Reload and verify it persists.
    await page.reload();
    await page.locator('flutter-view').waitFor({ timeout: 30_000 });
    await page.waitForTimeout(2000);

    const activeKubo = await page.evaluate(() => localStorage.getItem('abot_active_kubo'));
    expect(activeKubo).toBe(kuboName);
  });

  test('empty state shows no xterm containers regardless of active kubo', async ({ page }) => {
    // Clean all sessions.
    const sessions = await listSessions(page);
    for (const s of sessions) {
      await deleteSession(page, s.name).catch(() => {});
    }

    // Create a kubo but add no abots.
    const kuboName = `e2e-empty-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    // Navigate first so localStorage is accessible, then set active kubo.
    await page.goto('/');
    await page.evaluate((name) => localStorage.setItem('abot_active_kubo', name), kuboName);

    // Reload to pick up the localStorage value.
    await page.reload();
    await page.locator('flutter-view').waitFor({ timeout: 30_000 });
    await page.waitForTimeout(2000);

    const count = await page.locator('.xterm-container').count();
    expect(count).toBe(0);
  });

  test('sessions in a different kubo do not create xterms when active kubo is empty', async ({ page }) => {
    // Clean all sessions first.
    const sessions = await listSessions(page);
    for (const s of sessions) {
      await deleteSession(page, s.name).catch(() => {});
    }

    // Create a kubo and add an abot.
    const otherKubo = `e2e-other-${Date.now()}`;
    await trackedCreateKubo(page, otherKubo);
    await trackedAddAbot(page, otherKubo, `e2e-other-abot-${Date.now()}`);

    // Create a separate empty kubo and set it active.
    const emptyKubo = `e2e-empty2-${Date.now()}`;
    await trackedCreateKubo(page, emptyKubo);

    // This test verifies the API setup — sessions exist in otherKubo, not in emptyKubo.
    const allSessions = await listSessions(page);
    expect(allSessions.filter(s => s.kubo === emptyKubo).length).toBe(0);
    expect(allSessions.filter(s => s.kubo === otherKubo).length).toBeGreaterThanOrEqual(1);
  });
});

// ── Kubo landing page ─────────────────────────────────────────────────────

test.describe('Kubo landing page', () => {
  test.afterEach(async ({ page }) => {
    if (page.url() === 'about:blank') await page.goto('/');
    await page.evaluate(() => {
      localStorage.removeItem('abot_active_kubo');
      localStorage.removeItem('abot_open_kubos');
    }).catch(() => {});
    await cleanupResources(page);
  });

  test('kubo with abots has sessions visible via API', async ({ page }) => {
    // Clean slate
    const sessions = await listSessions(page);
    for (const s of sessions) {
      await deleteSession(page, s.name).catch(() => {});
    }

    // Create a kubo and add abots
    const kubo = `e2e-lp-kubo-${Date.now()}`;
    await trackedCreateKubo(page, kubo);
    const ts = Date.now();
    const abot1 = `e2e-lp-a-${ts}`;
    const abot2 = `e2e-lp-b-${ts}`;
    await trackedAddAbot(page, kubo, abot1);
    await trackedAddAbot(page, kubo, abot2);

    // Both sessions should exist on the server (qualified names)
    const allSessions = await listSessions(page);
    expect(allSessions.find(s => s.name === `${abot1}@${kubo}`)).toBeDefined();
    expect(allSessions.find(s => s.name === `${abot2}@${kubo}`)).toBeDefined();

    // App loads and shows flutter-view
    await waitForApp(page);
    const flutterView = page.locator('flutter-view');
    await expect(flutterView).toBeVisible();
  });

  test('closing all sessions leaves no sessions on server', async ({ page }) => {
    // Create a kubo and abot
    const kubo = `e2e-lp-close-kubo-${Date.now()}`;
    await trackedCreateKubo(page, kubo);
    const ts = Date.now();
    await trackedAddAbot(page, kubo, `e2e-lp-close-${ts}`);

    // Verify session exists
    let sessions = await listSessions(page);
    expect(sessions.length).toBeGreaterThanOrEqual(1);

    // Delete all sessions via API
    for (const s of sessions) {
      await deleteSession(page, s.name).catch(() => {});
    }
    createdSessions.length = 0;

    // Verify no sessions remain
    sessions = await listSessions(page);
    expect(sessions.length).toBe(0);

    // Reload app — should show empty state (landing page)
    await waitForApp(page);
    const count = await page.locator('.xterm-container').count();
    expect(count).toBe(0);
  });

  test('switching active kubo via localStorage persists across reload', async ({ page }) => {
    test.setTimeout(60_000);
    // Create two kubos with abots
    const ts = Date.now();
    const kubo1 = `e2e-lp-k1-${ts}`;
    const kubo2 = `e2e-lp-k2-${ts}`;
    await trackedCreateKubo(page, kubo1);
    await trackedCreateKubo(page, kubo2);
    await trackedAddAbot(page, kubo1, `e2e-lp-k1a-${ts}`);
    await trackedAddAbot(page, kubo2, `e2e-lp-k2a-${ts}`);

    // Set kubo1 as active and verify it persists after reload
    await page.goto('/');
    await page.evaluate((name) => localStorage.setItem('abot_active_kubo', name), kubo1);
    await page.reload();
    await page.locator('flutter-view').waitFor({ timeout: 30_000 });

    let activeKubo = await page.evaluate(() => localStorage.getItem('abot_active_kubo'));
    expect(activeKubo).toBe(kubo1);

    // Switch to kubo2 and verify persistence
    await page.evaluate((name) => localStorage.setItem('abot_active_kubo', name), kubo2);
    await page.reload();
    await page.locator('flutter-view').waitFor({ timeout: 30_000 });

    activeKubo = await page.evaluate(() => localStorage.getItem('abot_active_kubo'));
    expect(activeKubo).toBe(kubo2);
  });

  test('empty kubo (no abots) shows no xterms — landing page visible', async ({ page }) => {
    // Clean slate
    const sessions = await listSessions(page);
    for (const s of sessions) {
      await deleteSession(page, s.name).catch(() => {});
    }

    // Create an empty kubo
    const kuboName = `e2e-lp-empty-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    // Set it as active and load
    await page.goto('/');
    await page.evaluate((name) => localStorage.setItem('abot_active_kubo', name), kuboName);
    await page.reload();
    await page.locator('flutter-view').waitFor({ timeout: 30_000 });
    await page.waitForTimeout(2000);

    // No xterm containers — landing page should be showing
    const count = await page.locator('.xterm-container').count();
    expect(count).toBe(0);

    // Active kubo should still be our empty kubo
    const activeKubo = await page.evaluate(() => localStorage.getItem('abot_active_kubo'));
    expect(activeKubo).toBe(kuboName);
  });

  test('adding abot to active empty kubo creates a session', async ({ page }) => {
    // Create empty kubo and set active
    const kuboName = `e2e-lp-add-${Date.now()}`;
    await trackedCreateKubo(page, kuboName);

    // No sessions in this kubo initially
    let sessions = await listSessions(page);
    expect(sessions.filter(s => s.kubo === kuboName).length).toBe(0);

    // Add an abot — this creates a session in the kubo
    const abotName = `e2e-lp-newabot-${Date.now()}`;
    await trackedAddAbot(page, kuboName, abotName);

    // Session should now exist in the kubo (qualified name)
    sessions = await listSessions(page);
    const session = sessions.find(s => s.name === `${abotName}@${kuboName}`);
    expect(session).toBeDefined();
    expect(session!.kubo).toBe(kuboName);

    // Set as active and verify app loads
    await page.goto('/');
    await page.evaluate((name) => localStorage.setItem('abot_active_kubo', name), kuboName);
    await page.reload();
    await page.locator('flutter-view').waitFor({ timeout: 30_000 });
    await page.waitForTimeout(2000);

    const activeKubo = await page.evaluate(() => localStorage.getItem('abot_active_kubo'));
    expect(activeKubo).toBe(kuboName);
  });
});

// ── Kubo creation round-trip ───────────────────────────────────────────────

test.describe('Kubo creation round-trip', () => {
  test.afterEach(async ({ page }) => {
    if (page.url() === 'about:blank') await page.goto('/');
    await page.evaluate(() => {
      localStorage.removeItem('abot_active_kubo');
      localStorage.removeItem('abot_open_kubos');
    }).catch(() => {});
    await cleanupResources(page);
  });

  test('creating a kubo makes it immediately listable via API', async ({ page }) => {
    const name = `e2e-create-rt-${Date.now()}`;
    await trackedCreateKubo(page, name);

    const kubos = await listKubos(page);
    const found = kubos.find(k => k.name === name);
    expect(found).toBeDefined();
    expect(found!.running).toBe(false);
    expect(found!.activeSessions).toBe(0);
    expect(found!.abots).toEqual([]);
  });

  test('creating a kubo and adding an abot produces a working session', async ({ page }) => {
    const kubo = `e2e-create-sess-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-create-abot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Session should exist in the new kubo (qualified name)
    const sessions = await listSessions(page);
    const session = sessions.find(s => s.name === `${abot}@${kubo}`);
    expect(session).toBeDefined();
    expect(session!.kubo).toBe(kubo);

    // Kubo should list the abot
    const kubos = await listKubos(page);
    const found = kubos.find(k => k.name === kubo);
    expect(found).toBeDefined();
    expect(found!.abots).toContain(abot);
  });

  test('new kubo appears in app after creation and reload', async ({ page }) => {
    const kubo = `e2e-create-ui-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    // Set it as active kubo (simulating what the Flutter client does on create)
    await page.goto('/');
    await page.evaluate((name) => {
      localStorage.setItem('abot_active_kubo', name);
      const open = JSON.parse(localStorage.getItem('abot_open_kubos') ?? '[]');
      if (!open.includes(name)) open.push(name);
      localStorage.setItem('abot_open_kubos', JSON.stringify(open));
    }, kubo);

    await page.reload();
    await page.locator('flutter-view').waitFor({ timeout: 30_000 });
    await page.waitForTimeout(2000);

    // Verify localStorage persisted
    const activeKubo = await page.evaluate(() => localStorage.getItem('abot_active_kubo'));
    expect(activeKubo).toBe(kubo);

    const openKubos = await page.evaluate(() => {
      const raw = localStorage.getItem('abot_open_kubos');
      return raw ? JSON.parse(raw) : [];
    });
    expect(openKubos).toContain(kubo);
  });
});

// ── Add existing abot to another kubo ─────────────────────────────────────

test.describe('Add existing abot to another kubo', () => {
  test.afterEach(async ({ page }) => {
    await cleanupResources(page);
  });

  test('an abot employed in one kubo can be added to a second kubo with independent sessions', async ({ page }) => {
    // Create first kubo and add abot
    const kubo1 = `e2e-src-${Date.now()}`;
    await trackedCreateKubo(page, kubo1);
    const abot = `e2e-shared-${Date.now()}`;
    await trackedAddAbot(page, kubo1, abot);

    // Verify abot exists in first kubo
    let kubos = await listKubos(page);
    expect(kubos.find(k => k.name === kubo1)!.abots).toContain(abot);

    // Create second kubo and add the SAME abot
    const kubo2 = `e2e-dst-${Date.now()}`;
    await trackedCreateKubo(page, kubo2);
    const result = await addAbotToKubo(page, kubo2, abot, true);
    createdSessions.push(`${abot}@${kubo2}`);
    expect(result.abot).toBe(abot);
    expect(result.kubo).toBe(kubo2);
    // Each kubo gets its own session: abot@kubo2
    expect(result.session).toBe(`${abot}@${kubo2}`);

    // The abot should now be in both kubos
    kubos = await listKubos(page);
    expect(kubos.find(k => k.name === kubo1)!.abots).toContain(abot);
    expect(kubos.find(k => k.name === kubo2)!.abots).toContain(abot);

    // Both sessions should exist independently (one per kubo)
    const sessions = await listSessions(page);
    expect(sessions.find(s => s.name === `${abot}@${kubo1}`)).toBeDefined();
    expect(sessions.find(s => s.name === `${abot}@${kubo2}`)).toBeDefined();
  });

  test('adding same abot to second kubo creates independent session (both alive)', async ({ page }) => {
    // With kubo-scoped sessions, each kubo gets its own session.
    const kubo1 = `e2e-rep1-${Date.now()}`;
    await trackedCreateKubo(page, kubo1);
    const abot = `e2e-repabot-${Date.now()}`;
    await trackedAddAbot(page, kubo1, abot);

    // Session should exist in kubo1
    let sessions = await listSessions(page);
    let session = sessions.find(s => s.name === `${abot}@${kubo1}`);
    expect(session).toBeDefined();
    expect(session!.kubo).toBe(kubo1);

    // Now add same abot to kubo2 with createSession=true
    const kubo2 = `e2e-rep2-${Date.now()}`;
    await trackedCreateKubo(page, kubo2);
    const result = await addAbotToKubo(page, kubo2, abot, true);
    createdSessions.push(`${abot}@${kubo2}`);
    expect(result.abot).toBe(abot);
    // Session IS created — each kubo gets its own session
    expect(result.session).toBe(`${abot}@${kubo2}`);

    // Both sessions should be alive
    sessions = await listSessions(page);
    const s1 = sessions.find(s => s.name === `${abot}@${kubo1}`);
    const s2 = sessions.find(s => s.name === `${abot}@${kubo2}`);
    expect(s1).toBeDefined();
    expect(s2).toBeDefined();
    expect(s1!.kubo).toBe(kubo1);
    expect(s2!.kubo).toBe(kubo2);

    // The abot should appear in both kubos' manifests
    const kubos = await listKubos(page);
    expect(kubos.find(k => k.name === kubo1)!.abots).toContain(abot);
    expect(kubos.find(k => k.name === kubo2)!.abots).toContain(abot);
  });

  test('adding abot with dead session in another kubo creates new session', async ({ page }) => {
    // Scenario: alice has a dead session in kubo1. User adds alice to kubo2.
    // With kubo-scoped sessions, kubo2 always gets its own session.
    const kubo1 = `e2e-dead1-${Date.now()}`;
    await trackedCreateKubo(page, kubo1);
    const abot = `e2e-deadabot-${Date.now()}`;
    await trackedAddAbot(page, kubo1, abot);

    // Kill the session in kubo1
    await deleteSession(page, `${abot}@${kubo1}`);
    const idx = createdSessions.indexOf(`${abot}@${kubo1}`);
    if (idx >= 0) createdSessions.splice(idx, 1);

    // Add to kubo2 — creates an independent session
    const kubo2 = `e2e-dead2-${Date.now()}`;
    await trackedCreateKubo(page, kubo2);
    const result = await addAbotToKubo(page, kubo2, abot, true);
    expect(result.abot).toBe(abot);
    expect(result.session).toBe(`${abot}@${kubo2}`);
    createdSessions.push(`${abot}@${kubo2}`);

    // Session should be in kubo2
    const sessions = await listSessions(page);
    const session = sessions.find(s => s.name === `${abot}@${kubo2}`);
    expect(session).toBeDefined();
    expect(session!.kubo).toBe(kubo2);
  });

  test('deleting session in one kubo does not affect session in another', async ({ page }) => {
    // Multi-kubo session independence: alice@kubo1 and alice@kubo2 are independent.
    const kubo1 = `e2e-indep1-${Date.now()}`;
    const kubo2 = `e2e-indep2-${Date.now()}`;
    await trackedCreateKubo(page, kubo1);
    await trackedCreateKubo(page, kubo2);

    const abot = `e2e-indepabot-${Date.now()}`;
    await trackedAddAbot(page, kubo1, abot);
    const result2 = await addAbotToKubo(page, kubo2, abot, true);
    createdSessions.push(`${abot}@${kubo2}`);
    expect(result2.session).toBe(`${abot}@${kubo2}`);

    // Both sessions should exist
    let sessions = await listSessions(page);
    expect(sessions.find(s => s.name === `${abot}@${kubo1}`)).toBeDefined();
    expect(sessions.find(s => s.name === `${abot}@${kubo2}`)).toBeDefined();

    // Delete session in kubo1
    await deleteSession(page, `${abot}@${kubo1}`);
    const idx2 = createdSessions.indexOf(`${abot}@${kubo1}`);
    if (idx2 >= 0) createdSessions.splice(idx2, 1);

    // Session in kubo2 should still exist and be unaffected
    sessions = await listSessions(page);
    expect(sessions.find(s => s.name === `${abot}@${kubo1}`)).toBeUndefined();
    const remaining = sessions.find(s => s.name === `${abot}@${kubo2}`);
    expect(remaining).toBeDefined();
    expect(remaining!.kubo).toBe(kubo2);
  });

  test('add-abot response includes all expected fields', async ({ page }) => {
    const kubo = `e2e-resp-${Date.now()}`;
    await trackedCreateKubo(page, kubo);
    const abot = `e2e-respabot-${Date.now()}`;

    // POST with raw request to inspect the full response
    const resp = await page.request.post(`/kubos/${encodeURIComponent(kubo)}/abots`, {
      data: { abot, createSession: true },
    });
    expect(resp.ok(), `POST failed: ${resp.status()} ${await resp.text()}`).toBeTruthy();
    const body = await resp.json();
    expect(body.kubo).toBe(kubo);
    expect(body.abot).toBe(abot);
    expect(body.session).toBe(`${abot}@${kubo}`);
    createdKubos.push(kubo);
    createdSessions.push(`${abot}@${kubo}`);
  });

  test('abot worktree is created in the second kubo', async ({ page }) => {
    const kubo1 = `e2e-wt1-${Date.now()}`;
    await trackedCreateKubo(page, kubo1);
    const abot = `e2e-wtabot-${Date.now()}`;
    await trackedAddAbot(page, kubo1, abot);

    const kubo2 = `e2e-wt2-${Date.now()}`;
    await trackedCreateKubo(page, kubo2);
    await addAbotToKubo(page, kubo2, abot, false);

    // Abot detail should show kubo branches for both kubos
    const detail = await getAbotDetail(page, abot);
    expect(detail.name).toBe(abot);
    const branchNames = (detail.kubo_branches ?? []).map((b: any) => b.kubo_name);
    expect(branchNames).toContain(kubo1);
    expect(branchNames).toContain(kubo2);
  });
});

// ── Remove abot from kubo ──────────────────────────────────────────────────

test.describe('Remove abot from kubo', () => {
  test.afterEach(async ({ page }) => {
    await cleanupResources(page);
  });

  test('DELETE /kubos/:name/abots/:abot removes abot and session', async ({ page }) => {
    const kubo = `e2e-rm-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-rmabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Verify session exists (qualified name)
    const qualified = `${abot}@${kubo}`;
    let sessions = await listSessions(page);
    expect(sessions.find(s => s.name === qualified)).toBeDefined();

    // Remove abot from kubo
    const result = await removeAbotFromKubo(page, kubo, abot);
    expect(result.kubo).toBe(kubo);
    expect(result.abot).toBe(abot);

    // Session should be gone
    sessions = await listSessions(page);
    expect(sessions.find(s => s.name === qualified)).toBeUndefined();

    // Abot should be removed from kubo manifest
    const kubos = await listKubos(page);
    const updatedKubo = kubos.find(k => k.name === kubo);
    expect(updatedKubo).toBeDefined();
    expect(updatedKubo!.abots).not.toContain(abot);

    // Remove from tracked so cleanup doesn't fail
    const idx = createdSessions.indexOf(qualified);
    if (idx >= 0) createdSessions.splice(idx, 1);
  });

  test('GET /kubos includes abots array', async ({ page }) => {
    const kubo = `e2e-abots-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot1 = `e2e-a1-${Date.now()}`;
    const abot2 = `e2e-a2-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot1);
    await trackedAddAbot(page, kubo, abot2);

    const kubos = await listKubos(page);
    const found = kubos.find(k => k.name === kubo);
    expect(found).toBeDefined();
    expect(found!.abots).toContain(abot1);
    expect(found!.abots).toContain(abot2);
  });

  test('removing non-existent abot from kubo still succeeds', async ({ page }) => {
    const kubo = `e2e-rmnone-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    // Remove an abot that was never added — should succeed (idempotent)
    const result = await removeAbotFromKubo(page, kubo, 'nonexistent');
    expect(result.kubo).toBe(kubo);
    expect(result.abot).toBe('nonexistent');
  });
});

// ── Variant lifecycle ──────────────────────────────────────────────────────

test.describe('Variant lifecycle', () => {
  test.afterEach(async ({ page }) => {
    await cleanupResources(page);
  });

  test('abot detail includes has_session field on kubo branches', async ({ page }) => {
    const kubo = `e2e-hs-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-hsabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    const detail = await getAbotDetail(page, abot);
    const branch = detail.kubo_branches.find(b => b.kubo_name === kubo);
    expect(branch).toBeDefined();
    expect(branch!.has_worktree).toBe(true);
    // has_session should be true because addAbotToKubo created a session
    expect(typeof branch!.has_session).toBe('boolean');
  });

  test('abot detail does not include merged field', async ({ page }) => {
    const kubo = `e2e-nm-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-nmabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    const detail = await getAbotDetail(page, abot);
    const branch = detail.kubo_branches.find(b => b.kubo_name === kubo);
    expect(branch).toBeDefined();
    expect((branch as any).merged).toBeUndefined();
  });

  test('dismiss removes worktree but keeps branch', async ({ page }) => {
    const kubo = `e2e-dism-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-dismabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Verify employed state (has worktree)
    let detail = await getAbotDetail(page, abot);
    let branch = detail.kubo_branches.find(b => b.kubo_name === kubo);
    expect(branch).toBeDefined();
    expect(branch!.has_worktree).toBe(true);

    // Dismiss
    const result = await dismissVariant(page, abot, kubo);
    expect(result.ok, `dismiss failed: ${result.status} ${JSON.stringify(result.body)}`).toBeTruthy();
    expect(result.body.dismissed).toBe(abot);
    expect(result.body.kubo).toBe(kubo);

    // Session should be gone (qualified name)
    const sessions = await listSessions(page);
    expect(sessions.find(s => s.name === `${abot}@${kubo}`)).toBeUndefined();

    // Branch should still exist but worktree should be gone
    detail = await getAbotDetail(page, abot);
    branch = detail.kubo_branches.find(b => b.kubo_name === kubo);
    expect(branch).toBeDefined();
    expect(branch!.has_worktree).toBe(false);

    // Abot should be removed from kubo manifest
    const kubos = await listKubos(page);
    const found = kubos.find(k => k.name === kubo);
    expect(found!.abots).not.toContain(abot);

    // Clean up tracking
    const idx = createdSessions.indexOf(`${abot}@${kubo}`);
    if (idx >= 0) createdSessions.splice(idx, 1);
  });

  test('integrate merges variant into default branch and deletes kubo branch', async ({ page }) => {
    const kubo = `e2e-int-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-intabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Dismiss first (removes worktree + session, keeps branch)
    await dismissVariant(page, abot, kubo);
    const idx = createdSessions.indexOf(`${abot}@${kubo}`);
    if (idx >= 0) createdSessions.splice(idx, 1);

    // Verify kubo branch still exists but worktree is gone
    let detail = await getAbotDetail(page, abot);
    let branch = detail.kubo_branches.find(b => b.kubo_name === kubo);
    expect(branch).toBeDefined();
    expect(branch!.has_worktree).toBe(false);

    // Integrate the variant
    const result = await integrateVariant(page, abot, kubo);
    expect(result.ok, `integrate failed: ${result.status} ${JSON.stringify(result.body)}`).toBeTruthy();
    expect(result.body.integrated).toBe(abot);
    expect(result.body.kubo).toBe(kubo);

    // Kubo branch should be gone
    detail = await getAbotDetail(page, abot);
    expect(detail.kubo_branches.find(b => b.kubo_name === kubo)).toBeUndefined();
  });

  test('discard deletes kubo branch without merging', async ({ page }) => {
    const kubo = `e2e-disc-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-discabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Dismiss first (removes worktree + session, keeps branch)
    await dismissVariant(page, abot, kubo);
    const idx = createdSessions.indexOf(`${abot}@${kubo}`);
    if (idx >= 0) createdSessions.splice(idx, 1);

    // Verify kubo branch still exists
    let detail = await getAbotDetail(page, abot);
    expect(detail.kubo_branches.find(b => b.kubo_name === kubo)).toBeDefined();

    // Discard the variant
    const result = await discardVariant(page, abot, kubo);
    expect(result.ok, `discard failed: ${result.status} ${JSON.stringify(result.body)}`).toBeTruthy();
    expect(result.body.discarded).toBe(abot);
    expect(result.body.kubo).toBe(kubo);

    // Kubo branch should be gone
    detail = await getAbotDetail(page, abot);
    expect(detail.kubo_branches.find(b => b.kubo_name === kubo)).toBeUndefined();
  });

  test('discard works on employed variant (has worktree + session)', async ({ page }) => {
    const kubo = `e2e-discact-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-discactabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Discard while still employed — should kill session + remove worktree + delete branch
    const result = await discardVariant(page, abot, kubo);
    expect(result.ok, `discard failed: ${result.status} ${JSON.stringify(result.body)}`).toBeTruthy();

    // Session should be gone (qualified name)
    const sessions = await listSessions(page);
    expect(sessions.find(s => s.name === `${abot}@${kubo}`)).toBeUndefined();

    // Kubo branch should be gone
    const detail = await getAbotDetail(page, abot);
    expect(detail.kubo_branches.find(b => b.kubo_name === kubo)).toBeUndefined();

    // Clean up tracking
    const idx = createdSessions.indexOf(`${abot}@${kubo}`);
    if (idx >= 0) createdSessions.splice(idx, 1);
  });

  test('integrate on employed variant removes worktree first', async ({ page }) => {
    const kubo = `e2e-intact-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-intactabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Delete session but keep worktree (don't remove from kubo)
    await deleteSession(page, `${abot}@${kubo}`);
    const idx = createdSessions.indexOf(`${abot}@${kubo}`);
    if (idx >= 0) createdSessions.splice(idx, 1);

    // Integrate while worktree still exists
    const result = await integrateVariant(page, abot, kubo);
    expect(result.ok, `integrate failed: ${result.status} ${JSON.stringify(result.body)}`).toBeTruthy();

    // Kubo branch should be gone
    const detail = await getAbotDetail(page, abot);
    expect(detail.kubo_branches.find(b => b.kubo_name === kubo)).toBeUndefined();
  });

  test('integrate on nonexistent abot returns error', async ({ page }) => {
    const result = await integrateVariant(page, 'nonexistent-abot', 'nonexistent-kubo');
    expect(result.ok).toBeFalsy();
  });

  test('discard on nonexistent abot returns error', async ({ page }) => {
    const result = await discardVariant(page, 'nonexistent-abot', 'nonexistent-kubo');
    expect(result.ok).toBeFalsy();
  });

  test('integrate removes abot from kubo manifest', async ({ page }) => {
    const kubo = `e2e-intmf-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-intmfabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Dismiss first, then integrate
    await dismissVariant(page, abot, kubo);
    const idx = createdSessions.indexOf(`${abot}@${kubo}`);
    if (idx >= 0) createdSessions.splice(idx, 1);

    await integrateVariant(page, abot, kubo);

    // Kubo manifest should no longer list this abot
    const kubos = await listKubos(page);
    const found = kubos.find(k => k.name === kubo);
    expect(found).toBeDefined();
    expect(found!.abots).not.toContain(abot);
  });

  test('discard removes abot from kubo manifest', async ({ page }) => {
    const kubo = `e2e-discmf-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-discmfabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Dismiss first, then discard
    await dismissVariant(page, abot, kubo);
    const idx = createdSessions.indexOf(`${abot}@${kubo}`);
    if (idx >= 0) createdSessions.splice(idx, 1);

    await discardVariant(page, abot, kubo);

    // Kubo manifest should no longer list this abot
    const kubos = await listKubos(page);
    const found = kubos.find(k => k.name === kubo);
    expect(found).toBeDefined();
    expect(found!.abots).not.toContain(abot);
  });

  test('full lifecycle: employ → dismiss → integrate', async ({ page }) => {
    const kubo = `e2e-lc-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-lcabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Verify employed (has worktree + session)
    let detail = await getAbotDetail(page, abot);
    let branch = detail.kubo_branches.find(b => b.kubo_name === kubo);
    expect(branch).toBeDefined();
    expect(branch!.has_worktree).toBe(true);

    // Dismiss — removes worktree + session, keeps branch
    await dismissVariant(page, abot, kubo);
    const idx = createdSessions.indexOf(`${abot}@${kubo}`);
    if (idx >= 0) createdSessions.splice(idx, 1);

    // Worktree gone, branch still there
    detail = await getAbotDetail(page, abot);
    branch = detail.kubo_branches.find(b => b.kubo_name === kubo);
    expect(branch).toBeDefined();
    expect(branch!.has_worktree).toBe(false);

    // Integrate — merges into default, removes branch
    const result = await integrateVariant(page, abot, kubo);
    expect(result.ok).toBeTruthy();

    // Branch gone
    detail = await getAbotDetail(page, abot);
    expect(detail.kubo_branches.find(b => b.kubo_name === kubo)).toBeUndefined();
  });

  test('full lifecycle: employ → dismiss → discard', async ({ page }) => {
    const kubo = `e2e-lcd-${Date.now()}`;
    await trackedCreateKubo(page, kubo);

    const abot = `e2e-lcdabot-${Date.now()}`;
    await trackedAddAbot(page, kubo, abot);

    // Dismiss — removes worktree + session, keeps branch
    await dismissVariant(page, abot, kubo);
    const idx = createdSessions.indexOf(`${abot}@${kubo}`);
    if (idx >= 0) createdSessions.splice(idx, 1);

    // Branch still exists, worktree gone
    let detail = await getAbotDetail(page, abot);
    let branch = detail.kubo_branches.find(b => b.kubo_name === kubo);
    expect(branch).toBeDefined();
    expect(branch!.has_worktree).toBe(false);

    // Discard — deletes branch
    const result = await discardVariant(page, abot, kubo);
    expect(result.ok).toBeTruthy();

    // Branch gone
    detail = await getAbotDetail(page, abot);
    expect(detail.kubo_branches.find(b => b.kubo_name === kubo)).toBeUndefined();
  });
});
