import { rm, mkdir, copyFile } from 'node:fs/promises';
import { build } from 'esbuild';
await rm('dist', { recursive: true, force: true });
await mkdir('dist');
await build({ entryPoints: ['src/main.ts'], bundle: true, outfile: 'dist/main.js', format: 'esm',
  target: 'es2022', minify: true, legalComments: 'eof' });
for (const file of ['index.html', 'style.css']) await copyFile(file, `dist/${file}`);
