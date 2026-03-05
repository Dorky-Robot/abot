import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

export default async function globalTeardown() {
  const markerPath = path.join(os.tmpdir(), 'abot-e2e-datadir');
  if (fs.existsSync(markerPath)) {
    const dataDir = fs.readFileSync(markerPath, 'utf-8').trim();
    if (dataDir.startsWith(os.tmpdir()) && fs.existsSync(dataDir)) {
      fs.rmSync(dataDir, { recursive: true, force: true });
    }
    fs.unlinkSync(markerPath);
  }
}
