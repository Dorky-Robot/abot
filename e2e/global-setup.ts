import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

export default async function globalSetup() {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'abot-e2e-'));
  process.env.ABOT_E2E_DATA_DIR = dataDir;
  // Write path to a file so global-teardown can read it
  fs.writeFileSync(path.join(os.tmpdir(), 'abot-e2e-datadir'), dataDir);
}
