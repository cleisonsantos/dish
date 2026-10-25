#!/usr/bin/env node
// Adaptador de autenticação do Dish para o Pi.
//
// O Dish não reimplementa autenticação: o Pi é dono do OAuth, do refresh e do
// armazenamento. Este processo é a fronteira entre os dois, e o que atravessa é
// *estado*, *origem* e as *perguntas* de um login — nunca um valor de
// credencial de volta.
//
// Uso:
//   node pi-auth-adapter.mjs --status [--package DIR] [--agent-dir DIR] [--pi-bin CAMINHO]
//   echo '{"id":1,"op":"status"}' | node pi-auth-adapter.mjs
//
// Protocolo (uma linha JSON por mensagem, stdin/stdout). O processo é de longa
// duração: um login faz perguntas e espera respostas, então a leitura continua
// enquanto ele acontece.
//
//   entrada  {"id": N, "op": "status"}
//   entrada  {"id": N, "op": "login", "provider": "anthropic", "type": "api_key"|"oauth"}
//   entrada  {"id": N, "op": "logout", "provider": "anthropic"}
//   entrada  {"id": N, "op": "reply", "prompt": P, "value": "…"}
//   entrada  {"id": N, "op": "cancel", "target": N}
//
//   saída    {"id": N, "ok": true,  "data": {…}}
//   saída    {"id": N, "ok": false, "error": "<código>", "detail": "<texto>"}
//   saída    {"id": N, "prompt": P, "ask": "text|secret|select|manual_code", …}
//   saída    {"id": N, "notify": "info|auth_url|device_code|progress", …}
//
// Regras do contrato: a resposta de um prompt `secret` viaja só do Dish para o
// Pi; nenhum valor volta. O desfecho de um login informa provedor e tipo, e
// nada mais. Nunca escreve nada além de JSON em stdout; diagnóstico vai para
// stderr.

