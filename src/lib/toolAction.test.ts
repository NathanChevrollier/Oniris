import { renderHook, act, cleanup } from "@testing-library/react";
import { it, expect, afterEach } from "vitest";
import { friendlyError, useToolAction } from "./toolAction";
import type { ToolOutcome } from "./ipc";

afterEach(cleanup);

const FALLBACK = "Notes indisponibles, réessaie.";
const setup = () => renderHook(() => useToolAction(FALLBACK));
const fail = (error: string): ToolOutcome => ({ Failed: { action_id: 1, error } });

it("Done et NeedsConfirmation réussissent sans erreur", async () => {
  const { result } = setup();
  let ok = false;
  await act(async () => { ok = await result.current.run(Promise.resolve({ Done: { action_id: 1, output: {} } })); });
  expect(ok).toBe(true);
  expect(result.current.error).toBeNull();
  const pending = { action_id: 2, tool: "notes.delete", text: { summary: "", details: "", risk: "Low" as const }, provenance: "User" as const, expires_at_ms: 0 };
  await act(async () => { ok = await result.current.run(Promise.resolve({ NeedsConfirmation: pending })); });
  expect(ok).toBe(true);
  expect(result.current.error).toBeNull();
});

it("Failed : erreur connue traduite, erreur brute remplacée par le message de repli", async () => {
  const { result } = setup();
  let ok = true;
  await act(async () => { ok = await result.current.run(Promise.resolve(fail("arguments invalides : durée introuvable"))); });
  expect(ok).toBe(false);
  expect(result.current.error).toBe("Durée introuvable — essaie « 10 min pâtes ».");
  await act(async () => { await result.current.run(Promise.resolve(fail("échec : database is locked"))); });
  expect(result.current.error).toBe(FALLBACK);
  act(() => result.current.clear());
  expect(result.current.error).toBeNull();
});

it("appel rejeté : message de repli ; pending suit l'appel en cours", async () => {
  const { result } = setup();
  let reject!: (e: unknown) => void;
  let done!: Promise<boolean>;
  act(() => { done = result.current.run(new Promise<ToolOutcome>((_, r) => { reject = r; })); });
  expect(result.current.pending).toBe(true);
  await act(async () => { reject(new Error("ipc")); await done; });
  expect(await done).toBe(false);
  expect(result.current.pending).toBe(false);
  expect(result.current.error).toBe(FALLBACK);
});

it("Rejected : échec silencieux", async () => {
  const { result } = setup();
  let ok = true;
  await act(async () => { ok = await result.current.run(Promise.resolve({ Rejected: { action_id: 1 } })); });
  expect(ok).toBe(false);
  expect(result.current.error).toBeNull();
});

it("friendlyError retire les préfixes du courtier", () => {
  expect(friendlyError("échec : note introuvable", FALLBACK)).toBe("Cette note n'existe plus.");
  expect(friendlyError("arguments invalides : date introuvable", FALLBACK)).toBe("Date introuvable — essaie « demain 14h dentiste ».");
  expect(friendlyError("outil inconnu : x", FALLBACK)).toBe(FALLBACK);
});
