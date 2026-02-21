import { useEffect, useState, useRef } from "react";
import WalletSessionService, {
  WalletProviderAdapter,
} from "../services/wallet-session-service";
import type {
  WalletSessionMeta,
  WalletSessionState,
} from "../types/wallet-session";

// Simple hook to expose the session service state. UI-agnostic.
export function useWalletSession(service?: WalletSessionService) {
  const svcRef = useRef<WalletSessionService | null>(service ?? null);
  if (!svcRef.current) {
    svcRef.current = new WalletSessionService();
  }

  const [state, setState] = useState<WalletSessionState>(
    svcRef.current.getState(),
  );
  const [meta, setMeta] = useState<WalletSessionMeta | null>(
    svcRef.current.getMeta(),
  );
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    const unsubscribe = svcRef.current!.subscribe(
      (
        s: WalletSessionState,
        m: WalletSessionMeta | null | undefined,
        e: Error | null | undefined,
      ) => {
        setState(s);
        setMeta(m ?? null);
        setError(e ?? null);
      },
    );
    return () => unsubscribe();
  }, []);

  const connect = (
    providerAdapter?: WalletProviderAdapter,
    opts?: { network?: string },
  ) => {
    if (providerAdapter) svcRef.current!.setProviderAdapter(providerAdapter);
    return svcRef.current!.connect(opts);
  };

  const disconnect = () => svcRef.current!.disconnect();
  const reconnect = () => svcRef.current!.reconnect();

  return {
    state,
    meta,
    error,
    connect,
    disconnect,
    reconnect,
    service: svcRef.current,
  };
}

export default useWalletSession;
