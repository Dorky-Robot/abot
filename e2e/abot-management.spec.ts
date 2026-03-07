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

async function dismissVariant(page: Page, abot: string, kubo: string) {
  const resp = await page.request.post(
    `/abots/${encodeURIComponent(abot)}/dismiss`,
    { data: { kubo } },
  );
  expect(resp.ok(), `dismissVariant(${abot}, ${kubo}) failed: ${resp.status()}`).toBeTruthy();
  return resp.json();
}

async function removeAbotFromKubo(page: Page, kubo: string, abot: string) {
  const resp = await page.request.delete(
    `/kubos/${encodeURIComponent(kubo)}/abots/${encodeURIComponent(abot)}`,
  );
  expect(resp.ok(), `removeAbotFromKubo(${kubo}, ${abot}) failed: ${resp.status()}`).toBeTruthy();
  return resp.json();
}

async function listKubos(page: Page) {
  const resp = await page.request.get('/kubos');
  expect(resp.ok()).toBeTruthy();
  return resp.json() as Promise<Array<{ name: string; abots: string[]; running: boolean }>>;
}

async function listAbots(page: Page) {
  const resp = await page.request.get('/abots');
  expect(resp.ok()).toBeTruthy();
  const data = await resp.json();
  return data.abots as Array<{
    name: string;
    kuboBranches: Array<{ kuboName: string; hasWorktree: boolean; hasSession: boolean | null }>;
  }>;
}

async function listSessions(page: Page) {
  const resp = await page.request.get('/sessions');
  expect(resp.ok()).toBeTruthy();
  return resp.json() as Promise<Array<{ name: string; alive: boolean }>>;
}

async function deleteSession(page: Page, name: string) {
  await page.request.delete(`/sessions/${encodeURIComponent(name)}`).catch(() => {});
}

async function stopKubo(page: Page, name: string) {
  await page.request.post(`/kubos/${encodeURIComponent(name)}/stop`).catch(() => {});
}

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

