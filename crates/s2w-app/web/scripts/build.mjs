import { rm, mkdir, copyFile } from 'node:fs/promises';
import { build } from 'esbuild';
await rm('dist', { recursive: true, force: true });
await mkdir('dist');
await build({ entryPoints: { main: 'src/main.ts', home: 'src/home-main.ts' }, bundle: true, outdir: 'dist', format: 'esm',
  target: 'es2022', minify: true, legalComments: 'eof' });
for (const file of ['index.html', 'home.html', 'style.css']) await copyFile(file, `dist/${file}`);
