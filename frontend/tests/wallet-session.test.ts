import { describe, it, expect, beforeEach, vi } from "vitest";
import WalletSessionService from "../src/services/wallet-session-service";
import {
  ProviderNotFoundError,
  RejectedSignatureError,
  StaleSessionError,
} from "../src/types/wallet-session";

class MockAdapter {
  available = true;
  address = "GTESTADDRESS123";
  provider = { id: "mock", name: "MockWallet" };
  willRejectSign = false;

  isAvailable() {
    return this.available;
  }

  async connect() {
    return {
      address: this.address,
      provider: this.provider,
      network: "TESTNET",
    };
  }

  async disconnect() {
    return;
  }

  async signMessage(_m: string) {
    if (this.willRejectSign) throw new Error("User rejected");
    return "signed";
  }
}

beforeEach(() => {
  localStorage.clear();
});

describe("WalletSessionService", () => {
  it("connects successfully", async () => {
    const svc = new WalletSessionService({ supportedNetworks: ["TESTNET"] });
    const adapter = new MockAdapter();
    svc.setProviderAdapter(adapter as any);

    const meta = await svc.connect({ network: "TESTNET" });
    expect(meta.address).toBe(adapter.address);
    expect(svc.getState()).toBe("CONNECTED");
    // persisted
    const raw = JSON.parse(
      localStorage.getItem("stc_wallet_session_v1") as string,
    );
    expect(raw.meta.address).toBe(adapter.address);
  });

  it("throws ProviderNotFoundError when adapter missing", async () => {
    const svc = new WalletSessionService();
    await expect(svc.connect()).rejects.toBeInstanceOf(ProviderNotFoundError);
  });

  it("reconnects using stored session and verifies signature", async () => {
    const svc = new WalletSessionService({ supportedNetworks: ["TESTNET"] });
    const adapter = new MockAdapter();
    svc.setProviderAdapter(adapter as any);

    const meta = await svc.connect({ network: "TESTNET" });
    // create a new service instance to simulate app reload
    const svc2 = new WalletSessionService({ supportedNetworks: ["TESTNET"] });
    svc2.setProviderAdapter(adapter as any);
    const meta2 = await svc2.reconnect();
    expect(meta2.address).toBe(meta.address);
  });

  it("reconnect fails when sign rejected", async () => {
    const svc = new WalletSessionService({ supportedNetworks: ["TESTNET"] });
    const adapter = new MockAdapter();
    svc.setProviderAdapter(adapter as any);

    await svc.connect({ network: "TESTNET" });

    // adapter that rejects signatures
    const adapter2 = new MockAdapter();
    adapter2.willRejectSign = true;
    const svc2 = new WalletSessionService({ supportedNetworks: ["TESTNET"] });
    svc2.setProviderAdapter(adapter2 as any);

    await expect(svc2.reconnect()).rejects.toBeInstanceOf(
      RejectedSignatureError,
    );
  });

  it("stale session is removed", async () => {
    const svc = new WalletSessionService({
      supportedNetworks: ["TESTNET"],
      sessionExpiryMs: 1,
    });
    const adapter = new MockAdapter();
    svc.setProviderAdapter(adapter as any);
    await svc.connect({ network: "TESTNET" });
    // wait for expiry
    await new Promise((r) => setTimeout(r, 5));
    const svc2 = new WalletSessionService({
      supportedNetworks: ["TESTNET"],
      sessionExpiryMs: 1,
    });
    await expect(svc2.reconnect()).rejects.toBeInstanceOf(StaleSessionError);
  });
});
