#!/usr/bin/env node
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
let ts;
try {
  ts = require("../web/node_modules/typescript/lib/typescript.js");
} catch {
  console.error(
    "web i18n AST gate failed: unable to load the repository TypeScript compiler; "
    + "run npm ci in web first",
  );
  process.exit(1);
}
const ROOT = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const SOURCE_ROOT = path.join(ROOT, "web", "mail-app", "src");
const VISIBLE_ATTRIBUTES = new Set([
  "alt",
  "aria-description",
  "aria-label",
  "aria-placeholder",
  "aria-roledescription",
  "aria-valuetext",
  "caption",
  "description",
  "emptytext",
  "errormessage",
  "helpertext",
  "label",
  "placeholder",
  "successmessage",
  "title",
  "tooltip",
]);

class I18nAstError extends Error {}

const TEST_DIRECTORY_NAMES = new Set(["test", "tests", "__tests__"]);
const NON_COPY_LITERALS = new Set([
  "API",
  "C",
  "CopilotMail",
  "CSS",
  "HTML",
  "HTTP",
  "HTTPS",
  "ID",
  "IMAP",
  "JSON",
  "KB",
  "MB",
  "MIME",
  "OAuth",
  "OIDC",
  "SMTP",
  "URI",
  "URL",
  "UTC",
  "UUID",
  "XML",
]);
const LETTER_PATTERN = /\p{L}/u;
const HTML_ENTITY_PATTERN = /&(?:#\d+|#x[\da-f]+|[a-z][\da-z]+);/giu;
const MACHINE_VALUE_PATTERNS = [
  /^(?:https?|mailto):\S+$/iu,
  /^[\w.+-]+@[\w.-]+\.[a-z]{2,}$/iu,
  /^(?:\.{0,2}[\/]|[a-z]:[\/])\S+$/iu,
  /^[a-z][\w.+-]*\/[\w.+-]+$/u,
  /^[a-z]{2,3}(?:-[A-Z][a-z]{3}|-[A-Z]{2}|-\d{3})$/u,
  /^(?:v)?\d+(?:\.\d+){1,3}(?:[-+][\w.-]+)?$/iu,
  /^[\da-f]{8}-(?:[\da-f]{4}-){3}[\da-f]{12}$/iu,
  /^[a-z][a-z\d]*(?:[._:][a-z\d]+)+$/u,
];

function isNonCopyLiteral(value) {
  const normalized = normalizedVisibleText(value);
  if (!normalized) return true;
  const withoutEntities = normalized.replace(HTML_ENTITY_PATTERN, "");
  if (!withoutEntities || !LETTER_PATTERN.test(withoutEntities)) return true;
  if (NON_COPY_LITERALS.has(normalized)) return true;
  return MACHINE_VALUE_PATTERNS.some((pattern) => pattern.test(normalized));
}

function isProductionSource(filePath, sourceRoot = SOURCE_ROOT) {
  const relative = path.relative(sourceRoot, filePath);
  if (relative === ".." || relative.startsWith(".." + path.sep) || path.isAbsolute(relative)) return false;
  const parts = relative.split(path.sep);
  const baseName = parts.at(-1).toLowerCase();
  if (!(baseName.endsWith(".ts") || baseName.endsWith(".tsx"))) return false;
  if (baseName === "i18n.ts" || baseName.endsWith(".d.ts")) return false;
  if (baseName.includes(".test.") || baseName.includes(".spec.")) return false;
  return !parts.slice(0, -1).some(
    (part) => TEST_DIRECTORY_NAMES.has(part.toLowerCase()),
  );
}

function productionSources(directory) {
  const files = [];
  const visit = (current) => {
    const entries = fs.readdirSync(current, { withFileTypes: true })
      .sort((left, right) => left.name.localeCompare(right.name, "en"));
    for (const entry of entries) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        if (!TEST_DIRECTORY_NAMES.has(entry.name.toLowerCase())) visit(fullPath);
      } else if (entry.isFile() && isProductionSource(fullPath, directory)) {
        files.push(fullPath);
      }
    }
  };
  visit(directory);
  return files;
}

