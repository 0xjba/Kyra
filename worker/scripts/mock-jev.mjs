// Local stand-in for the JEV scoring API, for use with `wrangler dev` only.
// Scores are deterministic: derived from the size stated in each question.
import { createServer } from "node:http";

const port = Number(process.env.MOCK_JEV_PORT || 8788);

function scoreFor(question) {
  const m = /has ([\d.]+) (KB|MB|GB) cleanable/.exec(question);
  if (!m) return 50;
  const mb = Number(m[1]) * { KB: 1 / 1024, MB: 1, GB: 1024 }[m[2]];
  if (mb >= 1024) return 90;
  if (mb >= 100) return 55;
  return 20;
}

createServer((req, res) => {
  let raw = "";
  req.on("data", (c) => (raw += c));
  req.on("end", () => {
    let questions = [];
    try {
      questions = JSON.parse(raw).questions ?? [];
    } catch {}
    const scores = questions.map((q) => ({ score: scoreFor(q), confidence: 0.9 }));
    console.log(`[mock-jev] ${req.method} ${req.url} auth=${req.headers.authorization ? "yes" : "no"} -> ${scores.map((s) => s.score).join(",")}`);
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ scores }));
  });
}).listen(port, "127.0.0.1", () => {
  console.log(`[mock-jev] listening on http://127.0.0.1:${port}/score`);
});
