import { execFile } from "child_process";
import { promisify } from "util";
import { getPreferenceValues, showToast, Toast } from "@raycast/api";
import type { Preferences } from "./types";

const execFileAsync = promisify(execFile);

function getBinaryPath(): string {
  const prefs = getPreferenceValues<Preferences>();
  return prefs.pagerunnerPath || "pagerunner";
}

export async function execCommand(subcommand: string, args: string[] = []): Promise<string> {
  const bin = getBinaryPath();
  try {
    const { stdout } = await execFileAsync(bin, [subcommand, ...args], {
      timeout: 30_000,
      maxBuffer: 10 * 1024 * 1024,
    });
    return stdout;
  } catch (error: unknown) {
    const err = error as { stderr?: string; message?: string };
    const message = err.stderr?.trim() || err.message || "Unknown error";
    await showToast({ style: Toast.Style.Failure, title: `pagerunner ${subcommand} failed`, message });
    throw new Error(message);
  }
}

export async function execCommandJson<T>(subcommand: string, args: string[] = []): Promise<T> {
  const stdout = await execCommand(subcommand, args);
  return JSON.parse(stdout) as T;
}

export async function ensureDaemon(): Promise<void> {
  const prefs = getPreferenceValues<Preferences>();
  if (!prefs.daemonAutoStart) return;

  try {
    await execCommand("list-sessions");
  } catch {
    const bin = getBinaryPath();
    const { spawn } = await import("child_process");
    const child = spawn(bin, ["daemon"], {
      detached: true,
      stdio: "ignore",
    });
    child.unref();
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
}
