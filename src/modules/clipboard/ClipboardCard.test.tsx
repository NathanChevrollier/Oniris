import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { ClipboardCard } from "./ClipboardCard";
import { clipboardIpc, type ClipItem } from "./api";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });
beforeEach(() => usePanelStore.setState({ open: true }));

const items: ClipItem[] = [
  { id: 1, kind: "text", text: "Adresse du client", truncated: false, thumb: null, pinned: true, created_ms: Date.now() },
  { id: 2, kind: "image", text: null, truncated: false, thumb: "iVBORw0KGgo=", pinned: false, created_ms: Date.now() - 3_600_000 },
];

it("un clic sur une entrée appelle clipboard_paste(id)", async () => {
  vi.spyOn(clipboardIpc, "list").mockResolvedValue(items);
  const paste = vi.spyOn(clipboardIpc, "paste").mockResolvedValue();
  render(<ClipboardCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Coller Adresse du client" }));
  expect(paste).toHaveBeenCalledWith(1);
  const img = screen.getByRole("img", { name: "Image copiée" });
  expect(img).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo=");
  await userEvent.click(screen.getByRole("button", { name: "Coller l'image" }));
  expect(paste).toHaveBeenCalledWith(2);
});

it("la recherche est temporisée de 150 ms", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  const list = vi.spyOn(clipboardIpc, "list").mockResolvedValue([]);
  render(<ClipboardCard />);
  await act(async () => {});
  list.mockClear();
  fireEvent.change(screen.getByRole("searchbox", { name: "Rechercher dans le presse-papiers" }), { target: { value: "adr" } });
  await act(async () => { vi.advanceTimersByTime(149); });
  expect(list).not.toHaveBeenCalled();
  await act(async () => { vi.advanceTimersByTime(1); });
  expect(list).toHaveBeenCalledWith("adr");
});

it("épingler et supprimer appellent l'IPC sans coller", async () => {
  vi.spyOn(clipboardIpc, "list").mockResolvedValue(items);
  const paste = vi.spyOn(clipboardIpc, "paste").mockResolvedValue();
  const pin = vi.spyOn(clipboardIpc, "pin").mockResolvedValue();
  const remove = vi.spyOn(clipboardIpc, "remove").mockResolvedValue();
  render(<ClipboardCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Désépingler Adresse du client" }));
  expect(pin).toHaveBeenCalledWith(1, false);
  await userEvent.click(screen.getByRole("button", { name: "Supprimer l'image" }));
  expect(remove).toHaveBeenCalledWith(2);
  expect(paste).not.toHaveBeenCalled();
});

it("affiche l'erreur quand le collage échoue", async () => {
  vi.spyOn(clipboardIpc, "list").mockResolvedValue(items);
  vi.spyOn(clipboardIpc, "paste").mockRejectedValue("presse-papiers verrouillé");
  render(<ClipboardCard />);
  await userEvent.click(await screen.findByRole("button", { name: "Coller Adresse du client" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Presse-papiers indisponible");
});
