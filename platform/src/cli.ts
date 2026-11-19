import { readFile } from "node:fs/promises";
import { text } from "node:stream/consumers";
import { z } from "zod";
import { commandSchema } from "./protocol";
import { request } from "./transport";

async function cli() {
  const [command, file] = process.argv.slice(2);
  if (command === "schema") return z.toJSONSchema(commandSchema);
  if (command === "--help" || !command) return {
    usage: "shipforge <setup|login|join|request|schema> [file]",
    address: "SHIPFORGE_SERVER defaults to http://127.0.0.1:4790",
    auth: "Set SHIPFORGE_TOKEN for authenticated requests. Input defaults to UTF-8 stdin.",
  };
  const paths = { setup: "/v1/setup", login: "/v1/login", join: "/v1/join", request: "/v1/command" };
  if (!Object.hasOwn(paths, command)) throw new Error("Unknown command. See --help");
  const input = JSON.parse(file ? await readFile(file, "utf8") : await text(process.stdin));
  const data = command === "request" ? commandSchema.parse(input) : input;
  return request(process.env.SHIPFORGE_SERVER ?? "http://127.0.0.1:4790", paths[command as keyof typeof paths], data, process.env.SHIPFORGE_TOKEN);
}

void cli().then((data) => {
  console.log(JSON.stringify({ protocol: 1, ok: true, data }, null, 2));
}).catch((error: Error) => {
  console.log(JSON.stringify({ protocol: 1, ok: false, error: { message: error.message } }));
  process.exitCode = 1;
});
