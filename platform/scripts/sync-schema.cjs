const { readFileSync, writeFileSync } = require("node:fs");
const { join } = require("node:path");
const { z } = require("zod");
const { commandSchema } = require("../build/protocol.js");

const filename = join(__dirname, "../../docs/electron-protocol.schema.json");
const schema = JSON.stringify(z.toJSONSchema(commandSchema), null, 2) + "\n";
if (process.argv.includes("--check")) {
  const current = readFileSync(filename, "utf8").replace(/\r\n/g, "\n");
  if (current !== schema) {
    console.error("Regenerate the Electron CLI schema with npm run schema.");
    process.exitCode = 1;
  }
} else {
  writeFileSync(filename, schema);
}
