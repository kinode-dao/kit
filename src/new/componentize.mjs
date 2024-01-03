import { componentize } from '@bytecodealliance/componentize-js';
import { readFile, writeFile } from 'node:fs/promises';

// Retrieve the package name from command line arguments
const processName = process.argv[2];
if (!processName) {
    console.error('Please provide a process name (e.g. `node componentize.mjs process_name`).');
    process.exit(1);
}

const jsSource = await readFile('src/lib.js', 'utf8');
const witPath = 'wit/uqbar.wit';

const { component } = await componentize(jsSource, { witPath: witPath, worldName: 'process', debug: false });

await writeFile(`../pkg/${processName}.wasm`, component);
