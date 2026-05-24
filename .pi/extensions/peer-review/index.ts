// FLUI Peer Review extension
//
// Registers /peer commands that dispatch the current question to other AI coding agents
// installed on this workstation (codex, gemini, copilot, claude) and capture their replies
// to .peer-review/ for synthesis by the primary Pi session.
//
// Usage:
//   /peer codex <prompt>          → ask OpenAI Codex for a second opinion
//   /peer gemini <prompt>         → ask Google Gemini (great for huge context)
//   /peer copilot <prompt>        → ask GitHub Copilot (PR-style reviews)
//   /peer claude <prompt>         → ask Claude Code
//   /peer all <prompt>            → broadcast to codex+gemini+claude in parallel
//   /peer review                  → run `codex exec review` against the repo
//
// Result files land in .peer-review/<agent>-<timestamp>.md and are surfaced to the model
// via pi.sendUserMessage so it can synthesize the answers in the next turn.

import type { ExtensionAPI, ExtensionCommandContext } from "@earendil-works/pi-coding-agent";
import { promises as fs } from "node:fs";
import { join } from "node:path";

type Peer = "codex" | "gemini" | "copilot" | "claude";

interface PeerSpec {
  bin: string;
  args: (prompt: string) => string[];
  /** Whether to feed prompt via stdin instead of argv (avoids quoting issues for long prompts). */
  stdin?: boolean;
}

const PEERS: Record<Peer, PeerSpec> = {
  codex:   { bin: "codex",   args: (_p) => ["exec", "-"],                         stdin: true },
  gemini:  { bin: "gemini",  args: (p)  => ["-p", p, "--approval-mode", "plan"]                },
  copilot: { bin: "copilot", args: (p)  => ["-p", p, "--allow-all-tools"]                      },
  claude:  { bin: "claude",  args: (p)  => ["-p", p]                                            },
};

function timestamp(): string {
  const d = new Date();
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;
}

async function ensureDir(cwd: string): Promise<string> {
  const dir = join(cwd, ".peer-review");
  await fs.mkdir(dir, { recursive: true });
  return dir;
}

async function callPeer(
  pi: ExtensionAPI,
  ctx: ExtensionCommandContext,
  peer: Peer,
  prompt: string,
): Promise<{ file: string; ok: boolean; preview: string }> {
  const spec = PEERS[peer];
  const dir = await ensureDir(ctx.cwd);
  const file = join(dir, `${peer}-${timestamp()}.md`);

  ctx.ui.notify(`peer:${peer} → running…`, "info");

  const result = await pi.exec(spec.bin, spec.args(prompt), {
    signal: ctx.signal,
    timeout: 5 * 60_000,
    cwd: ctx.cwd,
    stdin: spec.stdin ? prompt : undefined,
  } as any);

  const body = [
    `# peer:${peer}`,
    `prompt:\n\n${prompt}\n`,
    `---`,
    `stdout:\n\n${result.stdout || "(empty)"}\n`,
    result.stderr ? `---\nstderr:\n\n${result.stderr}\n` : "",
    `---\nexit code: ${result.code}`,
  ].join("\n\n");
  await fs.writeFile(file, body, "utf8");

  const ok = result.code === 0;
  const preview = (result.stdout || result.stderr || "").slice(0, 400);
  ctx.ui.notify(
    `peer:${peer} ${ok ? "✓" : "✗"} → ${file}`,
    ok ? "success" : "error",
  );
  return { file, ok, preview };
}

function parsePeerArgs(args: string): { peer: Peer | "all" | "review" | null; prompt: string } {
  const trimmed = args.trim();
  if (!trimmed) return { peer: null, prompt: "" };
  const [head, ...rest] = trimmed.split(/\s+/);
  const prompt = rest.join(" ").trim();
  if (head === "all" || head === "review") return { peer: head, prompt };
  if (head in PEERS) return { peer: head as Peer, prompt };
  return { peer: null, prompt: trimmed };
}

export default function (pi: ExtensionAPI) {
  pi.registerCommand("peer", {
    description: "Ask another AI agent (codex|gemini|copilot|claude|all|review) for a second opinion",
    getArgumentCompletions: (prefix: string) => {
      const opts = ["codex", "gemini", "copilot", "claude", "all", "review"];
      const matches = opts
        .filter((o) => o.startsWith(prefix))
        .map((o) => ({ value: o, label: o }));
      return matches.length > 0 ? matches : null;
    },
    handler: async (args, ctx) => {
      const { peer, prompt } = parsePeerArgs(args);

      if (!peer) {
        ctx.ui.notify(
          "Usage: /peer <codex|gemini|copilot|claude|all|review> <prompt>",
          "warning",
        );
        return;
      }

      if (peer === "review") {
        // codex has built-in repo review mode
        const dir = await ensureDir(ctx.cwd);
        const file = join(dir, `codex-review-${timestamp()}.md`);
        ctx.ui.notify("peer: running `codex exec review` on the repo…", "info");
        const result = await pi.exec("codex", ["exec", "review"], {
          signal: ctx.signal,
          timeout: 10 * 60_000,
          cwd: ctx.cwd,
        } as any);
        await fs.writeFile(
          file,
          `# codex exec review\n\n${result.stdout || ""}\n\n---\nexit ${result.code}`,
          "utf8",
        );
        ctx.ui.notify(`peer:codex review → ${file}`, result.code === 0 ? "success" : "error");
        await pi.sendUserMessage(
          `Codex finished a repo review. Read \`${file}\` and summarize the top findings.`,
          { sender: "extension" } as any,
        );
        return;
      }

      if (!prompt) {
        ctx.ui.notify(`No prompt supplied. Usage: /peer ${peer} <prompt>`, "warning");
        return;
      }

      if (peer === "all") {
        const results = await Promise.all(
          (["codex", "gemini", "claude"] as Peer[]).map((p) => callPeer(pi, ctx, p, prompt)),
        );
        const list = results.map((r) => `- \`${r.file}\` ${r.ok ? "✓" : "✗"}`).join("\n");
        await pi.sendUserMessage(
          [
            `Three peer agents answered the question:`,
            `> ${prompt}`,
            ``,
            `Their full replies are saved to:`,
            list,
            ``,
            `Read each file, then synthesize: where do they AGREE, where do they DISAGREE,`,
            `and what is your final recommendation? Be concise.`,
          ].join("\n"),
          { sender: "extension" } as any,
        );
        return;
      }

      const { file, ok } = await callPeer(pi, ctx, peer, prompt);
      await pi.sendUserMessage(
        `Peer agent \`${peer}\` answered. Reply saved to \`${file}\`.\n` +
          `${ok ? "" : "(Exit was non-zero — check stderr.)\n"}` +
          `Read it and tell me: (a) the key insight, (b) what to do next.`,
        { sender: "extension" } as any,
      );
    },
  });

  pi.on("session_start" as any, async (_e: any, ctx: any) => {
    ctx?.ui?.notify?.("peer-review extension loaded · /peer codex|gemini|copilot|claude|all|review", "info");
  });
}