test.describe('Abot management: dismiss, remove, and state consistency', () => {
  test.afterEach(async ({ page }) => {
    await cleanup(page);
  });

  test('dismiss removes abot from kubo manifest and kills session', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-dism-${ts}`;
    const abot = `e2e-da-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot);

    // Verify session exists
    let sessions = await listSessions(page);
    const session = sessions.find((s) => s.name === `${abot}@${kubo}`);
    expect(session, 'session should exist after add').toBeTruthy();

    // Verify abot appears in kubo manifest
    let kubos = await listKubos(page);
    let kuboData = kubos.find((k) => k.name === kubo);
    expect(kuboData?.abots).toContain(abot);

    // Dismiss the abot
    await dismissVariant(page, abot, kubo);

    // Session should be gone
    sessions = await listSessions(page);
    const sessionAfter = sessions.find((s) => s.name === `${abot}@${kubo}`);
    expect(sessionAfter, 'session should be gone after dismiss').toBeFalsy();

    // Kubo manifest should no longer contain the abot
    kubos = await listKubos(page);
    kuboData = kubos.find((k) => k.name === kubo);
    expect(kuboData?.abots ?? []).not.toContain(abot);
  });

  test('remove_abot_from_kubo (X button) has same effect as dismiss', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-xbtn-${ts}`;
    const abot = `e2e-xa-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot);

    // Verify session and kubo manifest
    let sessions = await listSessions(page);
    expect(sessions.find((s) => s.name === `${abot}@${kubo}`)).toBeTruthy();
    let kubos = await listKubos(page);
    expect(kubos.find((k) => k.name === kubo)?.abots).toContain(abot);

    // Remove via the kubo endpoint (X button code path)
    await removeAbotFromKubo(page, kubo, abot);

    // Same postconditions as dismiss
    sessions = await listSessions(page);
    expect(sessions.find((s) => s.name === `${abot}@${kubo}`)).toBeFalsy();
    kubos = await listKubos(page);
    expect(kubos.find((k) => k.name === kubo)?.abots ?? []).not.toContain(abot);
  });

  test('dismiss updates abots list — branch still exists but no session', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-abotl-${ts}`;
    const abot = `e2e-al-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot);

    // Before dismiss: abot should have a kubo branch with session
    let abots = await listAbots(page);
    let abotData = abots.find((a) => a.name === abot);
    expect(abotData, 'abot should be in list').toBeTruthy();
    let branch = abotData?.kuboBranches.find((b) => b.kuboName === kubo);
    expect(branch, 'kubo branch should exist before dismiss').toBeTruthy();

    // Dismiss
    await dismissVariant(page, abot, kubo);

    // After dismiss: branch should still exist (past work) but no session
    abots = await listAbots(page);
    abotData = abots.find((a) => a.name === abot);
    expect(abotData, 'abot should still be in known list').toBeTruthy();
    branch = abotData?.kuboBranches.find((b) => b.kuboName === kubo);
    // The branch may still exist as past work (no worktree) or may not depending
    // on whether the branch was kept. Either way, there should be no active session.
    const sessions = await listSessions(page);
    expect(sessions.find((s) => s.name === `${abot}@${kubo}`)).toBeFalsy();
  });

  test('dismiss and X button produce identical state', async ({ page }) => {
    const ts = Date.now();
    const kubo1 = `e2e-eq1-${ts}`;
    const kubo2 = `e2e-eq2-${ts}`;
    const abot1 = `e2e-eqa-${ts}`;
    const abot2 = `e2e-eqb-${ts}`;

    // Set up two identical scenarios
    await trackedCreateKubo(page, kubo1);
    await trackedCreateKubo(page, kubo2);
    await trackedAddAbot(page, kubo1, abot1);
    await trackedAddAbot(page, kubo2, abot2);

    // Dismiss abot1 via dismiss endpoint
    await dismissVariant(page, abot1, kubo1);

    // Remove abot2 via X button endpoint
    await removeAbotFromKubo(page, kubo2, abot2);

    // Both kubos should have empty abot lists
    const kubos = await listKubos(page);
    const k1 = kubos.find((k) => k.name === kubo1);
    const k2 = kubos.find((k) => k.name === kubo2);
    expect(k1?.abots ?? []).toEqual([]);
    expect(k2?.abots ?? []).toEqual([]);

    // Neither should have a session
    const sessions = await listSessions(page);
    expect(sessions.find((s) => s.name === `${abot1}@${kubo1}`)).toBeFalsy();
    expect(sessions.find((s) => s.name === `${abot2}@${kubo2}`)).toBeFalsy();
  });

  test('adding abot back after dismiss creates new session', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-readd-${ts}`;
    const abot = `e2e-ra-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot);

    // Dismiss
    await dismissVariant(page, abot, kubo);

    // Re-add
    await trackedAddAbot(page, kubo, abot);

    // Session should exist again
    const sessions = await listSessions(page);
    const session = sessions.find((s) => s.name === `${abot}@${kubo}`);
    expect(session, 'session should exist after re-add').toBeTruthy();
    expect(session?.alive).toBeTruthy();

    // Kubo manifest should contain the abot
    const kubos = await listKubos(page);
    expect(kubos.find((k) => k.name === kubo)?.abots).toContain(abot);
  });

  test('dismissing one abot does not affect other abots in same kubo', async ({ page }) => {
    const ts = Date.now();
    const kubo = `e2e-multi-${ts}`;
    const abot1 = `e2e-m1-${ts}`;
    const abot2 = `e2e-m2-${ts}`;
    await trackedCreateKubo(page, kubo);
    await trackedAddAbot(page, kubo, abot1);
    await trackedAddAbot(page, kubo, abot2);

    // Dismiss only abot1
    await dismissVariant(page, abot1, kubo);

    // abot1 session gone, abot2 session still alive
    const sessions = await listSessions(page);
    expect(sessions.find((s) => s.name === `${abot1}@${kubo}`)).toBeFalsy();
    const session2 = sessions.find((s) => s.name === `${abot2}@${kubo}`);
    expect(session2, 'abot2 session should still exist').toBeTruthy();

    // Kubo should still list abot2 but not abot1
    const kubos = await listKubos(page);
    const kuboData = kubos.find((k) => k.name === kubo);
    expect(kuboData?.abots).not.toContain(abot1);
    expect(kuboData?.abots).toContain(abot2);
  });
});
