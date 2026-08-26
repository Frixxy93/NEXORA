import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import type { AuthStatus } from "../lib/types";
import { api } from "../lib/api";

// Exposes the signed-in identity + logout to the rest of the app (Settings uses
// it to show "Signed in as …" and Log out).
interface AuthContextValue {
  email: string | null;
  logout: () => Promise<void>;
}
const AuthContext = createContext<AuthContextValue>({
  email: null,
  logout: async () => {},
});
export function useAuth() {
  return useContext(AuthContext);
}

/**
 * Gates the whole app behind Google sign-in. Shows a "Continue with Google"
 * screen until the user authenticates. The session lives in the Rust backend's
 * memory, so every launch starts signed out.
 */
export function AuthGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AuthStatus | null>(null); // null = still loading

  useEffect(() => {
    api.authStatus().then(setStatus).catch(() => setStatus({ authenticated: false, email: null }));
  }, []);

  if (!status) {
    return <Splash />;
  }

  if (!status.authenticated) {
    return <AuthScreen onDone={setStatus} />;
  }

  const logout = async () => {
    const next = await api.authLogout();
    setStatus(next);
  };

  return (
    <AuthContext.Provider value={{ email: status.email, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

function Splash() {
  return (
    <div className="h-full w-full flex items-center justify-center bg-ink-900">
      <div className="text-muted text-sm animate-pulse">Loading NEXORA…</div>
    </div>
  );
}

function AuthScreen({ onDone }: { onDone: (s: AuthStatus) => void }) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const signIn = async () => {
    setError(null);
    setBusy(true);
    try {
      const status = await api.authLoginGoogle();
      onDone(status);
    } catch (err) {
      setError(String(err instanceof Error ? err.message : err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="h-full w-full flex items-center justify-center bg-ink-900 px-4">
      <div className="w-full max-w-sm">
        <div className="text-center mb-6">
          <div className="text-2xl font-bold tracking-tight text-white">NEXORA</div>
          <div className="text-[11px] uppercase tracking-[0.2em] text-accent mt-1">
            Material Library
          </div>
        </div>

        <div className="panel p-6 shadow-2xl text-center">
          <h1 className="text-lg font-semibold text-white">Sign in</h1>
          <p className="text-xs text-muted mt-1 mb-5">
            Sign in with your Google account to unlock your library.
          </p>

          <button
            onClick={signIn}
            disabled={busy}
            className="w-full flex items-center justify-center gap-3 bg-white text-[#3c4043] font-medium text-sm rounded-lg px-4 py-2.5 hover:bg-slate-100 disabled:opacity-60 transition-colors"
          >
            <GoogleG />
            {busy ? "Waiting for Google…" : "Continue with Google"}
          </button>

          {busy && (
            <p className="text-[11px] text-muted mt-3">
              A browser window opened — pick your Google account there, then come back.
            </p>
          )}
          {error && <div className="text-xs text-bad mt-3">{error}</div>}
        </div>

        <p className="text-[10px] text-ink-600 text-center mt-4 leading-relaxed">
          NEXORA never sees your Google password. Signing in needs an internet connection.
        </p>
      </div>
    </div>
  );
}

// Google's multi-color "G" mark.
function GoogleG() {
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
      <path
        fill="#4285F4"
        d="M17.64 9.2c0-.64-.06-1.25-.16-1.84H9v3.48h4.84a4.14 4.14 0 0 1-1.8 2.72v2.26h2.92c1.7-1.57 2.68-3.88 2.68-6.62z"
      />
      <path
        fill="#34A853"
        d="M9 18c2.43 0 4.47-.8 5.96-2.18l-2.92-2.26c-.8.54-1.83.86-3.04.86-2.34 0-4.32-1.58-5.03-3.7H.96v2.33A9 9 0 0 0 9 18z"
      />
      <path
        fill="#FBBC05"
        d="M3.97 10.72a5.4 5.4 0 0 1 0-3.44V4.95H.96a9 9 0 0 0 0 8.1l3.01-2.33z"
      />
      <path
        fill="#EA4335"
        d="M9 3.58c1.32 0 2.5.45 3.44 1.35l2.58-2.58C13.47.9 11.43 0 9 0A9 9 0 0 0 .96 4.95l3.01 2.33C4.68 5.16 6.66 3.58 9 3.58z"
      />
    </svg>
  );
}
