import { readFileSync } from "node:fs";
import { resolve } from "node:path";

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

const root = process.cwd();
const policyPath = resolve(root, "security/js-dependency-policy.json");
const pkgPath = resolve(root, "apps/dashboard/package.json");
const lockPath = resolve(root, "pnpm-lock.yaml");

const policy = readJson(policyPath);
const pkg = readJson(pkgPath);
const lockRaw = readFileSync(lockPath, "utf8");

function fail(message) {
  console.error(`Dependency policy check failed: ${message}`);
  process.exit(1);
}

function assertPinnedSet(current, allowed, label) {
  const currentKeys = Object.keys(current ?? {}).sort();
  const allowedKeys = Object.keys(allowed ?? {}).sort();

  if (JSON.stringify(currentKeys) !== JSON.stringify(allowedKeys)) {
    fail(
      `${label} keys mismatch. expected=${allowedKeys.join(",")} got=${currentKeys.join(",")}`,
    );
  }

  for (const [name, version] of Object.entries(allowed)) {
    if (current[name] !== version) {
      fail(
        `${label} version mismatch for ${name}. expected=${version} got=${current[name]}`,
      );
    }
  }
}

assertPinnedSet(
  pkg.dependencies,
  policy.allowedDirectDependencies,
  "dependencies",
);
assertPinnedSet(
  pkg.devDependencies,
  policy.allowedDirectDevDependencies,
  "devDependencies",
);

for (const bannedPattern of policy.bannedVersionPatterns ?? []) {
  if (lockRaw.includes(bannedPattern)) {
    fail(`banned package pattern found in lockfile: ${bannedPattern}`);
  }
}

console.log("Dependency policy check passed.");
