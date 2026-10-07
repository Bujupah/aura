import { useEffect, useState, type FormEvent } from "react";
import type { ShellBridge } from "../shell/bridge";
import type { Provider, TokenSource, TokenStatus } from "../shell/types";

const PROVIDERS: readonly { id: Provider; name: string; use: string }[] = [
  { id: "openai", name: "OpenAI token", use: "Listening, notes, diagrams and images" },
  { id: "gemini", name: "Gemini token", use: "Translation only" },
];

const SOURCE_TEXT: Record<TokenSource, string> = {
  keychain: "Saved in this Mac's Keychain",
  environment: "Using the one from the environment",
  none: "Not set",
};

/**
 * Lets the seller replace the provider tokens. Entry is one-way: a token is
 * sent to the core to be stored and is never read back, so this window can
 * only say whether one is present.
 */
export function TokensWindow({ bridge }: { bridge: ShellBridge }) {
  const [status, setStatus] = useState<TokenStatus | null>(null);
  useEffect(() => {
    bridge.getTokenStatus().then(setStatus, (error: unknown) => console.error("tokens_status failed", error));
  }, [bridge]);

  return (
    <main className="tokens">
      <p>
        Tokens are stored in your Keychain and used from the next time you start listening. A token
        saved here replaces any other.
      </p>
      {PROVIDERS.map((provider) => (
        <TokenRow
          key={provider.id}
          bridge={bridge}
          provider={provider.id}
          name={provider.name}
          use={provider.use}
          source={status?.[provider.id] ?? null}
          onStatus={setStatus}
        />
      ))}
    </main>
  );
}

function TokenRow({
  bridge,
  provider,
  name,
  use,
  source,
  onStatus,
}: {
  bridge: ShellBridge;
  provider: Provider;
  name: string;
  use: string;
  source: TokenSource | null;
  onStatus: (status: TokenStatus) => void;
}) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ tone: "ok" | "error"; text: string } | null>(null);
  const inputId = `token-${provider}`;

  async function run(action: () => Promise<TokenStatus>, done: string) {
    setBusy(true);
    try {
      onStatus(await action());
      // The field never keeps a token once it has been handed over.
      setValue("");
      setMessage({ tone: "ok", text: done });
    } catch (error) {
      setMessage({ tone: "error", text: typeof error === "string" ? error : "That didn't work." });
    } finally {
      setBusy(false);
    }
  }

  function save(event: FormEvent) {
    event.preventDefault();
    void run(() => bridge.setToken(provider, value), "Saved.");
  }

  return (
    <form className="token-row" onSubmit={save}>
      <div className="token-head">
        <label htmlFor={inputId}>{name}</label>
        {source && (
          <span className="token-source" data-source={source}>
            {SOURCE_TEXT[source]}
          </span>
        )}
      </div>
      <div className="token-controls">
        <input
          id={inputId}
          type="password"
          value={value}
          onChange={(event) => setValue(event.target.value)}
          placeholder={source === "none" ? "Paste a token" : "Paste a new token to replace it"}
          autoComplete="off"
          spellCheck={false}
          aria-describedby={`${inputId}-help`}
        />
        <button type="submit" className="action" disabled={busy || value.trim() === ""}>
          Save
        </button>
        <button
          type="button"
          className="action"
          disabled={busy || source !== "keychain"}
          title="Remove the token saved here"
          onClick={() => void run(() => bridge.clearToken(provider), "Removed.")}
        >
          Remove
        </button>
      </div>
      <div
        id={`${inputId}-help`}
        className="token-message"
        data-tone={message?.tone ?? "ok"}
        role={message?.tone === "error" ? "alert" : "status"}
      >
        {message?.text ?? use}
      </div>
    </form>
  );
}
