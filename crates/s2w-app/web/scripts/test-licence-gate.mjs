import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
const allow = 'MIT;Apache-2.0;ISC;BSD-2-Clause;BSD-3-Clause;0BSD';
// The checker stops at its first violation. Exclude that package on the second pass to
// prove that both prohibited licences independently fail, without changing the fixture.
for (const [name, license, exclude] of [
  ['bad-pkg', 'GPL-3.0', 'bad-pkg-2@1.0.0'],
  ['bad-pkg-2', 'CC-BY-NC-4.0', 'bad-pkg@1.0.0'],
]) {
  const result = spawnSync('node_modules/.bin/license-checker-rseidelsohn', [
    '--production', '--excludePrivatePackages', '--onlyAllow', allow,
    '--start', 'fixtures/bad-licence', '--excludePackages', exclude,
  ], { encoding: 'utf8' });
  if (result.error) throw result.error;
  const output = result.stdout + result.stderr;
  assert.notEqual(result.status, 0, `${name} was accepted`);
  assert.ok(output.includes(`${name}@1.0.0`) && output.includes(license), output);
  console.log(`Rejected ${name}: ${license}`);
}
