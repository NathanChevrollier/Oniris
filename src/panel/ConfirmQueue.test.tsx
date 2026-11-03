import { render, screen, fireEvent, cleanup, act } from "@testing-library/react";
import { it, expect, vi, afterEach } from "vitest";
import { ConfirmQueue } from "./ConfirmQueue";
import { usePanelStore } from "./store";
import type { PendingAction } from "../lib/ipc";

afterEach(() => { cleanup(); usePanelStore.setState({ pending: [] }); });
const mk = (id: number, summary: string): PendingAction => ({
  action_id: id, tool: "t", text: { summary, details: "d", risk: "Low" }, provenance: "User", expires_at_ms: 0,
});

it("deux confirmations en file : résoudre la première affiche la seconde", async () => {
  let push: (p: PendingAction) => void = () => {};
  const subscribe = (cb: (p: PendingAction) => void) => { push = cb; return Promise.resolve(() => {}); };
  const resolve = vi.fn();
  render(<ConfirmQueue subscribe={subscribe} resolve={resolve} />);
  act(() => { push(mk(1, "Première")); push(mk(2, "Seconde")); });
  expect(screen.getByText("Première")).toBeInTheDocument();
  expect(screen.queryByText("Seconde")).toBeNull();
  await act(async () => { await new Promise((r) => setTimeout(r, 520)); });
  fireEvent.click(screen.getByRole("button", { name: "Autoriser" }));
  expect(resolve).toHaveBeenCalledWith(1, true);
  expect(screen.getByText("Seconde")).toBeInTheDocument();
});
