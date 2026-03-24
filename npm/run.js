#!/usr/bin/env node
"use strict";

const { execFileSync } = require("child_process");
const { join } = require("path");

const bin = join(__dirname, "bin", "pagerunner");

try {
  execFileSync(bin, process.argv.slice(2), { stdio: "inherit" });
} catch (err) {
  process.exit(err.status || 1);
}
