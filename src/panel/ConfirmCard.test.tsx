import { useLayoutEffect } from "react";
import { render, screen, fireEvent, cleanup, act } from "@testing-library/react";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { ConfirmCard } from "./ConfirmCard";
import type { PendingAction } from "../lib/ipc";
import { usePanelStore } from "./store";

beforeEach(() => { vi.useFakeTimers(); usePanelStore.setState({ open: true }); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

const base: PendingAction = {
  action_id: 1, tool: "shell",
  text: { summary: "Lancer une commande", details: "Remove-Item x", risk: "High" },
  provenance: { Untrusted: { source: "mail de inconnu@x.fr" } }, expires_at_ms: 0,
};

it("ConfirmCard signale la provenance non fiable", () => {
  render(<ConfirmCard pending={base} onResolve={() => {}} />);
  expect(screen.getByText(/Demandé à partir de : mail de inconnu@x.fr/)).toBeInTheDocument();
  expect(screen.getByText("Remove-Item x")).toBeInTheDocument();
});

it("pas de mention de provenance pour l'utilisateur, focus sur Refuser", () => {
  render(<ConfirmCard pending={{ ...base, provenance: "User" }} onResolve={() => {}} />);
  expect(screen.queryByText(/Demandé à partir de/)).toBeNull();
  expect(screen.getByRole("button", { name: "Refuser" })).toHaveFocus();
});

it("Autoriser appelle onResolve(true), Refuser onResolve(false)", () => {
  const onResolve = vi.fn();
  render(<ConfirmCard pending={base} onResolve={onResolve} />);
  act(() => { vi.advanceTimersByTime(500); });
  fireEvent.click(screen.getByRole("button", { name: "Autoriser" }));
  expect(onResolve).toHaveBeenLastCalledWith(true);
  fireEvent.click(screen.getByRole("button", { name: "Refuser" }));
  expect(onResolve).toHaveBeenLastCalledWith(false);
});

it("Autoriser est inactif 500 ms après l'apparition d'une nouvelle action", () => {
  const { rerender } = render(<ConfirmCard pending={base} onResolve={() => {}} />);
  const btn = () => screen.getByRole("button", { name: "Autoriser" });
  expect(btn()).toBeDisabled();
  act(() => { vi.advanceTimersByTime(500); });
  expect(btn()).toBeEnabled();
  rerender(<ConfirmCard pending={{ ...base, action_id: 2 }} onResolve={() => {}} />);
  expect(btn()).toBeDisabled();
  act(() => { vi.advanceTimersByTime(500); });
  expect(btn()).toBeEnabled();
});

it("Autoriser est inactif dès le premier rendu après un changement d'action", () => {
  const seen: boolean[] = [];
  // Lit le DOM pendant le commit, avant les effets passifs : c'est la frame réellement peinte.
  function Probe({ id }: { id: number }) {
    useLayoutEffect(() => { seen.push((screen.getByRole("button", { name: "Autoriser" }) as HTMLButtonElement).disabled); }, [id]);
    return null;
  }
  const ui = (id: number) => <><ConfirmCard pending={{ ...base, action_id: id }} onResolve={() => {}} /><Probe id={id} /></>;
  const { rerender } = render(ui(1));
  act(() => { vi.advanceTimersByTime(500); });
  rerender(ui(2));
  expect(seen).toEqual([true, true]);
});

it("monté panneau fermé : Autoriser reste inactif, y compris juste après l'ouverture", () => {
  usePanelStore.setState({ open: false });
  render(<ConfirmCard pending={base} onResolve={() => {}} />);
  const btn = () => screen.getByRole("button", { name: "Autoriser" });
  act(() => { vi.advanceTimersByTime(5000); });
  expect(btn()).toBeDisabled();
  act(() => usePanelStore.setState({ open: true }));
  expect(btn()).toBeDisabled();
});

it("Autoriser s'active 500 ms après l'ouverture du panneau", () => {
  usePanelStore.setState({ open: false });
  render(<ConfirmCard pending={base} onResolve={() => {}} />);
  const btn = () => screen.getByRole("button", { name: "Autoriser" });
  act(() => usePanelStore.setState({ open: true }));
  act(() => { vi.advanceTimersByTime(499); });
  expect(btn()).toBeDisabled();
  act(() => { vi.advanceTimersByTime(1); });
  expect(btn()).toBeEnabled();
});

it("fermer puis rouvrir désarme Autoriser pour 500 ms", () => {
  render(<ConfirmCard pending={base} onResolve={() => {}} />);
  const btn = () => screen.getByRole("button", { name: "Autoriser" });
  act(() => { vi.advanceTimersByTime(500); });
  expect(btn()).toBeEnabled();
  act(() => usePanelStore.setState({ open: false }));
  expect(btn()).toBeDisabled();
  act(() => usePanelStore.setState({ open: true }));
  expect(btn()).toBeDisabled();
  act(() => { vi.advanceTimersByTime(499); });
  expect(btn()).toBeDisabled();
  act(() => { vi.advanceTimersByTime(1); });
  expect(btn()).toBeEnabled();
});