import { existsSync, readdirSync, realpathSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

const PACKAGE_NAME = "@earendil-works/pi-coding-agent";
const EXIT_OK = 0;
const EXIT_NO_RUNTIME = 4;

// Formas de segredo que nunca podem aparecer na saída. Serve de rede de
// segurança: mesmo que alguém acrescente um campo por engano, o valor é
// substituído antes de sair. Também cobre mensagens de erro do provedor, que
// costumam repetir a chave recusada.
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

/** Texto de erro que pode ser mostrado: sem segredo, sem caminho, curto. */
function safeDetail(value) {
  const text = typeof value === "string" ? value : String(value ?? "");
  if (looksLikeSecret(text)) return "<redacted>";
  return text.slice(0, 200);
}

function fail(code, detail) {
  return { ok: false, error: code, ...(detail ? { detail: safeDetail(detail) } : {}) };
}

function write(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

/// Procura o pacote do Pi nas formas de instalação que o Dish conhece.
///
/// Um caminho explícito (`--package` ou `DISH_PI_PACKAGE`) é autoridade: se ele
/// não contém o pacote, a busca para ali e o adaptador falha, em vez de usar
/// silenciosamente outra instalação que o usuário não pediu. Sem caminho
/// explícito, a ordem é o prefixo privado do Dish, as releases do instalador
/// oficial, o npm global e, por último, subindo a partir do executável do Pi.
///
/// Devolve `{ dirs, rejected }`.
function hasPackageIndex(dir) {
  return Boolean(dir) && existsSync(path.join(dir, "dist", "index.js"));
}

/** O pacote dentro de uma raiz npm, nas duas formas de prefixo. */
function packageDirs(root) {
  return [
    path.join(root, "node_modules", PACKAGE_NAME),
    path.join(root, "lib", "node_modules", PACKAGE_NAME),
  ];
}

/** O nome com escopo não cabe em `basename`, que devolveria `pi-coding-agent`. */
function isPackageDir(dir) {
  const parts = path.resolve(dir).split(path.sep);
  return parts.length >= 2 && parts.at(-2) === "@earendil-works" && parts.at(-1) === "pi-coding-agent";
}

function realPath(file) {
  try {
    return realpathSync(file);
  } catch {
    return path.resolve(file);
  }
}

/** `npm root -g`, quando o npm está disponível; sem ele, a busca continua. */
function npmGlobalRoot(env) {
  try {
    const result = spawnSync("npm", ["root", "-g"], {
      encoding: "utf8",
      env,
      timeout: 3000,
      windowsHide: true,
      shell: process.platform === "win32",
    });
    if (result.status !== 0 || typeof result.stdout !== "string") return null;
    return result.stdout.trim().split(/\r?\n/).filter(Boolean).pop() ?? null;
  } catch {
    return null;
  }
}

export function packageCandidates({ explicit, agentDir, piBin, env = process.env, home = homedir() } = {}) {
  const candidates = [];

  const stated = [explicit, env.DISH_PI_PACKAGE].filter(Boolean).map((dir) => path.resolve(dir));
  for (const dir of stated) {
    if (!hasPackageIndex(dir)) return { dirs: [], rejected: dir };
    candidates.push(dir);
  }

  // Prefixo privado do Dish: XDG no Unix, LOCALAPPDATA no Windows.
  const dataRoots = [];
  if (env.XDG_DATA_HOME) dataRoots.push(env.XDG_DATA_HOME);
  if (env.LOCALAPPDATA) dataRoots.push(env.LOCALAPPDATA);
  if (dataRoots.length === 0) dataRoots.push(path.join(home, ".local", "share"));
  for (const root of dataRoots) candidates.push(...packageDirs(path.join(root, "dish", "pi")));

  const installRoot = path.join(agentDir || path.join(home, ".pi", "agent"), "install", "releases");
  if (existsSync(installRoot)) {
    let releases = [];
    try {
      releases = readdirSync(installRoot).sort();
    } catch {
      releases = [];
    }
    for (const release of releases.reverse()) candidates.push(...packageDirs(path.join(installRoot, release)));
  }

  // npm global no Windows e outros prefixos npm conhecidos pelo ambiente.
  if (env.APPDATA) candidates.push(...packageDirs(path.join(env.APPDATA, "npm")));
  if (env.npm_config_prefix) candidates.push(...packageDirs(env.npm_config_prefix));

  // Um executável dentro do próprio pacote (npm global, prefixos locais).
  // O link `bin/pi` é resolvido antes: o alvo é que indica o pacote.
  if (piBin) {
    let current = path.dirname(realPath(piBin));
    const roots = [];
    for (let depth = 0; depth < 6; depth += 1) {
      if (isPackageDir(current)) candidates.push(current);
      roots.push(current);
      const parent = path.dirname(current);
      if (parent === current) break;
      current = parent;
    }
    for (const root of roots) candidates.push(...packageDirs(root));
  }

  const seen = new Set();
  const dirs = candidates.filter((dir) => {
    if (!dir || seen.has(dir)) return false;
    seen.add(dir);
    return hasPackageIndex(dir);
  });

  // Último recurso: perguntar ao npm onde ficam os pacotes globais. Só roda
  // quando as formas conhecidas falharam, para não atrasar o caminho comum.
  if (dirs.length === 0 && !env.DISH_PI_PACKAGE) {
    const npmRoot = npmGlobalRoot(env);
    if (npmRoot) {
      const packagePath = path.join(npmRoot, PACKAGE_NAME);
      if (hasPackageIndex(packagePath)) dirs.push(packagePath);
    }
  }

  return { dirs, rejected: null };
}

/// Um provedor só entra na lista se houver um login interativo para oferecer.
function authMethods(provider) {
  const apiKey = provider.auth?.apiKey;
  const oauth = provider.auth?.oauth;
  const interactive = (method) => Boolean(method) && typeof method.login === "function";
  const apiKeyEntry = interactive(apiKey)
    ? { name: safeString(apiKey.name) ?? "Chave de API" }
    : null;
  const oauthEntry = interactive(oauth)
    ? {
        name: safeString(oauth.name) ?? "Conta do provedor",
        label: safeString(oauth.loginLabel),
        subscription: oauth.isSubscription === true,
      }
    : null;
  return { api_key: apiKeyEntry, oauth: oauthEntry };
}

/** Catálogo + estado, sem nenhum valor de credencial. */
export async function snapshot(runtime, { packageDir, version } = {}) {
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
      auth: authMethods(provider),
    };
  });

  const credentials = (await runtime.listCredentials()).map((credential) => ({
    providerId: String(credential.providerId),
    type: safeString(credential.type),
  }));

  return {
    pi: { version: safeString(version), package: packageDir ?? null },
    providers: providers.sort((a, b) => a.id.localeCompare(b.id)),
    credentials: credentials.sort((a, b) => a.providerId.localeCompare(b.providerId)),
    readOnly: false,
  };
}

