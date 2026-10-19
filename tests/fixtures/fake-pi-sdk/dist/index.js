// Fixture do SDK do Pi para testar o adaptador sem Pi, sem rede e sem credencial.
//
// Cada objeto aqui esconde um segredo em um campo que o adaptador não pode
// copiar (`apiKey`, `oauth.token`, `secret`, `env`). Um teste que encontre
// qualquer um desses valores na saída do adaptador falha: é o contrato de que
// nada além de estado e origem atravessa a fronteira.
//
// Variáveis, todas opcionais:
//   FAKE_PI_EMPTY=1      todos os provedores começam sem credencial
//   FAKE_PI_LOGIN_FAIL=1 o login falha com a chave recusada na mensagem
//
// O armazenamento é um arquivo no `authPath` recebido (o mesmo desenho do Pi:
// `login` grava, `logout` apaga, e um processo novo relê o que ficou). Sem
// `authPath`, fica em memória. Nunca toca no `auth.json` de ninguém.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const SECRET = "sk-ant-FIXTURE-SECRET-DO-NOT-LEAK";
const EMPTY = process.env.FAKE_PI_EMPTY === "1";
const LOGIN_FAILS = process.env.FAKE_PI_LOGIN_FAIL === "1";

function delayed(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export class ModelRuntime {
  static async create(options = {}) {
    const runtime = new ModelRuntime(options);
    runtime.authPath = typeof options.authPath === "string" ? options.authPath : null;
    let loaded = false;
    if (runtime.authPath && existsSync(runtime.authPath)) {
      try {
        runtime.stored = new Map(Object.entries(JSON.parse(readFileSync(runtime.authPath, "utf8"))));
        loaded = true;
      } catch {
        runtime.stored = new Map();
      }
    }
    if (!loaded && !EMPTY) {
      runtime.stored.set("openai", { type: "oauth", refresh: "r", access: SECRET, expires: 0 });
    }
    return runtime;
  }

  constructor() {
    this.stored = new Map();
    this.authPath = null;
  }

  persist() {
    if (!this.authPath) return;
    // O diretório do agente pode ainda não existir num teste; o Pi real o cria.
    mkdirSync(dirname(this.authPath), { recursive: true });
    writeFileSync(this.authPath, JSON.stringify(Object.fromEntries(this.stored)));
  }

  getProviders() {
    return [
      {
        id: "anthropic",
        name: "Anthropic",
        apiKey: SECRET,
        auth: {
          apiKey: {
            name: "Anthropic API key",
            login: async (interaction) => {
              const key = await interaction.prompt({
                type: "secret",
                message: "Cole a chave de API da Anthropic",
                placeholder: "sk-ant-…",
              });
              if (LOGIN_FAILS) throw new Error(`chave recusada pelo provedor: ${key}`);
              return { type: "api_key", key };
            },
          },
        },
      },
      {
        id: "openai",
        name: "OpenAI",
        oauth: { token: "eyJhbGciOiFIXTURE" },
        auth: {
          apiKey: {
            name: "OpenAI API key",
            login: async (interaction) => ({
              type: "api_key",
              key: await interaction.prompt({ type: "secret", message: "Cole a chave da OpenAI" }),
            }),
          },
          oauth: {
            name: "OpenAI (ChatGPT Plus/Pro)",
            loginLabel: "Entrar com a conta ChatGPT",
            isSubscription: true,
            login: async (interaction) => {
              interaction.notify({
                type: "info",
                message: "O navegador vai abrir numa página do provedor.",
                links: [{ url: "https://example.invalid/help", label: "Ajuda" }],
              });
              interaction.notify({ type: "auth_url", url: "https://example.invalid/authorize", instructions: "Autorize e cole o código" });
              interaction.notify({
                type: "device_code",
                userCode: "ABCD-1234",
                verificationUri: "https://example.invalid/device",
                intervalSeconds: 5,
                expiresInSeconds: 900,
              });
              interaction.notify({ type: "progress", message: "Aguardando a autorização…" });
              await delayed(10);
              const code = await interaction.prompt({ type: "manual_code", message: "Cole o código de autorização" });
              return { type: "oauth", refresh: "refresh-token", access: "access-token", expires: 0, code };
            },
          },
        },
      },
      {
        id: "select-provider",
        name: "Provedor com escolha",
        auth: {
          apiKey: {
            name: "Chave com região",
            login: async (interaction) => {
              const region = await interaction.prompt({
                type: "select",
                message: "Escolha a região",
                options: [
                  { id: "br", label: "Brasil", description: "southamerica-east1" },
                  { id: "us", label: "Estados Unidos", description: "us-east-1" },
                ],
              });
              const key = await interaction.prompt({ type: "text", message: "Cole a chave", placeholder: "chave" });
              return { type: "api_key", key, env: { REGION: region } };
            },
          },
        },
      },
      { id: "groq", name: "Groq" },
    ];
  }

  getModels(providerId) {
    return providerId === "anthropic" ? [{}, {}, {}] : providerId === "openai" ? [{}] : [];
  }

  getProviderAuthStatus(providerId) {
    if (this.stored.has(providerId)) {
      return { configured: true, source: "stored", label: "OpenAI (assinatura)" };
    }
    if (providerId === "groq") {
      return { configured: false, source: undefined, label: undefined };
    }
    return { configured: false };
  }

  hasConfiguredAuth(providerId) {
    return this.stored.has(providerId);
  }

  isUsingOAuth(providerId) {
    return this.stored.get(providerId)?.type === "oauth";
  }

  async listCredentials() {
    return [...this.stored.entries()].map(([providerId, credential]) => ({
      providerId,
      type: credential.type,
      secret: SECRET,
    }));
  }

  async checkAuth(providerId) {
    if (this.stored.has(providerId)) return { source: "stored", type: this.stored.get(providerId).type };
    throw new Error(`no credential for ${providerId}: ${SECRET}`);
  }

  async getAuth() {
    // O adaptador nunca deve chamar isto; se chamar, o valor não pode sair.
    return { auth: { type: "oauth", access: SECRET }, source: "stored" };
  }

  async login(providerId, type, interaction) {
    const provider = this.getProviders().find((entry) => entry.id === providerId);
    const method = type === "oauth" ? provider?.auth?.oauth : provider?.auth?.apiKey;
    if (!method?.login) throw new Error(`${providerId} does not support ${type} login`);
    const credential = await method.login(interaction);
    if (interaction.signal?.aborted) throw Object.assign(new Error("aborted"), { name: "AbortError" });
    this.stored.set(providerId, credential);
    this.persist();
    return credential;
  }

  async logout(providerId) {
    this.stored.delete(providerId);
    this.persist();
  }
}
