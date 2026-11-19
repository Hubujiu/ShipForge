export type Reply<T> =
  | { protocol: 1; ok: true; data: T }
  | { protocol: 1; ok: false; error: { code: string; message: string } };

export function originOf(address: string) {
  const url = new URL(address.trim());
  const loopback = ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname);
  if (url.username || url.password || url.search || url.hash || url.pathname !== "/"
      || (url.protocol !== "https:" && !(url.protocol === "http:" && loopback))) {
    throw new Error("Use an HTTPS service address or an HTTP address on this machine");
  }
  return url.origin;
}

export async function request<T>(origin: string, path: string, input?: unknown, token?: string) {
  const response = await fetch(`${originOf(origin)}${path}`, {
    method: input === undefined ? "GET" : "POST",
    headers: {
      Accept: "application/json",
      ...(input === undefined ? {} : { "Content-Type": "application/json" }),
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: input === undefined ? undefined : JSON.stringify(input),
    redirect: "error",
    signal: AbortSignal.timeout(12_000),
  });
  const result = await response.json() as Reply<T>;
  if (result.protocol !== 1 || typeof result.ok !== "boolean") throw new Error("Unexpected service response");
  if (!result.ok) throw new Error(result.error.message);
  return result.data;
}
