// The sidecar as a process, for the black-box tests of its contract (HTTP,
// packets, VizRS commands): target/release/vizrs-cg by default (`cargo build
// --release`), or another build with VIRTRS_CG_BIN.

import { spawn } from "node:child_process";
import { accessSync, constants, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const RELEASE_BIN = fileURLToPath(new URL("../../target/release/vizrs-cg", import.meta.url));
/** The ErsatzTV script client: target/release/vizrs-etv-client or VIZRS_ETV_CLIENT_BIN. */
export const CLIENT = process.env.VIZRS_ETV_CLIENT_BIN || fileURLToPath(new URL("../../target/release/vizrs-etv-client", import.meta.url));
const BROWSERS = ["chromium-headless-shell", "chromium", "chromium-browser", "google-chrome", "google-chrome-stable", "chrome", "headless_shell"];
const START_TIMEOUT = 15000;

/** Command of the build under test. */
export function sidecarCommand(env = process.env) {
  return [env.VIRTRS_CG_BIN || RELEASE_BIN];
}

/** Chromium for the tests: VIZRS_BROWSER or the first one on PATH. */
export function findBrowser(env = process.env) {
  if (env.VIZRS_BROWSER) return env.VIZRS_BROWSER;
  for (const dir of (env.PATH ?? "").split(path.delimiter)) {
    for (const name of BROWSERS) {
      const file = path.join(dir, name);
      try {
        accessSync(file, constants.X_OK);
        return file;
      } catch {
        // next
      }
    }
  }
  return null;
}

/** Runs the ErsatzTV script client like ErsatzTV does, for `ms`, and returns its stdout. */
export function runClient(url, data, ms, env = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(CLIENT, [url], { env: { ...process.env, ...env } });
    const out = [];
    child.stdout.on("data", (chunk) => out.push(chunk));
    child.on("error", reject);
    child.on("exit", () => resolve(Buffer.concat(out)));
    child.stdin.end(JSON.stringify(data));
    setTimeout(() => child.kill("SIGTERM"), ms);
  });
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

/**
 * Starts the sidecar with `config` written to a config file and `env` on top
 * of a clean environment; resolves once `/health` answers. Without a browser
 * in `env`, VIZRS_BROWSER is a program that exits at once: captures exist,
 * their frames stay transparent (as after a Chromium crash).
 */
export async function startSidecar(config = {}, env = {}) {
  const port = await freePort();
  const dir = mkdtempSync(path.join(os.tmpdir(), "virtrs-sidecar-"));
  const file = path.join(dir, "config.json");
  writeFileSync(file, JSON.stringify(config));

  const [command, ...args] = sidecarCommand();
  const child = spawn(command, [...args, file], {
    env: {
      PATH: process.env.PATH,
      HOME: dir,
      VIZRS_BROWSER: "/bin/false",
      ...env,
      LISTEN: `127.0.0.1:${port}`,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const output = [];
  child.stdout.on("data", (chunk) => output.push(chunk));
  child.stderr.on("data", (chunk) => output.push(chunk));
  const exited = new Promise((resolve) => child.once("exit", resolve));

  const url = `http://127.0.0.1:${port}`;
  const logs = () => Buffer.concat(output).toString();
  const stop = async () => {
    child.kill("SIGTERM");
    const timer = setTimeout(() => child.kill("SIGKILL"), 3000);
    await exited;
    clearTimeout(timer);
    rmSync(dir, { recursive: true, force: true });
    if (process.env.TRACE) process.stderr.write(logs());
  };
  const health = async () => (await fetch(`${url}/health`)).json();

  const deadline = Date.now() + START_TIMEOUT;
  for (;;) {
    try {
      await health();
      break;
    } catch {
      if (child.exitCode !== null || Date.now() > deadline) {
        await stop();
        throw new Error(`sidecar did not start:\n${logs()}`);
      }
      await sleep(50);
    }
  }

  return { url, port, pid: child.pid, health, logs, stop };
}
