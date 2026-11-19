const { copyFileSync, mkdirSync } = require("node:fs");
const { join } = require("node:path");

const root = join(__dirname, "..");
for (const [source, destination] of [
  ["src/renderer/index.html", "build/renderer/index.html"],
  ["src/renderer/workbench.css", "build/renderer/workbench.css"],
  ["src/server/schema.sql", "build/server/schema.sql"],
]) {
  mkdirSync(join(root, destination, ".."), { recursive: true });
  copyFileSync(join(root, source), join(root, destination));
}
