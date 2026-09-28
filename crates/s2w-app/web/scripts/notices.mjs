import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { relative, resolve } from 'node:path';
const packages = JSON.parse(execFileSync('node_modules/.bin/license-checker-rseidelsohn',
  ['--production', '--excludePrivatePackages', '--relativeLicensePath', '--json'], { encoding: 'utf8' }));
const root = resolve('../../..');
const notices = Object.keys(packages).sort().map(name => {
  const info = packages[name];
  if (!info.licenseFile) throw new Error(`Missing licence text for ${name}`);
  const path = resolve(info.licenseFile);
  return `${name}\nLicense: ${info.licenses}\nSource: ${relative(root, path)}\n\n${readFileSync(path, 'utf8').trim()}\n`;
});
writeFileSync('dist/THIRD-PARTY-LICENSES.txt', notices.join('\n----------------------------------------\n\n'));
