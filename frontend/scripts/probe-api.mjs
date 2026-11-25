const base = "http://127.0.0.1:3001";
const paths = ["/swagger/openapi.json", "/api-docs/openapi.json", "/swagger.json", "/openapi.json"];
for (const p of paths) {
  const r = await fetch(base + p);
  console.log(p, r.status);
}
const html = await fetch(base + "/swagger/").then((r) => r.text());
const urls = [...html.matchAll(/["']([^"']*openapi[^"']*)["']/gi)].map((m) => m[1]);
console.log("urls", urls.slice(0, 8));
