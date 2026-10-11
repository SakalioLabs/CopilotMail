import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import ts from "typescript";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

function filesUnder(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? filesUnder(file) : [file];
  });
}

function inspect(file) {
  const source = fs.readFileSync(file, "utf8");
  const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
  if (tree.parseDiagnostics.length) throw new Error(`Cannot parse ${path.relative(root, file)}`);
  const functions = [];
  function visit(node) {
    if ((ts.isFunctionDeclaration(node) || ts.isFunctionExpression(node)
        || ts.isArrowFunction(node) || ts.isMethodDeclaration(node)
        || ts.isGetAccessorDeclaration(node) || ts.isSetAccessorDeclaration(node)
        || ts.isConstructorDeclaration(node)) && node.body) {
      const line = tree.getLineAndCharacterOfPosition(node.getStart(tree)).line + 1;
      const endLine = tree.getLineAndCharacterOfPosition(node.end).line + 1;
      const name = node.name?.getText(tree)
        ?? (ts.isVariableDeclaration(node.parent) ? node.parent.name.getText(tree) : `callback@${line}`);
      functions.push({ name, line, end_line: endLine, source_lines: endLine - line + 1 });
    }
    ts.forEachChild(node, visit);
  }
  visit(tree);
  return { path: path.relative(root, file).replaceAll("\\", "/"),
    sha256: createHash("sha256").update(source.replaceAll("\r\n", "\n")).digest("hex"),
    source_lines: source.split("\n").length, functions };
}

const files = ["admin-app", "mail-app", "design-system"].flatMap((application) =>
  filesUnder(path.join(root, "web", application, "src"))
    .filter((file) => /\.tsx?$/.test(file) && !/\.test\.tsx?$|\.d\.ts$/.test(file)
      && !file.split(path.sep).includes("test"))).sort().map(inspect);
const report = { schema_version: 1, kind: "typescript_ast_function_inventory_not_test_coverage", source_hash_encoding: "UTF-8 normalized LF",
  file_count: files.length, function_count: files.reduce((sum, file) => sum + file.functions.length, 0), files };
const output = path.join(root, "quality/reports/typescript-function-inventory.json");
fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
process.stdout.write(`Inventoried ${report.file_count} TypeScript files / ${report.function_count} functions; no review completion inferred\n`);

// Literal Vitest test titles, parsed from code rather than matching comments.
// Parameterized/generated titles remain outside this deliberately modest index.
const tests = ["admin-app", "mail-app", "design-system"].flatMap((application) =>
  filesUnder(path.join(root, "web", application, "src")).filter((file) => /\.test\.tsx?$/.test(file)))
  .sort().map((file) => {
    const source = fs.readFileSync(file, "utf8");
    const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
    if (tree.parseDiagnostics.length) throw new Error(`Cannot parse ${file}`);
    const titles = [];
    function visit(node) {
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)
          && ["it", "test"].includes(node.expression.text)
          && node.arguments.length >= 2 && ts.isStringLiteral(node.arguments[0])
          && (ts.isArrowFunction(node.arguments[1]) || ts.isFunctionExpression(node.arguments[1]))) {
        titles.push({ name: node.arguments[0].text, line: tree.getLineAndCharacterOfPosition(node.getStart(tree)).line + 1 });
      }
      ts.forEachChild(node, visit);
    }
    visit(tree);
    return { path: path.relative(root, file).replaceAll("\\", "/"),
      sha256: createHash("sha256").update(source.replaceAll("\r\n", "\n")).digest("hex"), tests: titles };
  });
fs.writeFileSync(path.join(root, "quality/reports/typescript-test-inventory.json"),
  `${JSON.stringify({ schema_version: 1, kind: "literal_test_declarations_not_execution_evidence", source_hash_encoding: "UTF-8 normalized LF", files: tests }, null, 2)}\n`);