export class Adapter {
  constructor(options = {}) {
    this.options = options;
    this.runtimePromise = null;
    this.packageDir = null;
    this.active = null;
    this.nextPromptId = 1;
  }

  async runtime() {
    if (!this.runtimePromise) {
      const { dirs, rejected } = packageCandidates(this.options);
      if (rejected) throw Object.assign(new Error("package_invalid"), { code: "package_invalid" });
      if (dirs.length === 0) throw Object.assign(new Error("package_not_found"), { code: "package_not_found" });
      this.packageDir = dirs[0];
      this.runtimePromise = import(pathToFileURL(path.join(dirs[0], "dist", "index.js")).href).then(
        async (module) => {
          // `refreshOnCreate` fica no padrão: com `false`, os acessores
          // síncronos de estado não enxergam credencial já armazenada.
          const runtime = await module.ModelRuntime.create({
            ...(this.options.agentDir ? { authPath: path.join(this.options.agentDir, "auth.json") } : {}),
            allowModelNetwork: false,
          });
          this.version = module.VERSION;
          return runtime;
        },
      );
    }
    return this.runtimePromise;
  }

  async handle(request) {
    const op = typeof request?.op === "string" ? request.op : "";
    const id = request?.id ?? null;
    if (op === "status") {
      try {
        const runtime = await this.runtime();
        return { id, ok: true, data: await snapshot(runtime, { packageDir: this.packageDir, version: this.version }) };
      } catch (error) {
        return { id, ...fail(error?.code ?? "probe_failed", error?.message ?? error) };
      }
    }
    if (op === "logout") {
      const provider = typeof request.provider === "string" ? request.provider : "";
      if (!provider) return { id, ...fail("bad_request", "provider ausente") };
      try {
        const runtime = await this.runtime();
        await runtime.logout(provider);
        return { id, ok: true, data: { provider } };
      } catch (error) {
        return { id, ...fail("logout_failed", error?.message ?? error) };
      }
    }
    if (op === "reply") {
      const pending = this.active?.prompts.get(Number(request.prompt));
      if (!pending) return { id, ...fail("unknown_prompt") };
      this.active.prompts.delete(Number(request.prompt));
      pending.resolve(typeof request.value === "string" ? request.value : "");
      return null; // a resposta em si só sai no fim da operação
    }
    if (op === "cancel") {
      if (!this.active || this.active.id !== request.target) return { id, ...fail("nothing_to_cancel") };
      this.active.controller.abort();
      return null;
    }
    if (op === "login") {
      return { id, ...fail("unsupported_op", "login é despachado em paralelo") };
    }
    return { id, ...fail("unsupported_op", op || "(vazio)") };
  }