function normalizedVisibleText(value) {
  return value.replace(/\s+/g, " ").trim();
}

function attributeName(attribute) {
  const name = ts.isIdentifier(attribute.name)
    ? attribute.name.text
    : attribute.name.getText();
  return name.toLowerCase();
}

function scanSource(sourceFile) {
  const violations = [];
  const record = (node, category, value) => {
    const text = normalizedVisibleText(value);
    if (isNonCopyLiteral(text)) return;
    const position = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile));
    violations.push({
      category,
      column: position.character + 1,
      line: position.line + 1,
      text,
    });
  };

  const scanVisibleExpression = (expression, category) => {
    if (!expression) return;
    if (ts.isParenthesizedExpression(expression)
      || ts.isAsExpression(expression)
      || ts.isTypeAssertionExpression(expression)
      || ts.isSatisfiesExpression(expression)
      || ts.isNonNullExpression(expression)) {
      scanVisibleExpression(expression.expression, category);
      return;
    }
    if (ts.isStringLiteral(expression) || ts.isNoSubstitutionTemplateLiteral(expression)) {
      record(expression, category, expression.text);
      return;
    }
    if (ts.isTemplateExpression(expression)) {
      record(expression.head, category, expression.head.text);
      for (const span of expression.templateSpans) {
        scanVisibleExpression(span.expression, category);
        record(span.literal, category, span.literal.text);
      }
      return;
    }
    if (ts.isConditionalExpression(expression)) {
      scanVisibleExpression(expression.whenTrue, category);
      scanVisibleExpression(expression.whenFalse, category);
      return;
    }
    if (ts.isArrayLiteralExpression(expression)) {
      for (const element of expression.elements) {
        scanVisibleExpression(element, category);
      }
      return;
    }
    if (!ts.isBinaryExpression(expression)) return;
    const kind = expression.operatorToken.kind;
    if (kind === ts.SyntaxKind.PlusToken
      || kind === ts.SyntaxKind.BarBarToken
      || kind === ts.SyntaxKind.QuestionQuestionToken) {
      scanVisibleExpression(expression.left, category);
      scanVisibleExpression(expression.right, category);
    } else if (kind === ts.SyntaxKind.AmpersandAmpersandToken
      || kind === ts.SyntaxKind.CommaToken) {
      scanVisibleExpression(expression.right, category);
    }
  };

  const scanAttributes = (attributes) => {
    for (const property of attributes.properties) {
      if (!ts.isJsxAttribute(property) || !VISIBLE_ATTRIBUTES.has(attributeName(property))) continue;
      const initializer = property.initializer;
      if (!initializer) continue;
      if (ts.isStringLiteral(initializer)) {
        record(initializer, `hard-coded ${attributeName(property)} attribute`, initializer.text);
      } else if (ts.isJsxExpression(initializer)) {
        scanVisibleExpression(
          initializer.expression,
          `hard-coded ${attributeName(property)} expression`,
        );
      }
    }
  };

  const visit = (node) => {
    if (ts.isJsxElement(node)) {
      scanAttributes(node.openingElement.attributes);
      for (const child of node.children) visitJsxChild(child);
      return;
    }
    if (ts.isJsxSelfClosingElement(node)) {
      scanAttributes(node.attributes);
      return;
    }
    if (ts.isJsxFragment(node)) {
      for (const child of node.children) visitJsxChild(child);
      return;
    }
    ts.forEachChild(node, visit);
  };

  const visitJsxChild = (child) => {
    if (ts.isJsxText(child)) {
      record(child, "hard-coded JSX text", child.text);
      return;
    }
    if (ts.isJsxExpression(child)) {
      scanVisibleExpression(child.expression, "hard-coded JSX expression");
      if (child.expression) visit(child.expression);
      return;
    }
    visit(child);
  };

  visit(sourceFile);
  return violations;
}

