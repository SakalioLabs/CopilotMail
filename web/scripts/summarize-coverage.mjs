import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import coverage from "istanbul-lib-coverage";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const merged = coverage.createCoverageMap({});
const inputs = [];
for (const application of ["web-design-system", "web-admin", "web-mail"]) {
  const relative = `quality/coverage/${application}/coverage-final.json`;
  const bytes = fs.readFileSync(path.join(root, relative));
  merged.merge(JSON.parse(bytes.toString("utf8")));
  inputs.push({ path: relative, sha256: createHash("sha256").update(bytes).digest("hex") });
}

const files = merged.files().sort().map((filename) => {
  const relative = path.relative(root, filename).replaceAll("\\", "/");
  if (relative.startsWith("../") || !relative.startsWith("web/")) throw new Error("Coverage escaped workspace");
  const record = merged.fileCoverageFor(filename);
  const unexecuted = Object.entries(record.f)
    .filter(([, hits]) => hits === 0)
    .map(([id]) => ({ name: record.fnMap[id].name, line: record.fnMap[id].loc.start.line }));
  return { path: relative, source_sha256: createHash("sha256").update(fs.readFileSync(filename)).digest("hex"),
    summary: record.toSummary().toJSON(), unexecuted_functions: unexecuted };
});
const result = { schema_version: 1, kind: "measured_v8_coverage_merged_across_workspaces",
  notes: ["Shared components retain hits from Admin, Mail, and their own tests", "Execution coverage is not assertion quality or final review approval"],
  inputs, totals: merged.getCoverageSummary().toJSON(), files };
const destination = path.join(root, "quality/reports/web-coverage.json");
fs.mkdirSync(path.dirname(destination), { recursive: true });
fs.writeFileSync(destination, `${JSON.stringify(result, null, 2)}\n`);
process.stdout.write(`Web coverage: ${files.length} files, ${result.totals.lines.pct}% lines, ${result.totals.functions.pct}% functions\n`);
