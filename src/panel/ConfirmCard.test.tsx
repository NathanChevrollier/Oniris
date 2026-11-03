import { render, screen, fireEvent, cleanup, act } from "@testing-library/react";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { ConfirmCard } from "./ConfirmCard";
import type { PendingAction } from "../lib/ipc";

beforeEach(() => { vi.useFakeTimers(); });
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
