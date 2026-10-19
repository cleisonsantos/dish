// Fixture do SDK do Pi para testar o adaptador sem Pi, sem rede e sem credencial.
//
// Cada objeto aqui esconde um segredo em um campo que o adaptador não pode
// copiar (`apiKey`, `oauth.token`, `secret`, `env`). Um teste que encontre
// qualquer um desses valores na saída do adaptador falha: é o contrato de que
// nada além de estado e origem atravessa a fronteira.
//
// `FAKE_PI_EMPTY=1` deixa todos os provedores sem credencial, para o teste de
// interface cobrir o primeiro uso em que nada está conectado.
const SECRET = "sk-ant-FIXTURE-SECRET-DO-NOT-LEAK";
const EMPTY = process.env.FAKE_PI_EMPTY === "1";

export class ModelRuntime {
  static async create(options = {}) {
    return new ModelRuntime(options);
  }

  getProviders() {
    return [
      { id: "anthropic", name: "Anthropic", apiKey: SECRET, env: { ANTHROPIC_API_KEY: SECRET } },
      { id: "openai", name: "OpenAI", oauth: { token: "eyJhbGciOiFIXTURE", refresh: SECRET } },
      { id: "groq", name: "Groq" },
    ];
  }

  getModels(providerId) {
    return providerId === "anthropic" ? [{}, {}, {}] : providerId === "openai" ? [{}] : [];
  }

  getProviderAuthStatus(providerId) {
    if (EMPTY) return { configured: false };
    if (providerId === "openai") {
      return { configured: true, source: "stored", label: "OpenAI (assinatura)" };
    }
    if (providerId === "groq") {
      return { configured: false, source: undefined, label: undefined };
    }
    return { configured: false };
  }

  hasConfiguredAuth(providerId) {
    return !EMPTY && providerId === "openai";
  }

  isUsingOAuth(providerId) {
    return !EMPTY && providerId === "openai";
  }

  async listCredentials() {
    if (EMPTY) return [];
    return [
      { providerId: "openai", type: "oauth", secret: SECRET },
      { providerId: "groq", type: "api_key", key: SECRET },
    ];
  }

  async checkAuth(providerId) {
    if (!EMPTY && providerId === "openai") return { source: "stored", type: "oauth" };
    throw new Error(`no credential for ${providerId}: ${SECRET}`);
  }

  async getAuth() {
    // O adaptador nunca deve chamar isto; se chamar, o valor não pode sair.
    return { auth: { type: "oauth", access: SECRET }, source: "stored" };
  }

  async login() {
    return { type: "oauth", access: SECRET };
  }

  async logout() {}
}
