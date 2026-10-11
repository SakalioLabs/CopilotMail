import {
  mkdir,
  mkdtemp,
  readdir,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { randomBytes } from "node:crypto";
import { tmpdir } from "node:os";
import {
  basename,
  dirname,
  join,
  normalize,
  relative,
  resolve,
} from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const budgets = Object.freeze({
  initialJavaScriptBytes: 360 * 1024,
  initialJavaScriptGzipBytes: 105 * 1024,
  totalJavaScriptBytes: 420 * 1024,
  cssBytes: 48 * 1024,
  cssGzipBytes: 12 * 1024,
  image2ContextRasterBytes: 120 * 1024,
  image2HeroRasterBytes: 300 * 1024,
});

/**
 * The total budget accounts for every production JavaScript chunk, including
 * lazily opened privileged workspaces. Admin has the larger audited control
 * plane surface; its first-load budget remains shared with Mail, while its
 * complete chunk graph receives a narrow, explicit allowance. P3 adds the
 * on-demand Agent Inspector and its Human-only, auditable mail-submission
 * evidence workspace to that complete graph; it does not change the shared
 * first-load budget. D3 adds the accessible full/compact/drawer shell and a
 * bounded decision surface without network-delivered visual dependencies;
 * its measured Admin stylesheet needs a narrow 53 KiB raw-CSS ceiling while
 * retaining the shared 12 KiB gzip ceiling. These Admin-only ceilings are
 * documented exceptions, not headroom that later work may consume silently.
 * Any further increase must be separately justified in design governance.
 */
const applicationBudgetOverrides = Object.freeze({
  "admin-app": Object.freeze({
    totalJavaScriptBytes: 480 * 1024,
    cssBytes: 53 * 1024,
  }),
});

const applications = Object.freeze(["admin-app", "mail-app"]);
const heroRasterPrefixes = Object.freeze(["mail-agent-routing-v1"]);

function budgetFor(application, key) {
  return applicationBudgetOverrides[application]?.[key] ?? budgets[key];
}

async function filesBelow(directory) {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await filesBelow(path));
    else if (entry.isFile()) result.push(path);
  }
  return result;
}

function assetPath(dist, owner, reference) {
  if (reference.startsWith("/")) return normalize(join(dist, reference.slice(1)));
  return normalize(resolve(dirname(owner), reference));
}

function displayPath(root, file) {
  return relative(root, file).replaceAll("\\", "/");
}

function isJavaScript(file) {
  return /\.[cm]?js$/iu.test(file);
}

function isImage2DeliveryRaster(file) {
  return /\.(?:avif|webp)$/iu.test(file);
}

function image2RasterBudget(file) {
  const name = basename(file).toLowerCase();
  return heroRasterPrefixes.some((prefix) => name.startsWith(prefix))
    ? budgets.image2HeroRasterBytes
    : budgets.image2ContextRasterBytes;
}

