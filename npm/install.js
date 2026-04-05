#!/usr/bin/env node
"use strict";

const { createHash } = require("crypto");
const { createWriteStream, chmodSync, existsSync, mkdirSync } = require("fs");
const { get } = require("https");
const { join } = require("path");
const { pipeline } = require("stream/promises");

const VERSION = "0.7.1";
const REPO = "Enreign/pagerunner";

const PLATFORMS = {
  "darwin-arm64": "pagerunner-macos-arm64",
  "darwin-x64": "pagerunner-macos-x86_64",
  "linux-x64": "pagerunner-linux-x86_64",
};

function getPlatformAsset() {
  const key = `${process.platform}-${process.arch}`;
  const asset = PLATFORMS[key];
  if (!asset) {
    console.error(
      `Unsupported platform: ${key}. Supported: ${Object.keys(PLATFORMS).join(", ")}`
    );
    process.exit(1);
  }
  return asset;
}

function download(url) {
  return new Promise((resolve, reject) => {
    get(url, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return download(res.headers.location).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
      }
      resolve(res);
    }).on("error", reject);
  });
}

async function fetchText(url) {
  const res = await download(url);
  const chunks = [];
  for await (const chunk of res) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8").trim();
}

async function main() {
  const asset = getPlatformAsset();
  const baseUrl = `https://github.com/${REPO}/releases/download/v${VERSION}`;
  const binDir = join(__dirname, "bin");
  const binPath = join(binDir, "pagerunner");

  if (existsSync(binPath)) {
    console.log("Pagerunner binary already installed.");
    return;
  }

  // Fetch expected SHA256
  const shaLine = await fetchText(`${baseUrl}/${asset}.sha256`);
  const expectedHash = shaLine.split(/\s+/)[0];

  // Download binary
  console.log(`Downloading ${asset} v${VERSION}...`);
  const res = await download(`${baseUrl}/${asset}`);

  mkdirSync(binDir, { recursive: true });
  const tmpPath = `${binPath}.tmp`;
  await pipeline(res, createWriteStream(tmpPath));

  // Verify SHA256
  const { readFileSync, renameSync, unlinkSync } = require("fs");
  const fileBuffer = readFileSync(tmpPath);
  const actualHash = createHash("sha256").update(fileBuffer).digest("hex");

  if (actualHash !== expectedHash) {
    unlinkSync(tmpPath);
    console.error(
      `SHA256 mismatch!\n  Expected: ${expectedHash}\n  Actual:   ${actualHash}`
    );
    process.exit(1);
  }

  renameSync(tmpPath, binPath);
  chmodSync(binPath, 0o755);
  console.log(`Pagerunner v${VERSION} installed successfully.`);
}

main().catch((err) => {
  console.error(`Failed to install Pagerunner: ${err.message}`);
  process.exit(1);
});