  /** Um login por vez: duas perguntas simultâneas não teriam para onde ir. */
  startLogin(request, onMessage) {
    const id = request?.id ?? null;
    if (this.active) {
      write({ id, ...fail("busy", "já existe um login em andamento") });
      return;
    }
    const provider = typeof request.provider === "string" ? request.provider : "";
    const type = request.type === "oauth" ? "oauth" : "api_key";
    if (!provider) {
      write({ id, ...fail("bad_request", "provider ausente") });
      return;
    }
    const controller = new AbortController();
    const active = { id, controller, prompts: new Map(), provider, type };
    this.active = active;

    const prompt = (request_) =>
      new Promise((resolve, reject) => {
        const promptId = this.nextPromptId++;
        const entry = { resolve, reject };
        active.prompts.set(promptId, entry);
        const abort = () => {
          active.prompts.delete(promptId);
          reject(withCode(new Error("cancelled"), "cancelled"));
        };
        request_.signal?.addEventListener("abort", abort, { once: true });
        controller.signal.addEventListener("abort", abort, { once: true });
        onMessage({
          id,
          prompt: promptId,
          ask: request_.type,
          message: safeString(request_.message),
          placeholder: safeString(request_.placeholder),
          options: (request_.options ?? []).map((option) => ({
            id: String(option.id),
            label: safeString(option.label) ?? String(option.id),
            description: safeString(option.description),
          })),
        });
      });

    const notify = (event) => {
      onMessage({
        id,
        notify: event.type,
        message: safeString(event.message),
        url: safeString(event.url),
        instructions: safeString(event.instructions),
        userCode: safeString(event.userCode),
        verificationUri: safeString(event.verificationUri),
        intervalSeconds: safeCount(event.intervalSeconds),
        expiresInSeconds: safeCount(event.expiresInSeconds),
        links: (event.links ?? []).map((link) => ({ url: safeString(link.url), label: safeString(link.label) })),
      });
    };

    this.runtime()
      .then((runtime) =>
        runtime.login(provider, type, { prompt, notify, signal: controller.signal }, { agentName: "Dish" }),
      )
      .then((credential) => {
        // Só o desfecho: nada do que foi digitado volta.
        write({ id, ok: true, data: { provider, type: safeString(credential?.type) ?? type } });
      })
      .catch((error) => {
        const aborted = controller.signal.aborted || error?.name === "AbortError" || error?.code === "cancelled";
        write({ id, ...fail(aborted ? "cancelled" : classifyLoginError(error), error?.message ?? error) });
      })
      .finally(() => {
        for (const entry of active.prompts.values()) entry.reject(withCode(new Error("cancelled"), "cancelled"));
        active.prompts.clear();
        if (this.active === active) this.active = null;
      });
  }
}

function withCode(error, code) {
  error.code = code;
  return error;
}

function classifyLoginError(error) {
  const message = String(error?.message ?? "");
  if (error?.code === "package_not_found" || error?.code === "package_invalid") return error.code;
  if (/does not support|unsupported/i.test(message)) return "unsupported_type";
  if (/unknown provider/i.test(message)) return "unknown_provider";
  return "login_failed";
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
      process.stdout.write(
        "uso: pi-auth-adapter.mjs --status [--package DIR] [--agent-dir DIR] [--pi-bin CAMINHO]\n" +
          "     echo '{\"id\":1,\"op\":\"status\"}' | pi-auth-adapter.mjs\n",
      );
      process.exit(EXIT_OK);
    }
  }
  return { status, options };
}

const invokedDirectly = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;

if (invokedDirectly) {
  const { status: oneShot, options } = parseArgs(process.argv.slice(2));
  const adapter = new Adapter(options);
  if (oneShot) {
    const result = await adapter.handle({ op: "status" });
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
        write({ id: null, ...fail("bad_json") });
        continue;
      }
      // Um login não bloqueia a leitura: as respostas aos prompts chegam nas
      // linhas seguintes.
      if (request?.op === "login") {
        adapter.startLogin(request, write);
        continue;
      }
      const answer = await adapter.handle(request);
      if (answer) write(answer);
    }
    process.exit(EXIT_OK);
  }
}
