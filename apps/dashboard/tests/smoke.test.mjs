import test from "node:test";
import assert from "node:assert/strict";

test("dashboard package metadata exists", async () => {
  const pkg = await import("../package.json", { with: { type: "json" } });
  assert.equal(pkg.default.name, "sentinel-dashboard");
});
