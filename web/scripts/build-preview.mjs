import { Resvg } from '@resvg/resvg-js';
import { readFile, writeFile } from 'node:fs/promises';
await writeFile('public/social.png', new Resvg(await readFile('public/social.svg')).render().asPng());
