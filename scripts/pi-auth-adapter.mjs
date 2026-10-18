#!/usr/bin/env node
// Adaptador de autenticação do Dish para o Pi.
//
// O Dish não reimplementa autenticação: o Pi é dono do OAuth, do refresh e do
// armazenamento. Este processo é a fronteira entre os dois, e a única coisa que
// atravessa são *estado* e *origem* — nunca um valor de credencial.
//
// Fase 1 (esta): somente leitura. `status` devolve o catálogo de provedores que
// o Pi conhece, incluindo os que não têm credencial, com a origem da
// configuração e sem tocar em segredo. `login` e `logout` respondem
// `not_implemented` de propósito: escrever credencial é a fase 3, e falhar
// fechado é melhor do que oferecer um caminho pela metade.
//
// Uso:
//   node pi-auth-adapter.mjs --status [--package DIR] [--agent-dir DIR]
//   echo '{"id":1,"op":"status"}' | node pi-auth-adapter.mjs
//
// Protocolo (uma linha JSON por mensagem, stdin/stdout):
//   entrada  {"id": <any>, "op": "status"}
//   saída    {"id": <any>, "ok": true,  "data": <snapshot>}
//   saída    {"id": <any>, "ok": false, "error": "<código>", "detail": "<texto>"}
//
// Nunca escreve nada além de JSON em stdout; diagnóstico vai para stderr.

import { existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

const PACKAGE_NAME = "@earendil-works/pi-coding-agent";
const EXIT_OK = 0;
const EXIT_UNSUPPORTED = 3;
const EXIT_NO_RUNTIME = 4;

// Formas de segredo que nunca podem aparecer na saída. Serve de rede de
// segurança: mesmo que alguém acrescente um campo por engano, o valor é
// substituído antes de sair.
const SECRET_PATTERNS = [
  /sk-[A-Za-z0-9_-]{8,}/,
  /eyJ[A-Za-z0-9_-]{10,}/,
  /gh[opusr]_[A-Za-z0-9]{16,}/,
  /Bearer\s+\S{8,}/i,
];

function looksLikeSecret(value) {
  return typeof value === "string" && SECRET_PATTERNS.some((pattern) => pattern.test(value));
}

/** Copia só o que o contrato permite, campo por campo. */
function safeString(value) {
  if (typeof value !== "string" || value.length === 0) return null;
  return looksLikeSecret(value) ? "<redacted>" : value;
}

function safeCount(value) {
  return Number.isInteger(value) && value >= 0 ? value : 0;
}

/// Procura o pacote do Pi nas formas de instalação que o Dish conhece.
///
/// Um caminho explícito (`--package` ou `DISH_PI_PACKAGE`) é autoridade: se ele
/// não contém o pacote, a busca para ali e o adaptador falha, em vez de usar
/// silenciosamente outra instalação que o usuário não pediu. Sem caminho
/// explícito, a ordem é o prefixo privado do Dish, as releases do instalador
/// oficial e, por último, subindo a partir do executável do Pi.
///
/// Devolve `{ dirs, rejected }`.
export function packageCandidates({ explicit, agentDir, piBin, env = process.env, home = homedir() } = {}) {
  const candidates = [];
  const packageDir = (root) => path.join(root, "node_modules", PACKAGE_NAME);
  const hasIndex = (dir) => Boolean(dir) && existsSync(path.join(dir, "dist", "index.js"));

  const stated = [explicit, env.DISH_PI_PACKAGE].filter(Boolean).map((dir) => path.resolve(dir));
  for (const dir of stated) {
    if (!hasIndex(dir)) return { dirs: [], rejected: dir };
    candidates.push(dir);
  }

  const dataHome = env.XDG_DATA_HOME || path.join(home, ".local", "share");
  candidates.push(path.join(dataHome, "dish", "pi", "node_modules", PACKAGE_NAME));

  const installRoot = path.join(agentDir || path.join(home, ".pi", "agent"), "install", "releases");
  if (existsSync(installRoot)) {
    let releases = [];
    try {
      releases = readdirSync(installRoot).sort();
    } catch {
      releases = [];
    }
    for (const release of releases.reverse()) candidates.push(packageDir(path.join(installRoot, release)));
  }

  // Um executável dentro do próprio pacote (npm global, prefixos locais).
  if (piBin) {
    let current = path.dirname(path.resolve(piBin));
    for (let depth = 0; depth < 6; depth += 1) {
      if (path.basename(current) === PACKAGE_NAME) candidates.push(current);
      const parent = path.dirname(current);
      if (parent === current) break;
      current = parent;
    }
  }

  const seen = new Set();
  const dirs = candidates.filter((dir) => {
    if (!dir || seen.has(dir)) return false;
    seen.add(dir);
    return hasIndex(dir);
  });
  return { dirs, rejected: null };
}

/** Catálogo + estado, sem nenhum valor de credencial. */
export async function snapshot({ packageDir, agentDir } = {}) {
  const entry = path.join(packageDir, "dist", "index.js");
  // Caminho absoluto, e não especificador de pacote: o diretório do Pi pode
  // estar em qualquer prefixo, e `import("tests/...")` seria lido como pacote.
  const { ModelRuntime, VERSION } = await import(pathToFileURL(entry).href);
  // `refreshOnCreate` precisa ficar no padrão: com `false`, os acessores
  // síncronos de estado não enxergam credencial já armazenada e o adaptador
  // diria "sem credencial" para um provedor configurado. Medido contra o Pi
  // 1.2.0 — ver docs/provider-auth.md.
  const runtime = await ModelRuntime.create({
    ...(agentDir ? { authPath: path.join(agentDir, "auth.json") } : {}),
    allowModelNetwork: false,
  });

  const providers = runtime.getProviders().map((provider) => {
    const status = (() => {
      try {
        return runtime.getProviderAuthStatus(provider.id);
      } catch {
        return undefined;
      }
    })();
    return {
      id: String(provider.id),
      models: safeCount(runtime.getModels(provider.id).length),
      configured: status?.configured === true,
      source: safeString(status?.source),
      label: safeString(status?.label),
    };
  });

  const credentials = (await runtime.listCredentials()).map((credential) => ({
    providerId: String(credential.providerId),
    type: safeString(credential.type),
  }));

  return {
    pi: { version: safeString(VERSION), package: packageDir },
    providers: providers.sort((a, b) => a.id.localeCompare(b.id)),
    credentials: credentials.sort((a, b) => a.providerId.localeCompare(b.providerId)),
    readOnly: true,
  };
}

function fail(code, detail) {
  return { ok: false, error: code, ...(detail ? { detail: String(detail).slice(0, 200) } : {}) };
}

async function handle(request, options) {
  const op = typeof request?.op === "string" ? request.op : "";
  if (op === "status") {
    const { dirs, rejected } = packageCandidates(options);
    if (rejected) {
      return fail("package_invalid", `${rejected} não contém ${PACKAGE_NAME}`);
    }
    if (dirs.length === 0) {
      return fail("package_not_found", `procurei ${PACKAGE_NAME} em ${options.agentDir ?? "~/.pi/agent"}`);
    }
    try {
      return { ok: true, data: await snapshot({ packageDir: dirs[0], agentDir: options.agentDir }) };
    } catch (error) {
      return fail("probe_failed", error?.message ?? error);
    }
  }
  if (op === "login" || op === "logout") {
    return fail("not_implemented", "escrever credencial é a fase 3; use /login no Pi");
  }
  return fail("unsupported_op", op || "(vazio)");
}

function parseArgs(argv) {
  const options = { agentDir: process.env.PI_CODING_AGENT_DIR, piBin: process.env.DISH_PI_BIN };
  let status = false;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--status") status = true;
    else if (arg === "--package") options.explicit = argv[++index];
    else if (arg === "--agent-dir") options.agentDir = argv[++index];
    else if (arg === "--pi-bin") options.piBin = argv[++index];
    else if (arg === "--help" || arg === "-h") {
      process.stdout.write("uso: pi-auth-adapter.mjs --status [--package DIR] [--agent-dir DIR] [--pi-bin CAMINHO]\n");
      process.exit(EXIT_OK);
    }
  }
  return { status, options };
}

const { status: oneShot, options } = parseArgs(process.argv.slice(2));

if (oneShot) {
  const result = await handle({ op: "status" }, options);
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  process.exit(result.ok ? EXIT_OK : EXIT_NO_RUNTIME);
} else {
  const lines = createInterface({ input: process.stdin });
  for await (const line of lines) {
    if (line.trim() === "") continue;
    let request;
    try {
      request = JSON.parse(line);
    } catch {
      process.stdout.write(`${JSON.stringify({ id: null, ...fail("bad_json") })}\n`);
      continue;
    }
    const result = await handle(request, options);
    process.stdout.write(`${JSON.stringify({ id: request?.id ?? null, ...result })}\n`);
  }
  process.exit(EXIT_OK);
}
