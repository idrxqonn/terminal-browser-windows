#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

// .claude/types holds the plugin API declarations the plugin's tsconfig includes.
// They are gitignored and tied to the installed Claude Code, so we regenerate
// them here rather than checking them in. Claude Code unpacks them to a temp dir
// when its plugin-authoring skill loads.
const root = path.resolve(import.meta.dirname, "..");
const dir = path.join(root, "claude-code-plugin", ".claude", "types");

if (process.argv[2] === "--if-missing" && fs.existsSync(path.join(dir, "claude-code.d.ts"))) {
  process.exit(0);
}

function resolveClaude() {
  const probe = process.platform === "win32" ? "where" : "which";
  // The native installer gives claude.exe; installing through npm gives a claude.cmd
  // shim instead, which node cannot spawn unless it goes through a shell.
  const names = process.platform === "win32" ? ["claude.exe", "claude.cmd", "claude.bat"] : ["claude"];
  for (const name of names) {
    const found = spawnSync(probe, [name], { stdio: ["ignore", "pipe", "ignore"], encoding: "utf8" });
    if (found.status !== 0) continue;
    const first = found.stdout.split(/\r?\n/).find((line) => line.trim());
    if (first) return first.trim();
  }
  return null;
}

const claude = resolveClaude();
if (!claude) {
  // fixme: placeholder copy
  process.stderr.write("plugin-types: claude is not on PATH, skipping (run 'pnpm types' once it is installed)\n");
  process.exit(0);
}

const viaShell = /\.(cmd|bat)$/i.test(claude);
// A shell concatenates argv, so anything with a space has to carry its own quotes.
function runClaude(args, quoted) {
  if (viaShell) {
    return spawnSync([`"${claude}"`, ...quoted].join(" "), {
      stdio: ["ignore", "pipe", "ignore"],
      encoding: "utf8",
      shell: true,
    });
  }
  return spawnSync(claude, args, { stdio: ["ignore", "pipe", "ignore"], encoding: "utf8" });
}

const versionRun = runClaude(["--version"], ["--version"]);
const version = (versionRun.stdout ?? "").trim().split(/\s+/)[0];
if (!version) {
  process.stderr.write("plugin-types: could not read the claude version, skipping\n");
  process.exit(0);
}

runClaude(
  ["-p", "/plugin-authoring reply ok", "--max-turns", "1"],
  ["-p", '"/plugin-authoring reply ok"', "--max-turns", "1"],
);

// The unpack directory is named for the user on unix and plainly "claude" on
// Windows, so the tree is searched rather than rebuilt from a uid.
function unpackedTypes() {
  const tmp = os.tmpdir();
  const found = [];
  let roots;
  try {
    roots = fs.readdirSync(tmp).filter((name) => name.startsWith("claude"));
  } catch {
    return null;
  }
  for (const base of roots) {
    const versionDir = path.join(tmp, base, "bundled-skills", version);
    let sessions;
    try {
      sessions = fs.readdirSync(versionDir);
    } catch {
      continue;
    }
    for (const session of sessions) {
      const candidate = path.join(versionDir, session, "plugin-authoring", "types", "claude-code.d.ts");
      try {
        found.push({ candidate, at: fs.statSync(candidate).mtimeMs });
      } catch {}
    }
  }
  if (found.length === 0) return null;
  found.sort((a, b) => b.at - a.at);
  return found[0].candidate;
}

const source = unpackedTypes();
if (!source) {
  // fixme: placeholder copy
  process.stderr.write(`plugin-types: claude ${version} did not unpack the plugin types, skipping\n`);
  process.exit(0);
}

fs.mkdirSync(dir, { recursive: true });
fs.copyFileSync(source, path.join(dir, "claude-code.d.ts"));
