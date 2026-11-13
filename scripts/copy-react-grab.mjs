#!/usr/bin/env node
// react-grab's browser bundle is read from assets at runtime, in dev and in releases alike
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const root = path.resolve(import.meta.dirname, "..");
const require = createRequire(path.join(root, "browser", "package.json"));
const src = require.resolve("react-grab/dist/index.global.js");
const dest = path.join(root, "assets", "react-grab");

fs.mkdirSync(dest, { recursive: true });
fs.copyFileSync(src, path.join(dest, "index.global.js"));