function parseSource(name, source) {
  const kind = name.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const sourceFile = ts.createSourceFile(name, source, ts.ScriptTarget.Latest, true, kind);
  const diagnostics = sourceFile.parseDiagnostics ?? [];
  if (diagnostics.length > 0) throw new I18nAstError(`unable to parse ${name}`);
  return sourceFile;
}

function validate() {
  if (!fs.existsSync(SOURCE_ROOT)) throw new I18nAstError("mail-app source directory is missing");
  const violations = [];
  for (const filePath of productionSources(SOURCE_ROOT)) {
    const sourceFile = parseSource(filePath, fs.readFileSync(filePath, "utf8"));
    for (const violation of scanSource(sourceFile)) {
      violations.push(
        `${path.relative(ROOT, filePath).split(path.sep).join("/")}`
        + `:${violation.line}:${violation.column} ${violation.category}`
        + ` ${JSON.stringify(violation.text)}`,
      );
    }
  }
  if (violations.length > 0) {
    throw new I18nAstError(
      "visible production JSX literals must come from i18n.ts:\n" + violations.join("\n"),
    );
  }
}

function selfTest() {
  const safe = parseSource("safe.tsx", `
    const t = (key: string) => key;
    declare const dynamicPlaceholder: string;
    export const Safe = () => <>
      <p>{t("nav.inbox")}</p>
      <span aria-hidden="true">C</span>
      <span>{"?"}</span>
      <strong>CopilotMail</strong>
      <code>{"message_id"}</code>
      <code>UUID</code>
      <img alt="" src="/logo.svg" />
      <input
        aria-label={t("inbox.searchAria")}
        placeholder={dynamicPlaceholder}
        title="v1.2.3"
      />
    </>;
  `);
  const safeViolations = scanSource(safe);
  if (safeViolations.length !== 0) {
    throw new I18nAstError(
      "AST self-test rejected catalog, symbol, brand, or technical values: "
      + safeViolations.map((item) => item.text).join(", "),
    );
  }

  const unsafeCases = [
    ["JSX text", "export const View = () => <p>Delete message</p>;"],
    ["string-literal child", "export const View = () => <p>{\"Inbox\"}</p>;"],
    ["aria-hidden text", "export const View = () => <span aria-hidden=\"true\">Delete</span>;"],
    ["aria-label", "export const View = () => <button aria-label=\"Delete message\" />;"],
    ["title", "export const View = () => <button title={ready ? \"Disable alerts\" : \"Enable alerts\"} />;"],
    ["placeholder", "export const View = () => <input placeholder=\"Search mail\" />;"],
    ["alt", "export const View = () => <img alt=\"Profile photo\" />;"],
    ["template attribute", "export const View = () => <input placeholder={`Type a subject`} />;"],
  ];
  for (const [label, source] of unsafeCases) {
    if (scanSource(parseSource(`${label}.tsx`, source)).length === 0) {
      throw new I18nAstError(`AST self-test missed ${label}`);
    }
  }

  const excludedPaths = [
    path.join(SOURCE_ROOT, "App.test.tsx"),
    path.join(SOURCE_ROOT, "Widget.spec.tsx"),
    path.join(SOURCE_ROOT, "test", "setup.ts"),
    path.join(SOURCE_ROOT, "tests", "fixture.tsx"),
    path.join(SOURCE_ROOT, "__tests__", "fixture.tsx"),
    path.join(SOURCE_ROOT, "i18n.ts"),
  ];
  if (!isProductionSource(path.join(SOURCE_ROOT, "App.tsx"))
    || excludedPaths.some((filePath) => isProductionSource(filePath))) {
    throw new I18nAstError("AST self-test failed production/test source classification");
  }
}

try {
  if (process.argv.includes("--self-test")) selfTest();
  validate();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`web i18n AST gate failed: ${message}`);
  process.exitCode = 1;
}