async function staticJavaScriptGraph(dist, entries) {
  const visited = new Set();
  const pending = [...entries];
  const staticImports = [
    /\bimport\s*["']([^"']+\.[cm]?js)["']/gu,
    /\b(?:import|export)(?!\s*\()[^;]*?\bfrom\s*["']([^"']+\.[cm]?js)["']/gu,
  ];
  while (pending.length) {
    const file = pending.pop();
    if (!file || visited.has(file)) continue;
    visited.add(file);
    const source = await readFile(file, "utf8");
    for (const pattern of staticImports) {
      for (const match of source.matchAll(pattern)) {
        const dependency = assetPath(dist, file, match[1]);
        if (!visited.has(dependency)) pending.push(dependency);
      }
    }
  }
  return visited;
}

function totalBytes(buffers) {
  return buffers.reduce((total, value) => total + value.byteLength, 0);
}

function totalGzipBytes(buffers) {
  return buffers.reduce(
    (total, value) => total + gzipSync(value, { level: 9 }).byteLength,
    0,
  );
}

function requireWithin(failures, label, actual, maximum) {
  if (actual > maximum) failures.push(`${label}: ${actual} > ${maximum} bytes`);
}

function exposesSourceMappingUrl(buffer) {
  return /sourceMappingURL\s*=/u.test(buffer.toString("utf8"));
}

async function checkApplication(application, distOverride) {
  const dist = distOverride ?? join(webRoot, application, "dist");
  const htmlPath = join(dist, "index.html");
  const [html, files] = await Promise.all([
    readFile(htmlPath, "utf8"),
    filesBelow(dist),
  ]);
  const failures = [];
  const sourceMaps = files.filter((file) => file.toLowerCase().endsWith(".map"));
  if (sourceMaps.length) {
    failures.push(`production output contains source maps: ${sourceMaps.map((file) => displayPath(dist, file)).join(", ")}`);
  }

  const allJavaScript = files.filter(isJavaScript);
  const allCss = files.filter((file) => file.toLowerCase().endsWith(".css"));
  const image2Rasters = files.filter(isImage2DeliveryRaster);
  if (!allJavaScript.length) failures.push("production output contains no JavaScript");
  if (!allCss.length) failures.push("production output contains no CSS");

  const scriptReferences = [...html.matchAll(/<script\b[^>]*>/gu)]
    .filter((match) => /\btype=["']module["']/u.test(match[0]))
    .map((match) => match[0].match(/\bsrc=["']([^"']+\.[cm]?js)["']/u)?.[1])
    .filter((reference) => reference !== undefined)
    .map((reference) => assetPath(dist, htmlPath, reference));
  if (!scriptReferences.length) failures.push("index.html contains no module entry script");
  const modulePreloadReferences = [...html.matchAll(/<link\b[^>]*>/giu)]
    .filter((match) => /\brel=["'][^"']*\bmodulepreload\b[^"']*["']/iu.test(match[0]))
    .map((match) => match[0].match(/\bhref=["']([^"']+\.[cm]?js)["']/iu)?.[1])
    .filter((reference) => reference !== undefined)
    .map((reference) => assetPath(dist, htmlPath, reference));
  const initialGraph = await staticJavaScriptGraph(
    dist,
    [...new Set([...scriptReferences, ...modulePreloadReferences])],
  );
  const initialBuffers = await Promise.all([...initialGraph].map((file) => readFile(file)));
  const javascriptBuffers = await Promise.all(allJavaScript.map((file) => readFile(file)));
  const cssBuffers = await Promise.all(allCss.map((file) => readFile(file)));

  for (const [index, file] of allJavaScript.entries()) {
    if (exposesSourceMappingUrl(javascriptBuffers[index])) {
      failures.push(`${displayPath(dist, file)} exposes a sourceMappingURL`);
    }
  }
  for (const [index, file] of allCss.entries()) {
    if (exposesSourceMappingUrl(cssBuffers[index])) {
      failures.push(`${displayPath(dist, file)} exposes a sourceMappingURL`);
    }
  }

  const image2RasterSizes = await Promise.all(
    image2Rasters.map(async (file) => (await readFile(file)).byteLength),
  );
  for (const [index, file] of image2Rasters.entries()) {
    requireWithin(
      failures,
      `Image 2 delivery raster ${displayPath(dist, file)}`,
      image2RasterSizes[index],
      image2RasterBudget(file),
    );
  }

  const evidence = {
    initialJavaScriptBytes: totalBytes(initialBuffers),
    initialJavaScriptGzipBytes: totalGzipBytes(initialBuffers),
    totalJavaScriptBytes: totalBytes(javascriptBuffers),
    cssBytes: totalBytes(cssBuffers),
    cssGzipBytes: totalGzipBytes(cssBuffers),
    image2DeliveryFiles: image2Rasters.length,
    largestImage2DeliveryBytes: Math.max(0, ...image2RasterSizes),
  };
  requireWithin(failures, "initial JavaScript", evidence.initialJavaScriptBytes, budgetFor(application, "initialJavaScriptBytes"));
  requireWithin(failures, "initial gzip JavaScript", evidence.initialJavaScriptGzipBytes, budgetFor(application, "initialJavaScriptGzipBytes"));
  requireWithin(failures, "total JavaScript", evidence.totalJavaScriptBytes, budgetFor(application, "totalJavaScriptBytes"));
  requireWithin(failures, "CSS", evidence.cssBytes, budgetFor(application, "cssBytes"));
  requireWithin(failures, "gzip CSS", evidence.cssGzipBytes, budgetFor(application, "cssGzipBytes"));
  return { application, evidence, failures };
}

async function writeSelfTestBundle(root, name, mutate) {
  const dist = join(root, name);
  const assets = join(dist, "assets");
  await mkdir(assets, { recursive: true });
  await Promise.all([
    writeFile(
      join(dist, "index.html"),
      '<!doctype html><link rel="stylesheet" href="/assets/app.css"><script type="module" src="/assets/app.js"></script>',
    ),
    writeFile(join(assets, "app.js"), "export const ready = true;\n"),
    writeFile(join(assets, "app.css"), ":root { color: CanvasText; }\n"),
  ]);
  await mutate({ assets, dist });
  return dist;
}

async function runSelfTest() {
  const temporaryRoot = await mkdtemp(join(tmpdir(), "copilotmail-bundle-budget-"));
  try {
    const validDist = await writeSelfTestBundle(temporaryRoot, "valid", async () => {});
    const valid = await checkApplication("self-test-valid", validDist);
    if (valid.failures.length) {
      throw new Error(`valid fixture was rejected: ${valid.failures.join("; ")}`);
    }

    const negativeFixtures = [
      {
        name: "map-file",
        expected: "production output contains source maps",
        mutate: ({ assets }) => writeFile(join(assets, "app.js.map"), "{}"),
      },
      {
        name: "javascript-inline-map",
        expected: "app.js exposes a sourceMappingURL",
        mutate: ({ assets }) => writeFile(join(assets, "app.js"), "//# sourceMappingURL=data:application/json;base64,e30=\n"),
      },
      {
        name: "css-inline-map",
        expected: "app.css exposes a sourceMappingURL",
        mutate: ({ assets }) => writeFile(join(assets, "app.css"), "/*# sourceMappingURL=data:application/json;base64,e30= */\n"),
      },
      {
        name: "oversized-webp",
        expected: "Image 2 delivery raster assets/context-v1.webp",
        mutate: ({ assets }) => writeFile(
          join(assets, "context-v1.webp"),
          Buffer.alloc(budgets.image2ContextRasterBytes + 1),
        ),
      },
      {
        name: "oversized-avif",
        expected: "Image 2 delivery raster assets/context-v1.avif",
        mutate: ({ assets }) => writeFile(
          join(assets, "context-v1.avif"),
          Buffer.alloc(budgets.image2ContextRasterBytes + 1),
        ),
      },
      {
        name: "initial-javascript-raw-budget",
        expected: "initial JavaScript",
        mutate: ({ assets }) => writeFile(
          join(assets, "app.js"),
          Buffer.alloc(budgets.initialJavaScriptBytes + 1, 0x61),
        ),
      },
      {
        name: "initial-javascript-gzip-budget",
        expected: "initial gzip JavaScript",
        mutate: ({ assets }) => writeFile(
          join(assets, "app.js"),
          `export default "${randomBytes(120 * 1024).toString("base64")}";`,
        ),
      },
      {
        name: "total-javascript-budget",
        expected: "total JavaScript",
        mutate: ({ assets }) => writeFile(
          join(assets, "unloaded.js"),
          Buffer.alloc(budgets.totalJavaScriptBytes + 1, 0x61),
        ),
      },
      {
        name: "css-raw-budget",
        expected: "CSS",
        mutate: ({ assets }) => writeFile(
          join(assets, "app.css"),
          Buffer.alloc(budgets.cssBytes + 1, 0x61),
        ),
      },
      {
        name: "css-gzip-budget",
        expected: "gzip CSS",
        mutate: ({ assets }) => writeFile(
          join(assets, "app.css"),
          `/*${randomBytes(14 * 1024).toString("base64")}*/`,
        ),
      },
      {
        name: "module-preload-budget",
        expected: "initial JavaScript",
        mutate: async ({ assets, dist }) => {
          await Promise.all([
            writeFile(
              join(assets, "preloaded.js"),
              Buffer.alloc(budgets.initialJavaScriptBytes + 1, 0x61),
            ),
            writeFile(
              join(dist, "index.html"),
              '<!doctype html><link rel="stylesheet" href="/assets/app.css">'
                + '<link rel="modulepreload" href="/assets/preloaded.js">'
                + '<script type="module" src="/assets/app.js"></script>',
            ),
          ]);
        },
      },
      {
        name: "missing-module-entry",
        expected: "index.html contains no module entry script",
        mutate: ({ dist }) => writeFile(
          join(dist, "index.html"),
          '<!doctype html><link rel="stylesheet" href="/assets/app.css">',
        ),
      },
      {
        name: "missing-static-import",
        expected: "missing.js",
        mutate: ({ assets }) => writeFile(
          join(assets, "app.js"),
          'import "./missing.js";\n',
        ),
      },
    ];

    for (const fixture of negativeFixtures) {
      const dist = await writeSelfTestBundle(
        temporaryRoot,
        fixture.name,
        fixture.mutate,
      );
      let fixtureFailures;
      try {
        const result = await checkApplication(`self-test-${fixture.name}`, dist);
        fixtureFailures = result.failures;
      } catch (error) {
        fixtureFailures = [error instanceof Error ? error.message : String(error)];
      }
      if (!fixtureFailures.some((failure) => failure.includes(fixture.expected))) {
        throw new Error(
          `${fixture.name} fixture did not fail closed; failures: ${fixtureFailures.join("; ") || "none"}`,
        );
      }
    }

    const totalWithinAdminOnly = await writeSelfTestBundle(
      temporaryRoot,
      "application-total-override",
      async ({ assets }) => writeFile(
        join(assets, "unloaded.js"),
        Buffer.alloc(440 * 1024, 0x61),
      ),
    );
    const adminOverride = await checkApplication("admin-app", totalWithinAdminOnly);
    if (adminOverride.failures.length) {
      throw new Error(`admin total-JS override was rejected: ${adminOverride.failures.join("; ")}`);
    }
    const mailDefault = await checkApplication("mail-app", totalWithinAdminOnly);
    if (!mailDefault.failures.some((failure) => failure.includes("total JavaScript"))) {
      throw new Error(`mail total-JS default was not enforced: ${mailDefault.failures.join("; ") || "none"}`);
    }

    const cssWithinAdminOnly = await writeSelfTestBundle(
      temporaryRoot,
      "application-css-override",
      async ({ assets }) => writeFile(
        join(assets, "app.css"),
        Buffer.alloc(52 * 1024, 0x61),
      ),
    );
    const adminCssOverride = await checkApplication("admin-app", cssWithinAdminOnly);
    if (adminCssOverride.failures.length) {
      throw new Error(`admin CSS override was rejected: ${adminCssOverride.failures.join("; ")}`);
    }
    const mailCssDefault = await checkApplication("mail-app", cssWithinAdminOnly);
    if (!mailCssDefault.failures.some((failure) => failure.includes("CSS"))) {
      throw new Error(`mail CSS default was not enforced: ${mailCssDefault.failures.join("; ") || "none"}`);
    }
    process.stdout.write(
      `bundle budget self-test: ok (${negativeFixtures.length} negative fixtures + application overrides)\n`,
    );
  } finally {
    await rm(temporaryRoot, { recursive: true, force: true });
  }
}

function usage() {
  return [
    "Usage: node scripts/check-bundle-budget.mjs [--application admin-app|mail-app]",
    "       node scripts/check-bundle-budget.mjs --self-test",
  ].join("\n");
}

function parseArguments(argv) {
  let selectedApplication;
  let applicationSpecified = false;
  let selfTest = false;
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--self-test") {
      selfTest = true;
    } else if (argument === "--application") {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        throw new Error("--application requires a value");
      }
      if (applicationSpecified) {
        throw new Error("--application may only be provided once");
      }
      applicationSpecified = true;
      selectedApplication = value;
      index += 1;
    } else if (argument.startsWith("--application=")) {
      if (applicationSpecified) {
        throw new Error("--application may only be provided once");
      }
      applicationSpecified = true;
      selectedApplication = argument.slice("--application=".length);
    } else if (argument === "--help" || argument === "-h") {
      process.stdout.write(`${usage()}\n`);
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${argument}`);
    }
  }
  if (selfTest && applicationSpecified) {
    throw new Error("--self-test cannot be combined with --application");
  }
  if (applicationSpecified && !applications.includes(selectedApplication)) {
    throw new Error(`unsupported application: ${selectedApplication}`);
  }
  return {
    selectedApplications: selectedApplication ? [selectedApplication] : applications,
    selfTest,
  };
}

async function main() {
  const { selectedApplications, selfTest } = parseArguments(process.argv.slice(2));
  if (selfTest) {
    await runSelfTest();
    return;
  }
  const results = await Promise.all(selectedApplications.map(async (application) => {
    try {
      return await checkApplication(application);
    } catch (error) {
      return {
        application,
        evidence: {},
        failures: [`could not inspect production output: ${error instanceof Error ? error.message : String(error)}`],
      };
    }
  }));
  for (const { application, evidence, failures } of results) {
    const summary = Object.entries(evidence)
      .map(([key, value]) => `${key}=${value}`)
      .join(" ");
    process.stdout.write(`${application}: ${summary}\n`);
    for (const failure of failures) process.stderr.write(`${application}: ${failure}\n`);
  }
  if (results.some(({ failures }) => failures.length)) process.exitCode = 1;
}

main().catch((error) => {
  process.stderr.write(`bundle budget check failed: ${error instanceof Error ? error.message : String(error)}\n${usage()}\n`);
  process.exitCode = 1;
});
